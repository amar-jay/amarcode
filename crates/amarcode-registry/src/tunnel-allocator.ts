/** Persistent, authenticated named-tunnel provisioning. No chat traffic passes here. */
export interface TunnelEnv {
  TUNNEL_ALLOCATOR?: DurableObjectNamespace;
  TUNNEL_CF_API_TOKEN?: string;
  TUNNEL_CF_ACCOUNT_ID?: string;
  TUNNEL_CF_ZONE_ID?: string;
  TUNNEL_DOMAIN?: string;
  TUNNEL_CAPACITY?: string;
}
type OriginTls = { server_name: string; ca_pool?: string };
type Installation = {
  originTls?: OriginTls;
  credentialHash: string;
  hostname: string;
  previousHostname?: string;
  retiredHostnames?: string[];
  port: number;
  status:
    | "provisioning"
    | "ready"
    | "reclaiming"
    | "reclaimed"
    | "deleting"
    | "deleted";
  disconnectedSince?: number;
  tunnelId?: string;
  dnsId?: string;
  dnsReadyAfter?: number;
};
type Database = Record<string, Installation>;
export interface AllocationStorage {
  get<T>(key: string): Promise<T | undefined>;
  put(key: string, value: unknown): Promise<void>;
}
export const TUNNEL_IDLE_GRACE_MS = 7 * 24 * 60 * 60 * 1000;
const CLEANUP_PATH = "/internal/cleanup";
function newHostname(db: Database, domain: string): string {
  const reserved = new Set(
    Object.values(db).flatMap((record) =>
      [
        record.hostname,
        record.previousHostname,
        ...(record.retiredHostnames ?? []),
      ].filter((value): value is string => Boolean(value)),
    ),
  );
  for (let attempt = 0; attempt < 32; attempt++) {
    const label = Array.from(
      crypto.getRandomValues(new Uint8Array(4)),
      (byte) => byte.toString(16).padStart(2, "0"),
    ).join("");
    const hostname = `${label}.${domain}`;
    if (!reserved.has(hostname)) return hostname;
  }
  throw new AllocationError(503, "Cannot allocate a unique hostname; retry.");
}
const UUID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const SECRET = /^[a-f0-9]{64}$/;
const reply = (body: unknown, status = 200) =>
  Response.json(body, {
    status,
    headers: {
      "cache-control": "no-store",
      "x-content-type-options": "nosniff",
    },
  });
async function hash(value: string) {
  return Array.from(
    new Uint8Array(
      await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value)),
    ),
    (b) => b.toString(16).padStart(2, "0"),
  ).join("");
}
class AllocationError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}
class CloudflareApiError extends Error {
  constructor(
    public upstreamStatus: number,
    public codes: number[],
    public operation: string,
  ) {
    super("Cloudflare API failed");
  }
}
// Enrollment is tiny ({port}); bound actual streamed bytes, not Content-Length.
const ENROLLMENT_BODY_LIMIT = 1024;
const ENROLLMENT_BODY_TIMEOUT_MS = 5_000;
async function enrollmentBody(request: Request): Promise<unknown> {
  if (!request.body) throw new AllocationError(400, "Invalid enrollment JSON.");
  const reader = request.body.getReader();
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      (async () => {
        const decoder = new TextDecoder();
        let size = 0;
        let text = "";
        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          size += value.byteLength;
          if (size > ENROLLMENT_BODY_LIMIT)
            throw new AllocationError(
              413,
              "Enrollment body exceeds 1024 bytes.",
            );
          text += decoder.decode(value, { stream: true });
        }
        text += decoder.decode();
        try {
          return JSON.parse(text) as unknown;
        } catch {
          throw new AllocationError(400, "Invalid enrollment JSON.");
        }
      })(),
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => {
          reject(new AllocationError(408, "Enrollment body timed out."));
        }, ENROLLMENT_BODY_TIMEOUT_MS);
      }),
    ]);
  } catch (error) {
    if (error instanceof AllocationError) throw error;
    throw new AllocationError(400, "Could not read enrollment body.");
  } finally {
    clearTimeout(timer);
    // A hostile stream's cancellation hook must not delay the response.
    void reader.cancel().catch(() => undefined);
  }
}
export class Allocator {
  private queue: Promise<unknown> = Promise.resolve();
  constructor(
    private storage: AllocationStorage,
    private env: TunnelEnv,
    private fetcher: typeof fetch = fetch,
  ) {}

  // Read untrusted bodies outside the queue. Only state-dependent work and
  // provisioning are serialized, so a slow upload cannot block other clients.
  fetch(request: Request): Promise<Response> {
    return (async () => {
      const body =
        request.method === "PUT" ? await enrollmentBody(request) : undefined;
      const next = this.queue.then(() => this.handle(request, body));
      this.queue = next.catch(() => undefined);
      return next;
    })().catch((error) =>
      reply(
        {
          ...(error instanceof CloudflareApiError
            ? {
                upstream_status: error.upstreamStatus,
                upstream_codes: error.codes,
                operation: error.operation,
              }
            : {}),
          ...(error instanceof AllocationError ||
          error instanceof CloudflareApiError
            ? {}
            : {
                failure_type: error instanceof Error ? error.name : "unknown",
              }),
          error:
            error instanceof AllocationError
              ? error.message
              : "Tunnel provisioning failed; retry the same installation.",
        },
        error instanceof AllocationError ? error.status : 502,
      ),
    );
  }
  // Share the same queue as enrollment/removal so capacity and cleanup cannot race.
  cleanup(now = Date.now()) {
    const next = this.queue.then(() => this.sweep(now));
    this.queue = next.catch(() => undefined);
    return next;
  }

  private async sweep(now: number) {
    const summary = { reclaimed: 0, failed: 0 };
    if (
      !this.env.TUNNEL_CF_API_TOKEN ||
      !this.env.TUNNEL_CF_ACCOUNT_ID ||
      !this.env.TUNNEL_CF_ZONE_ID
    )
      throw new Error("Named tunnels are not configured");
    const db = (await this.storage.get<Database>("installations")) ?? {};
    const account = `/accounts/${this.env.TUNNEL_CF_ACCOUNT_ID}/cfd_tunnel`;
    for (const [id, record] of Object.entries(db)) {
      if (["deleted", "deleting", "reclaimed"].includes(record.status))
        continue;
      try {
        if (!record.tunnelId) {
          const tunnels = await this.api<{ id: string; name: string }[]>(
            `${account}?name=amarcode-${id}&is_deleted=false`,
          );
          record.tunnelId = tunnels.find(
            (t) => t.name === `amarcode-${id}`,
          )?.id;
        }
        const tunnel = record.tunnelId
          ? await this.api<{ status: string; deleted_at?: string | null }>(
              `${account}/${record.tunnelId}`,
              "GET",
              undefined,
              true,
            )
          : undefined;
        // Unknown/degraded status is never evidence of disconnection.
        if (
          tunnel &&
          !tunnel.deleted_at &&
          tunnel.status !== "down" &&
          tunnel.status !== "inactive"
        ) {
          delete record.disconnectedSince;
          if (record.status === "reclaiming") record.status = "ready";
          await this.storage.put("installations", db);
          continue;
        }
        record.disconnectedSince ??= now;
        await this.storage.put("installations", db);
        if (now - record.disconnectedSince < TUNNEL_IDLE_GRACE_MS) continue;
        record.status = "reclaiming";
        await this.storage.put("installations", db);
        // Do not force-disconnect or rotate credentials here. Cloudflare refuses
        // ordinary tunnel deletion if a connector raced us and reconnected.
        if (record.tunnelId && !tunnel?.deleted_at)
          await this.api(`${account}/${record.tunnelId}`, "DELETE");
        await this.removeDns(record);
        delete record.tunnelId;
        delete record.dnsId;
        delete record.dnsReadyAfter;
        delete record.previousHostname;
        delete record.disconnectedSince;
        record.status = "reclaimed";
        await this.storage.put("installations", db);
        summary.reclaimed++;
      } catch {
        // Preserve capacity and retry partial deletions on the next daily run.
        summary.failed++;
      }
    }
    return summary;
  }

  private async removeDns(record: Installation) {
    if (record.previousHostname) {
      await this.removeDns({
        ...record,
        hostname: record.previousHostname,
        previousHostname: undefined,
      });
    }
    const dns = `/zones/${this.env.TUNNEL_CF_ZONE_ID}/dns_records`;
    const records = await this.api<{ id: string; content: string }[]>(
      `${dns}?type=CNAME&name=${encodeURIComponent(record.hostname)}`,
    );
    for (const entry of records) {
      if (entry.content !== `${record.tunnelId}.cfargotunnel.com`)
        throw new Error("DNS ownership mismatch");
      await this.api(`${dns}/${entry.id}`, "DELETE");
    }
  }

  private async api<T>(
    path: string,
    method = "GET",
    body?: unknown,
    allowMissing = false,
  ): Promise<T> {
    // Use the global receiver when invoking native Workers fetch.
    const response = await this.fetcher.call(
      globalThis,
      `https://api.cloudflare.com/client/v4${path}`,
      {
        method,
        headers: {
          authorization: `Bearer ${this.env.TUNNEL_CF_API_TOKEN}`,
          "content-type": "application/json",
          "user-agent": "amarcode-tunnel-allocator/1.0",
        },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: AbortSignal.timeout(10_000),
        redirect: "manual",
      },
    );
    if ((method === "DELETE" || allowMissing) && response.status === 404)
      return undefined as T;
    const operation = `${method} ${path.split("?")[0]}`;
    const value = (await response.json().catch(() => {
      throw new CloudflareApiError(response.status, [], operation);
    })) as { success: boolean; result: T; errors?: { code: number }[] };
    if (!response.ok || !value.success)
      throw new CloudflareApiError(
        response.status,
        (value.errors ?? []).map((e) => e.code),
        operation,
      );
    return value.result;
  }
  private async handle(
    request: Request,
    enrollment: unknown,
  ): Promise<Response> {
    const env = this.env;
    if (
      !env.TUNNEL_CF_API_TOKEN ||
      !env.TUNNEL_CF_ACCOUNT_ID ||
      !env.TUNNEL_CF_ZONE_ID ||
      !env.TUNNEL_DOMAIN
    )
      return reply({ error: "Named tunnels are not configured." }, 503);
    const domain = env.TUNNEL_DOMAIN;
    // Machine names add one label; nested parent domains need matching edge certificates.
    if (!/^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,}$/.test(domain))
      return reply({ error: "Invalid tunnel domain configuration." }, 503);
    const url = new URL(request.url);
    const id = url.pathname.slice("/v1/tunnels/".length);
    if (!UUID.test(id) || url.search)
      return reply({ error: "Invalid installation ID." }, 400);
    if (!["PUT", "DELETE"].includes(request.method))
      return reply({ error: "Use PUT or DELETE." }, 405);
    const secret =
      request.headers
        .get("authorization")
        ?.match(/^Bearer ([a-f0-9]{64})$/)?.[1] ?? "";
    if (!SECRET.test(secret))
      return reply({ error: "Invalid installation credential." }, 401);
    const credentialHash = await hash(secret);
    const db = (await this.storage.get<Database>("installations")) ?? {};
    let record = db[id];
    if (record && record.credentialHash !== credentialHash)
      return reply({ error: "Invalid installation credential." }, 401);
    let port: number | undefined;
    let originTls: OriginTls | undefined;
    if (request.method === "PUT") {
      const body = enrollment as {
        port?: unknown;
        origin_tls?: unknown;
      } | null;
      if (
        !body ||
        !Number.isInteger(body.port) ||
        Number(body.port) < 1 ||
        Number(body.port) > 65535
      )
        return reply({ error: "Invalid listener port." }, 400);
      port = Number(body.port);
      if (body.origin_tls !== undefined) {
        const tls = body.origin_tls as Partial<OriginTls> | null;
        if (
          !tls ||
          typeof tls !== "object" ||
          typeof tls.server_name !== "string" ||
          tls.server_name.length > 253 ||
          !/^[a-zA-Z0-9](?:[a-zA-Z0-9.-]*[a-zA-Z0-9])?$/.test(
            tls.server_name,
          ) ||
          (tls.ca_pool !== undefined &&
            (typeof tls.ca_pool !== "string" ||
              tls.ca_pool.length > 512 ||
              !/^(?:\/|[A-Za-z]:[\\/])/.test(tls.ca_pool) ||
              /[\r\n\0]/.test(tls.ca_pool)))
        )
          return reply({ error: "Invalid origin TLS settings." }, 400);
        originTls = {
          server_name: tls.server_name,
          ...(tls.ca_pool !== undefined ? { ca_pool: tls.ca_pool } : {}),
        };
      }
    }
    if (
      !record ||
      (record.status === "reclaimed" && request.method === "PUT")
    ) {
      if (request.method === "DELETE")
        return reply({ error: "Installation not found." }, 404);
      const capacity = Number(env.TUNNEL_CAPACITY ?? "5");
      if (!Number.isInteger(capacity) || capacity < 1)
        return reply({ error: "Invalid capacity configuration." }, 503);
      if (
        Object.values(db).filter(
          (r) => !["deleted", "reclaimed"].includes(r.status),
        ).length >= capacity
      )
        return reply({ error: "Installation capacity reached." }, 409);
      record = db[id] = {
        credentialHash,
        hostname: record?.hostname ?? newHostname(db, domain),
        retiredHostnames: record?.retiredHostnames,
        port: port!,
        originTls,
        status: "provisioning",
        disconnectedSince: Date.now(),
      };
      await this.storage.put("installations", db); // Reserve capacity BEFORE provisioning.
    }
    if (
      request.method === "PUT" &&
      ["ready", "provisioning"].includes(record.status)
    ) {
      // Older daemons only accept m-<full installation ID> names. Keep them
      // operational while newer daemons opt in to short hostnames.
      const shortNames =
        request.headers.get("x-amarcode-tunnel-hostnames") === "short-v2";
      const label = record.hostname.endsWith(`.${domain}`)
        ? record.hostname.slice(0, -domain.length - 1)
        : "";
      const desired = shortNames
        ? /^[a-f0-9]{8}$/.test(label)
          ? record.hostname
          : newHostname(db, domain)
        : `m-${id}.${domain}`;
      if (record.hostname !== desired && !record.previousHostname) {
        record.retiredHostnames = [
          ...(record.retiredHostnames ?? []),
          record.hostname,
        ];
        record.previousHostname = record.hostname;
        record.hostname = desired;
        record.status = "provisioning";
        delete record.dnsReadyAfter;
        await this.storage.put("installations", db);
      }
    }
    if (
      port !== undefined &&
      record.port !== port &&
      (record.status === "ready" || record.status === "provisioning")
    ) {
      record.port = port;
      record.status = "provisioning";
      await this.storage.put("installations", db);
    }
    if (
      request.method === "PUT" &&
      ["ready", "provisioning"].includes(record.status) &&
      JSON.stringify(record.originTls) !== JSON.stringify(originTls)
    ) {
      record.originTls = originTls;
      record.status = "provisioning";
      await this.storage.put("installations", db);
    }
    if (request.method === "PUT" && record.status === "reclaiming")
      return reply(
        { error: "Tunnel cleanup is pending; retry after daily cleanup." },
        409,
      );
    if (
      request.method === "PUT" &&
      ["ready", "provisioning"].includes(record.status)
    ) {
      // Authenticated reconnects receive a full grace period to start cloudflared.
      record.disconnectedSince = Date.now();
      await this.storage.put("installations", db);
    }
    const account = `/accounts/${env.TUNNEL_CF_ACCOUNT_ID}/cfd_tunnel`;
    const dns = `/zones/${env.TUNNEL_CF_ZONE_ID}/dns_records`;
    const name = `amarcode-${id}`;
    // Recover a successful create whose response or storage write was lost.
    if (!record.tunnelId && !["deleted", "reclaimed"].includes(record.status)) {
      const existing = await this.api<{ id: string; name: string }[]>(
        `${account}?name=${name}&is_deleted=false`,
      );
      record.tunnelId = existing.find((t) => t.name === name)?.id;
      if (record.tunnelId) await this.storage.put("installations", db);
    }
    if (request.method === "DELETE") {
      if (record.status === "deleted") return reply({ status: "deleted" });
      record.status = "deleting";
      await this.storage.put("installations", db);
      await this.removeDns(record);
      if (record.tunnelId) {
        // Stop old credentials from reconnecting between disconnection and deletion.
        const secret = btoa(
          String.fromCharCode(...crypto.getRandomValues(new Uint8Array(32))),
        );
        await this.api(
          `${account}/${record.tunnelId}`,
          "PATCH",
          { tunnel_secret: secret },
          true,
        );
        await this.api(`${account}/${record.tunnelId}/connections`, "DELETE");
        await this.api(`${account}/${record.tunnelId}`, "DELETE");
      }
      record.status = "deleted";
      await this.storage.put("installations", db); // Tombstone: hostname is never reused.
      return reply({ status: "deleted" });
    }
    if (record.status === "deleted" || record.status === "deleting")
      return reply(
        { error: "Installation removed; enroll a new installation." },
        410,
      );
    if (!record.tunnelId) {
      const tunnel = await this.api<{ id: string }>(account, "POST", {
        name,
        config_src: "cloudflare",
      });
      record.tunnelId = tunnel.id;
      await this.storage.put("installations", db);
    }
    if (record.status !== "ready") {
      await this.api(`${account}/${record.tunnelId}/configurations`, "PUT", {
        config: {
          ingress: [
            {
              hostname: record.hostname,
              service: `${record.originTls ? "https" : "http"}://127.0.0.1:${record.port}`,
              ...(record.originTls
                ? {
                    originRequest: {
                      originServerName: record.originTls.server_name,
                      ...(record.originTls.ca_pool
                        ? { caPool: record.originTls.ca_pool }
                        : {}),
                      noTLSVerify: false,
                    },
                  }
                : {}),
            },
            { service: "http_status:404" },
          ],
        },
      });
      const records = await this.api<{ id: string; content: string }[]>(
        `${dns}?type=CNAME&name=${encodeURIComponent(record.hostname)}`,
      );
      if (
        records.some((r) => r.content !== `${record.tunnelId}.cfargotunnel.com`)
      )
        throw new Error("DNS ownership mismatch");
      record.dnsId =
        records[0]?.id ??
        (
          await this.api<{ id: string }>(dns, "POST", {
            type: "CNAME",
            proxied: true,
            name: record.hostname,
            content: `${record.tunnelId}.cfargotunnel.com`,
            ttl: 1,
          })
        ).id;
      // Let new authoritative DNS records propagate before clients query them.
      // An immediate lookup can seed a resolver's negative cache for minutes.
      record.dnsReadyAfter ??= Date.now() + 15_000;
      if (record.previousHostname) {
        await this.removeDns({
          ...record,
          hostname: record.previousHostname,
          previousHostname: undefined,
        });
        delete record.previousHostname;
      }
      record.status = "ready";
      await this.storage.put("installations", db);
    }
    const token = await this.api<string>(`${account}/${record.tunnelId}/token`);
    return reply({
      installation_id: id,
      hostname: record.hostname,
      url: `wss://${record.hostname}/rpc`,
      tunnel_token: token,
      port: record.port,
      dns_ready_after: record.dnsReadyAfter,
      origin_tls: record.originTls,
    });
  }
}

export class TunnelAllocator {
  private allocator: Allocator;
  constructor(state: DurableObjectState, env: TunnelEnv) {
    this.allocator = new Allocator(state.storage, env);
  }
  async fetch(request: Request) {
    // Only reachable via the Worker binding; public routes forward /v1/tunnels/ only.
    if (new URL(request.url).pathname === CLEANUP_PATH) {
      if (request.method !== "POST") return reply({ error: "Use POST." }, 405);
      return reply(await this.allocator.cleanup());
    }
    return this.allocator.fetch(request);
  }
}

export async function routeTunnelAllocation(
  request: Request,
  env: TunnelEnv,
): Promise<Response | null> {
  if (!new URL(request.url).pathname.startsWith("/v1/tunnels/")) return null;
  if (!env.TUNNEL_ALLOCATOR)
    return reply({ error: "Named tunnels are not configured." }, 503);
  return env.TUNNEL_ALLOCATOR.get(
    env.TUNNEL_ALLOCATOR.idFromName("installations-v1"),
  ).fetch(request);
}

export async function cleanupTunnelAllocations(env: TunnelEnv): Promise<void> {
  if (!env.TUNNEL_ALLOCATOR)
    throw new Error("Tunnel allocator binding is missing");
  const response = await env.TUNNEL_ALLOCATOR.get(
    env.TUNNEL_ALLOCATOR.idFromName("installations-v1"),
  ).fetch(new Request(`https://allocator${CLEANUP_PATH}`, { method: "POST" }));
  if (!response.ok) throw new Error("Tunnel cleanup failed");
  const summary = (await response.json()) as {
    reclaimed: number;
    failed: number;
  };
  console.log("Tunnel cleanup", summary);
  if (summary.failed)
    throw new Error("Some tunnel cleanups failed; retained for retry");
}
