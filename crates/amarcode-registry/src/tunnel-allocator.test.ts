import { describe, expect, test, spyOn } from "bun:test";
import { readFileSync } from "node:fs";
import {
  Allocator,
  DEFAULT_CAPACITY,
  TUNNEL_IDLE_GRACE_MS,
  TUNNEL_REUSE_MS,
  cleanupTunnelAllocations,
  routeTunnelAllocation,
  type AllocationStorage,
  type TunnelEnv,
} from "./tunnel-allocator";

class Memory implements AllocationStorage {
  values = new Map<string, unknown>();
  async get<T>(key: string) {
    return structuredClone(this.values.get(key)) as T | undefined;
  }
  async put(key: string, value: unknown) {
    this.values.set(key, structuredClone(value));
  }
}
function fixture(
  capacity = "5",
  before?: (url: URL, method: string) => Promise<void>,
) {
  const storage = new Memory();
  const env: TunnelEnv = {
    TUNNEL_CF_API_TOKEN: "account-secret",
    TUNNEL_CF_ACCOUNT_ID: "account",
    TUNNEL_CF_ZONE_ID: "zone",
    TUNNEL_DOMAIN: "example.com",
    TUNNEL_CAPACITY: capacity,
    TUNNEL_ENROLLMENT_CODE: "enrollment-code",
  };
  const tunnels = new Map<string, { id: string; name: string }>();
  const deletedTunnels = new Map<
    string,
    { id: string; name: string; deleted_at: string }
  >();
  const records = new Map<
    string,
    { id: string; name: string; content: string }
  >();
  const calls: string[] = [];
  const services: string[] = [];
  const originSettings: unknown[] = [];
  let loseCreateResponse = false;
  let failConfiguration = false;
  let loseDnsResponse = false;
  let failDisconnect = false;
  let redirectApi = false;
  let tunnelStatus = "inactive";
  // Queued upstream responses, consumed one per request before normal handling.
  const failures: Response[] = [];
  let failStatus: boolean | string = false;
  let failDnsDelete = false;
  let reconnectOnDelete = false;
  let loseDeleteResponse = false;
  const fetcher = async function (
    this: unknown,
    input: string | URL | Request,
    init?: RequestInit,
  ) {
    expect(this).toBe(globalThis);
    // Workers throws before sending requests when redirect is set to "error".
    expect(init?.redirect).toBe("manual");
    const url = new URL(String(input));
    const method = init?.method ?? "GET";
    const path = url.pathname;
    calls.push(`${method} ${path}`);
    await before?.(url, method);
    expect(new Headers(init?.headers).get("authorization")).toBe(
      "Bearer account-secret",
    );
    const scripted = failures.shift();
    if (scripted) return scripted;
    if (redirectApi)
      return new Response(null, {
        status: 302,
        headers: { location: "https://untrusted.example" },
      });
    let result: unknown;
    if (path.endsWith("/cfd_tunnel")) {
      if (method === "GET")
        result = [...tunnels.values()].filter(
          (t) => t.name === url.searchParams.get("name"),
        );
      else {
        const body = JSON.parse(String(init?.body));
        const id = crypto.randomUUID();
        const value = { id, name: body.name };
        tunnels.set(id, value);
        if (loseCreateResponse) {
          loseCreateResponse = false;
          throw new Error("connection lost after creation");
        }
        result = value;
      }
    } else if (path.endsWith("/configurations")) {
      if (failConfiguration) {
        failConfiguration = false;
        throw new Error("configuration unavailable");
      }
      const ingress = JSON.parse(String(init?.body)).config.ingress;
      expect(ingress[0].service).toMatch(/^https?:\/\/127\.0\.0\.1:\d+$/);
      services.push(ingress[0].service);
      originSettings.push(ingress[0].originRequest);
      result = {};
    } else if (path.endsWith("/dns_records")) {
      if (method === "GET")
        result = [...records.values()].filter(
          (r) => r.name === url.searchParams.get("name"),
        );
      else {
        const value = {
          ...JSON.parse(String(init?.body)),
          id: crypto.randomUUID(),
        };
        records.set(value.id, value);
        if (loseDnsResponse) {
          loseDnsResponse = false;
          throw new Error("lost DNS response");
        }
        result = value;
      }
    } else if (path.endsWith("/token")) result = "connector-token";
    else if (path.endsWith("/connections")) {
      if (failDisconnect) {
        failDisconnect = false;
        throw new Error("disconnect failed");
      }
      result = {};
    } else if (method === "PATCH" && path.includes("/cfd_tunnel/")) {
      expect(
        JSON.parse(String(init?.body)).tunnel_secret.length,
      ).toBeGreaterThan(32);
      result = {};
    } else if (method === "DELETE" && path.includes("/dns_records/")) {
      if (failDnsDelete) {
        failDnsDelete = false;
        throw new Error("DNS deletion unavailable");
      }
      records.delete(path.split("/").at(-1)!);
      result = {};
    } else if (method === "DELETE" && path.includes("/cfd_tunnel/")) {
      if (reconnectOnDelete) throw new Error("Tunnel has active connections");
      const id = path.split("/").at(-1)!;
      const tunnel = tunnels.get(id);
      if (deletedTunnels.has(id)) throw new Error("Tunnel already deleted");
      if (tunnel)
        deletedTunnels.set(id, {
          ...tunnel,
          deleted_at: new Date().toISOString(),
        });
      tunnels.delete(id);
      if (loseDeleteResponse) {
        loseDeleteResponse = false;
        throw new Error("Lost tunnel deletion response");
      }
      result = {};
    } else if (method === "GET" && path.includes("/cfd_tunnel/")) {
      if (failStatus === true || failStatus === path.split("/").at(-1))
        throw new Error("status API unavailable");
      const tunnel =
        tunnels.get(path.split("/").at(-1)!) ??
        deletedTunnels.get(path.split("/").at(-1)!);
      if (!tunnel) return new Response(null, { status: 404 });
      result = { ...tunnel, status: tunnelStatus };
    } else throw new Error(`Unexpected API request ${method} ${path}`);
    return Response.json({ success: true, result });
  } as typeof fetch;
  const restart = () => new Allocator(storage, env, fetcher);
  return {
    allocator: restart(),
    fetcher,
    storage,
    setStatus: (value: string) => {
      tunnelStatus = value;
    },
    failStatus: (value: boolean | string) => {
      failStatus = value;
    },
    failDnsDelete: () => {
      failDnsDelete = true;
    },
    reconnectOnDelete: () => {
      reconnectOnDelete = true;
    },
    loseDelete: () => {
      loseDeleteResponse = true;
    },
    restart,
    tunnels,
    records,
    calls,
    services,
    originSettings,
    redirectApi: () => {
      redirectApi = true;
    },
    fail: (...responses: Response[]) => {
      failures.push(...responses);
    },
    loseDns: () => {
      loseDnsResponse = true;
    },
    failDisconnect: () => {
      failDisconnect = true;
    },
    loseCreate: () => {
      loseCreateResponse = true;
    },
    failConfiguration: () => {
      failConfiguration = true;
    },
  };
}
const request = (
  id: string,
  options: {
    method?: string;
    secret?: string;
    port?: number;
    enrollmentCode?: string | null;
  } = {},
) =>
  new Request(`https://allocator.example/v1/tunnels/${id}`, {
    method: options.method ?? "PUT",
    headers: {
      authorization: `Bearer ${options.secret ?? "a".repeat(64)}`,
      "content-type": "application/json",
      ...(options.enrollmentCode === null
        ? {}
        : {
            "x-amarcode-enrollment-code":
              options.enrollmentCode ?? "enrollment-code",
          }),
    },
    body:
      options.method === "DELETE"
        ? undefined
        : JSON.stringify({ port: options.port ?? 43721 }),
  });

describe("upstream failure handling", () => {
  const apiError = (status: number, headers?: HeadersInit) =>
    new Response(JSON.stringify({ success: false, errors: [{ code: 1000 }] }), {
      status,
      headers: { "content-type": "application/json", ...headers },
    });

  test("capacity default tracks the deployed Wrangler configuration", () => {
    const config = readFileSync(
      new URL("../wrangler.jsonc", import.meta.url),
      "utf8",
    );
    const parsed = JSON.parse(config.replace(/,(?=\s*[}\]])/g, "")) as {
      vars: { TUNNEL_CAPACITY: string };
    };
    expect(Number(parsed.vars.TUNNEL_CAPACITY)).toBe(DEFAULT_CAPACITY);
  });

  test("an unset capacity uses the default instead of a smaller legacy value", async () => {
    const f = fixture();
    const allocator = new Allocator(
      f.storage,
      {
        TUNNEL_CF_API_TOKEN: "account-secret",
        TUNNEL_CF_ACCOUNT_ID: "account",
        TUNNEL_CF_ZONE_ID: "zone",
        TUNNEL_DOMAIN: "example.com",
        TUNNEL_ENROLLMENT_CODE: "enrollment-code",
      },
      f.fetcher,
    );
    // The previous fallback of 5 would reject the sixth installation.
    for (let index = 0; index < 6; index++)
      expect(
        (await allocator.fetch(request(crypto.randomUUID()))).status,
        `installation ${index + 1}`,
      ).toBe(200);
  });

  test("replays rate-limited requests after honoring Retry-After", async () => {
    const f = fixture();
    f.fail(apiError(429, { "retry-after": "0" }));
    const response = await f.allocator.fetch(request(crypto.randomUUID()));
    expect(response.status).toBe(200);
    expect(
      f.calls.filter(
        (call) => call === "GET /client/v4/accounts/account/cfd_tunnel",
      ),
    ).toHaveLength(2);
    expect(f.tunnels.size).toBe(1);
  });

  test("replays a rate-limited tunnel creation but never a failed one", async () => {
    const ok = (result: unknown) =>
      new Response(JSON.stringify({ success: true, result }), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    const create = "POST /client/v4/accounts/account/cfd_tunnel";

    // Let the recovery lookup succeed so the failure lands on the create.
    const rateLimited = fixture();
    rateLimited.fail(ok([]), apiError(429, { "retry-after": "0" }));
    const created = await rateLimited.allocator.fetch(
      request(crypto.randomUUID()),
    );
    expect(created.status).toBe(200);
    expect(rateLimited.calls.filter((call) => call === create)).toHaveLength(2);
    expect(rateLimited.tunnels.size).toBe(1);

    // A 5xx may have created the tunnel, so replaying POST could duplicate it.
    const failed = fixture();
    failed.fail(ok([]), apiError(500));
    const rejected = await failed.allocator.fetch(request(crypto.randomUUID()));
    expect(rejected.status).toBe(502);
    expect(failed.calls.filter((call) => call === create)).toHaveLength(1);
    expect(failed.tunnels.size).toBe(0);
  });

  test("gives up instead of retrying when upstream asks for a long wait", async () => {
    const f = fixture();
    f.fail(apiError(429, { "retry-after": "120" }));
    const response = await f.allocator.fetch(request(crypto.randomUUID()));
    expect(response.status).toBe(502);
    const body = (await response.json()) as {
      upstream_status: number;
      upstream_codes: number[];
    };
    expect(body.upstream_status).toBe(429);
    expect(body.upstream_codes).toEqual([1000]);
    expect(f.calls).toHaveLength(1);
    expect(f.tunnels.size).toBe(0);
  });

  test("does not retry client errors", async () => {
    const f = fixture();
    f.fail(apiError(400));
    const response = await f.allocator.fetch(request(crypto.randomUUID()));
    expect(response.status).toBe(502);
    expect(f.calls).toHaveLength(1);
  });

  test("retries transient 5xx reads without duplicating resources", async () => {
    const f = fixture();
    f.fail(apiError(503), apiError(502));
    const response = await f.allocator.fetch(request(crypto.randomUUID()));
    expect(response.status).toBe(200);
    expect(
      f.calls.filter(
        (call) => call === "GET /client/v4/accounts/account/cfd_tunnel",
      ),
    ).toHaveLength(3);
    expect(f.tunnels.size).toBe(1);
    expect(f.records.size).toBe(1);
  });
});

describe("persistent tunnel allocation", () => {
  test("rejects Cloudflare API redirects without forwarding credentials", async () => {
    const f = fixture();
    f.redirectApi();
    const response = await f.allocator.fetch(request(crypto.randomUUID()));
    expect(response.status).toBe(502);
    const body = await response.text();
    expect(body).toContain('"upstream_status":302');
    expect(body).not.toContain("account-secret");
    expect(f.calls).toHaveLength(1);
  });
  test("automatically enrolls with a private credential and reconnects after allocator restart", async () => {
    const f = fixture();
    const id = crypto.randomUUID();
    const first = await f.allocator.fetch(request(id));
    expect(first.status).toBe(200);
    const assignment = (await first.json()) as {
      url: string;
      tunnel_token: string;
      dns_ready_after: number;
      reuse_until?: number;
    };
    expect(assignment.url).toMatch(/^wss:\/\/[a-f0-9]{8}\.example\.com\/rpc$/);
    expect(assignment.tunnel_token).toBe("connector-token");
    expect(assignment.dns_ready_after).toBeGreaterThan(Date.now());
    expect(assignment.dns_ready_after).toBeLessThanOrEqual(Date.now() + 15_000);
    expect(first.headers.get("cache-control")).toBe("no-store");
    const reconnect = await f.restart().fetch(request(id, {}));
    const reconnected = (await reconnect.json()) as typeof assignment;
    expect(reconnected.reuse_until).toBeGreaterThan(Date.now());
    expect(reconnected.reuse_until!).toBeLessThanOrEqual(
      Date.now() + TUNNEL_REUSE_MS,
    );
    delete assignment.reuse_until;
    delete reconnected.reuse_until;
    expect(reconnected).toEqual(assignment);
    expect(f.tunnels.size).toBe(1);
    expect(f.records.size).toBe(1);
  });
  test("short names cannot reuse a retired installation hostname", async () => {
    const f = fixture();
    const id = crypto.randomUUID();
    const first = (await (await f.allocator.fetch(request(id))).json()) as {
      hostname: string;
    };
    await f.allocator.fetch(request(id, { method: "DELETE" }));
    const bytes = new Uint8Array(
      first.hostname
        .slice(0, 8)
        .match(/../g)!
        .map((byte) => parseInt(byte, 16)),
    );
    const random = spyOn(crypto, "getRandomValues");
    random.mockImplementation(((array: Uint8Array) => {
      array.set(bytes);
      return array;
    }) as typeof crypto.getRandomValues);
    try {
      expect(
        (await f.allocator.fetch(request(crypto.randomUUID()))).status,
      ).toBe(503);
      expect(f.tunnels.size).toBe(0);
    } finally {
      random.mockRestore();
    }
    const next = await f.allocator.fetch(request(crypto.randomUUID()));
    expect(next.status).toBe(200);
    expect(((await next.json()) as { hostname: string }).hostname).not.toBe(
      first.hostname,
    );
  });

  test("migrates nested and UUID hostnames on reconnect, recovering interrupted configuration", async () => {
    for (const legacy of [
      "12345678.amarcode.example.com",
      "m-legacy.example.com",
    ]) {
      const f = fixture();
      const id = crypto.randomUUID();
      await f.allocator.fetch(request(id));
      const db =
        await f.storage.get<
          Record<string, { hostname: string; dnsReadyAfter?: number }>
        >("installations");
      db![id].hostname = legacy;
      db![id].dnsReadyAfter = 1;
      await f.storage.put("installations", db);
      for (const record of f.records.values()) record.name = legacy;
      f.failConfiguration();
      expect((await f.restart().fetch(request(id))).status).toBe(502);
      const migrated = await f.restart().fetch(request(id));
      expect(migrated.status).toBe(200);
      const assignment = (await migrated.json()) as {
        hostname: string;
        dns_ready_after: number;
      };
      expect(assignment.hostname).toMatch(/^[a-f0-9]{8}\.example\.com$/);
      expect(assignment.dns_ready_after).toBeGreaterThan(Date.now());
      expect([...f.records.values()].map((record) => record.name)).toEqual([
        assignment.hostname,
      ]);
      expect(f.tunnels.size).toBe(1);
      expect((await f.restart().fetch(request(id))).status).toBe(200);
    }
  });

  test("hostnames are always short labels, never legacy m- names", async () => {
    const f = fixture();
    const id = crypto.randomUUID();
    const response = await f.allocator.fetch(request(id));
    expect(response.status).toBe(200);
    const { hostname } = (await response.json()) as { hostname: string };
    expect(hostname).toMatch(/^[a-f0-9]{8}\.example\.com$/);
    expect(hostname).not.toContain(`m-${id}`);
    expect(f.records.size).toBe(1);
    expect(f.tunnels.size).toBe(1);
  });

  test("listener port changes keep the installation hostname and update ingress", async () => {
    const f = fixture();
    const id = crypto.randomUUID();
    const first = (await (await f.allocator.fetch(request(id))).json()) as {
      url: string;
    };
    const next = await f.restart().fetch(request(id, { port: 43888 }));
    expect(next.status).toBe(200);
    const value = (await next.json()) as { url: string; port: number };
    expect(value.url).toBe(first.url);
    expect(value.port).toBe(43888);
    expect(f.services.at(-1)).toBe("http://127.0.0.1:43888");
    expect(f.tunnels.size).toBe(1);
    expect(f.records.size).toBe(1);
  });
  test("rejects malformed initial credentials and authenticates reconnect and deletion", async () => {
    const f = fixture();
    const id = crypto.randomUUID();
    expect(
      (await f.allocator.fetch(request(id, { secret: "invalid" }))).status,
    ).toBe(401);
    expect(f.calls.length).toBe(0);
    await f.allocator.fetch(request(id));
    const count = f.calls.length;
    for (const method of ["PUT", "DELETE"])
      expect(
        (
          await f.allocator.fetch(
            request(id, { method, secret: "b".repeat(64) }),
          )
        ).status,
      ).toBe(401);
    expect(f.calls.length).toBe(count);
  });
  test("new installations require the enrollment code; reconnects do not", async () => {
    const f = fixture();
    const id = crypto.randomUUID();
    const missing = await f.allocator.fetch(
      request(crypto.randomUUID(), { enrollmentCode: null }),
    );
    expect(missing.status).toBe(403);
    expect(await missing.text()).toContain("enrollment code");
    const wrong = await f.allocator.fetch(
      request(crypto.randomUUID(), { enrollmentCode: "wrong-code" }),
    );
    expect(wrong.status).toBe(403);
    // Rejected enrollments consume no capacity and make no Cloudflare calls.
    expect(f.calls).toHaveLength(0);
    const first = await f.allocator.fetch(request(id));
    expect(first.status).toBe(200);
    const reconnect = await f
      .restart()
      .fetch(request(id, { enrollmentCode: null }));
    expect(reconnect.status).toBe(200);
    const removal = await f
      .restart()
      .fetch(request(id, { method: "DELETE", enrollmentCode: null }));
    expect(removal.status).toBe(200);
  });
  test("removing the configured code restores open enrollment for new installs", async () => {
    const f = fixture();
    // No TUNNEL_ENROLLMENT_CODE means the code is not required (rollback path).
    const open = new Allocator(
      f.storage,
      {
        TUNNEL_CF_API_TOKEN: "account-secret",
        TUNNEL_CF_ACCOUNT_ID: "account",
        TUNNEL_CF_ZONE_ID: "zone",
        TUNNEL_DOMAIN: "example.com",
        TUNNEL_CAPACITY: "5",
      },
      f.fetcher,
    );
    const response = await open.fetch(
      request(crypto.randomUUID(), { enrollmentCode: null }),
    );
    expect(response.status).toBe(200);
    // A configured code is enforced without any additional flag.
    const enforced = await f
      .restart()
      .fetch(request(crypto.randomUUID(), { enrollmentCode: null }));
    expect(enforced.status).toBe(403);
    // Leading/trailing whitespace is trimmed by the runtime, so it is not a mismatch.
    for (const wrong of ["", "wrong-code", "enrollment-cod", "ENROLLMENT-CODE"])
      expect(
        (
          await f.restart().fetch(
            request(crypto.randomUUID(), {
              enrollmentCode: wrong === "" ? null : wrong,
            }),
          )
        ).status,
      ).toBe(403);
  });
  test("simultaneous enrollment cannot overallocate; offline installations retain capacity", async () => {
    const f = fixture("1");
    const results = await Promise.all([
      f.allocator.fetch(request(crypto.randomUUID())),
      f.allocator.fetch(request(crypto.randomUUID())),
    ]);
    expect(results.map((r) => r.status).sort()).toEqual([200, 409]);
    expect((await f.restart().fetch(request(crypto.randomUUID()))).status).toBe(
      409,
    );
    expect(f.tunnels.size).toBe(1);
  });
  test("recovers ambiguous tunnel creation without creating a second tunnel", async () => {
    const f = fixture();
    const id = crypto.randomUUID();
    f.loseCreate();
    expect((await f.allocator.fetch(request(id))).status).toBe(502);
    expect((await f.restart().fetch(request(id, {}))).status).toBe(200);
    expect(f.tunnels.size).toBe(1);
  });
  test("partial provisioning reserves capacity and resumes on retry", async () => {
    const f = fixture("1");
    const id = crypto.randomUUID();
    f.failConfiguration();
    expect((await f.allocator.fetch(request(id))).status).toBe(502);
    expect((await f.allocator.fetch(request(crypto.randomUUID()))).status).toBe(
      409,
    );
    expect((await f.restart().fetch(request(id))).status).toBe(200);
    expect(f.tunnels.size).toBe(1);
  });
  test("removal disconnects and deletes resources, frees capacity, and tombstones the hostname", async () => {
    const f = fixture("1");
    const id = crypto.randomUUID();
    await f.allocator.fetch(request(id));
    expect(
      (await f.allocator.fetch(request(id, { method: "DELETE" }))).status,
    ).toBe(200);
    expect(f.tunnels.size).toBe(0);
    expect(f.records.size).toBe(0);
    expect(
      f.calls.some(
        (call) => call.endsWith("/connections") && call.startsWith("DELETE"),
      ),
    ).toBe(true);
    expect((await f.restart().fetch(request(id))).status).toBe(410);
    expect(
      (await f.restart().fetch(request(id, { method: "DELETE" }))).status,
    ).toBe(200);
    const fresh = await f.restart().fetch(request(crypto.randomUUID()));
    expect(fresh.status).toBe(200);
    expect(((await fresh.json()) as { hostname: string }).hostname).toMatch(
      /^[a-f0-9]{8}\.example\.com$/,
    );
  });
  test("recovers a lost DNS creation response without duplicate records", async () => {
    const f = fixture();
    const id = crypto.randomUUID();
    f.loseDns();
    expect((await f.allocator.fetch(request(id))).status).toBe(502);
    expect((await f.restart().fetch(request(id))).status).toBe(200);
    expect(f.records.size).toBe(1);
    expect(f.tunnels.size).toBe(1);
  });
  test("failed deletion retains capacity and revokes before disconnecting on retry", async () => {
    const f = fixture("1");
    const id = crypto.randomUUID();
    await f.allocator.fetch(request(id));
    f.failDisconnect();
    expect(
      (await f.allocator.fetch(request(id, { method: "DELETE" }))).status,
    ).toBe(502);
    expect((await f.restart().fetch(request(crypto.randomUUID()))).status).toBe(
      409,
    );
    expect((await f.restart().fetch(request(id))).status).toBe(410);
    expect(
      (await f.restart().fetch(request(id, { method: "DELETE" }))).status,
    ).toBe(200);
    const patch = f.calls.findIndex((c) => c.startsWith("PATCH"));
    const disconnect = f.calls.findIndex((c) => c.endsWith("/connections"));
    expect(patch).toBeGreaterThan(-1);
    expect(patch).toBeLessThan(disconnect);
  });
  test("can remove partially provisioned installations after an ambiguous create", async () => {
    const f = fixture();
    const id = crypto.randomUUID();
    f.loseCreate();
    await f.allocator.fetch(request(id));
    expect(
      (await f.restart().fetch(request(id, { method: "DELETE" }))).status,
    ).toBe(200);
    expect(f.tunnels.size).toBe(0);
  });
});

describe("daily tunnel cleanup", () => {
  const later = () => Date.now() + TUNNEL_IDLE_GRACE_MS + 1000;

  test("retains allocations for seven days, then reclaims and permits authenticated owner recovery", async () => {
    const f = fixture("1");
    const id = crypto.randomUUID();
    const first = (await (await f.allocator.fetch(request(id))).json()) as {
      hostname: string;
    };
    expect(
      await f.allocator.cleanup(Date.now() + TUNNEL_IDLE_GRACE_MS - 1000),
    ).toEqual({ reclaimed: 0, failed: 0 });
    expect(await f.restart().cleanup(later())).toEqual({
      reclaimed: 1,
      failed: 0,
    });
    expect(f.records.size).toBe(0);
    expect(f.tunnels.size).toBe(0);
    expect(
      f.calls.some((c) => c.startsWith("PATCH") || c.endsWith("/connections")),
    ).toBe(false);
    expect(
      (await f.restart().fetch(request(id, { secret: "b".repeat(64) }))).status,
    ).toBe(401);
    const recovered = await f.restart().fetch(request(id, {}));
    expect(recovered.status).toBe(200);
    expect(((await recovered.json()) as { hostname: string }).hostname).toBe(
      first.hostname,
    );
    expect(f.tunnels.size).toBe(1);
    expect(f.records.size).toBe(1);
  });

  test("reclaimed owners compete for capacity and explicit removal stays permanent", async () => {
    const f = fixture("1");
    const id = crypto.randomUUID();
    await f.allocator.fetch(request(id));
    await f.allocator.cleanup(later());
    expect((await f.allocator.fetch(request(crypto.randomUUID()))).status).toBe(
      200,
    );
    expect((await f.allocator.fetch(request(id, {}))).status).toBe(409);
    expect(
      (await f.allocator.fetch(request(id, { method: "DELETE" }))).status,
    ).toBe(200);
    expect((await f.allocator.fetch(request(id))).status).toBe(410);
    expect(f.tunnels.size).toBe(1);
  });

  test("healthy, degraded and unknown states reset disconnection tracking", async () => {
    for (const status of ["healthy", "degraded", "unexpected"]) {
      const f = fixture();
      await f.allocator.fetch(request(crypto.randomUUID()));
      f.setStatus(status);
      expect(await f.allocator.cleanup(later())).toEqual({
        reclaimed: 0,
        failed: 0,
      });
      f.setStatus("down");
      await f.allocator.cleanup(later() + 1000);
      expect(await f.allocator.cleanup(later() + 2000)).toEqual({
        reclaimed: 0,
        failed: 0,
      });
      expect(f.tunnels.size).toBe(1);
    }
  });

  test("legacy records receive a full grace period and authenticated reconnect resets it", async () => {
    const f = fixture();
    const id = crypto.randomUUID();
    await f.allocator.fetch(request(id));
    const db =
      await f.storage.get<Record<string, { disconnectedSince?: number }>>(
        "installations",
      );
    delete db![id].disconnectedSince;
    await f.storage.put("installations", db);
    expect(await f.restart().cleanup(later())).toEqual({
      reclaimed: 0,
      failed: 0,
    });
    db![id].disconnectedSince = 1;
    await f.storage.put("installations", db);
    await f.restart().fetch(request(id, {}));
    expect(await f.allocator.cleanup(Date.now())).toEqual({
      reclaimed: 0,
      failed: 0,
    });
  });

  test("status API failures never trigger deletion", async () => {
    const f = fixture();
    await f.allocator.fetch(request(crypto.randomUUID()));
    f.failStatus(true);
    expect(await f.allocator.cleanup(later())).toEqual({
      reclaimed: 0,
      failed: 1,
    });
    expect(f.calls.some((c) => c.startsWith("DELETE"))).toBe(false);
    expect(f.tunnels.size).toBe(1);
  });

  test("a failing installation does not prevent cleanup of other installations", async () => {
    const f = fixture();
    await f.allocator.fetch(request(crypto.randomUUID()));
    await f.allocator.fetch(request(crypto.randomUUID()));
    const failedTunnel = [...f.tunnels.keys()][0];
    f.failStatus(failedTunnel);
    expect(await f.allocator.cleanup(later())).toEqual({
      reclaimed: 1,
      failed: 1,
    });
    expect([...f.tunnels.keys()]).toEqual([failedTunnel]);
    expect(f.records.size).toBe(1);
  });

  test("partial cleanup retains capacity and resumes after allocator restart", async () => {
    const f = fixture("1");
    const id = crypto.randomUUID();
    await f.allocator.fetch(request(id));
    f.failDnsDelete();
    expect(await f.allocator.cleanup(later())).toEqual({
      reclaimed: 0,
      failed: 1,
    });
    expect(f.tunnels.size).toBe(0);
    expect(f.records.size).toBe(1);
    expect((await f.allocator.fetch(request(crypto.randomUUID()))).status).toBe(
      409,
    );
    expect((await f.allocator.fetch(request(id))).status).toBe(409);
    expect(await f.restart().cleanup(later())).toEqual({
      reclaimed: 1,
      failed: 0,
    });
    expect(f.records.size).toBe(0);
    expect((await f.restart().fetch(request(id, {}))).status).toBe(200);
  });

  test("recovers lost deletion responses when Cloudflare returns soft-deleted tunnels", async () => {
    const f = fixture();
    await f.allocator.fetch(request(crypto.randomUUID()));
    f.loseDelete();
    expect(await f.allocator.cleanup(later())).toEqual({
      reclaimed: 0,
      failed: 1,
    });
    expect(f.records.size).toBe(1);
    expect(await f.restart().cleanup(later())).toEqual({
      reclaimed: 1,
      failed: 0,
    });
    expect(f.records.size).toBe(0);
    expect(
      f.calls.filter(
        (c) => c.startsWith("DELETE") && c.includes("/cfd_tunnel/"),
      ),
    ).toHaveLength(1);
  });

  test("cleanup and simultaneous enrollments share the capacity queue", async () => {
    const f = fixture("1");
    await f.allocator.fetch(request(crypto.randomUUID()));
    const [summary, first, second] = await Promise.all([
      f.allocator.cleanup(later()),
      f.allocator.fetch(request(crypto.randomUUID())),
      f.allocator.fetch(request(crypto.randomUUID())),
    ]);
    expect(summary).toEqual({ reclaimed: 1, failed: 0 });
    expect([first.status, second.status]).toEqual([200, 409]);
    expect(f.tunnels.size).toBe(1);
    expect(f.records.size).toBe(1);
  });

  test("cleans up interrupted provisioning even when the create response was lost", async () => {
    const f = fixture();
    f.loseCreate();
    expect((await f.allocator.fetch(request(crypto.randomUUID()))).status).toBe(
      502,
    );
    expect(await f.restart().cleanup(later())).toEqual({
      reclaimed: 1,
      failed: 0,
    });
    expect(f.tunnels.size).toBe(0);
  });

  test("does not force-disconnect a connector that reconnects during cleanup", async () => {
    const f = fixture();
    await f.allocator.fetch(request(crypto.randomUUID()));
    f.reconnectOnDelete();
    expect(await f.allocator.cleanup(later())).toEqual({
      reclaimed: 0,
      failed: 1,
    });
    expect(f.tunnels.size).toBe(1);
    expect(f.records.size).toBe(1);
    expect(
      f.calls.some((c) => c.startsWith("PATCH") || c.endsWith("/connections")),
    ).toBe(false);
    f.setStatus("healthy");
    expect(await f.restart().cleanup(later())).toEqual({
      reclaimed: 0,
      failed: 0,
    });
  });

  test("scheduled cleanup uses the singleton binding and has no public route", async () => {
    let called = false;
    const env = {
      TUNNEL_ALLOCATOR: {
        idFromName(name: string) {
          expect(name).toBe("installations-v1");
          return "singleton";
        },
        get(id: string) {
          expect(id).toBe("singleton");
          return {
            async fetch(request: Request) {
              expect(new URL(request.url).pathname).toBe("/internal/cleanup");
              expect(request.method).toBe("POST");
              called = true;
              return Response.json({ reclaimed: 0, failed: 0 });
            },
          };
        },
      },
    } as unknown as TunnelEnv;
    await cleanupTunnelAllocations(env);
    expect(called).toBe(true);
    called = false;
    expect(
      await routeTunnelAllocation(
        new Request("https://public/internal/cleanup", { method: "POST" }),
        env,
      ),
    ).toBeNull();
    expect(called).toBe(false);
    const f = fixture();
    const response = await f.allocator.fetch(
      new Request("https://allocator/v1/tunnels/internal/cleanup", {
        method: "POST",
      }),
    );
    expect(response.status).toBe(400);
    expect(f.calls).toHaveLength(0);
  });
});

describe("enrollment body isolation", () => {
  function streamed(body: ReadableStream<Uint8Array>) {
    const base = request(crypto.randomUUID());
    return new Request(base.url, {
      method: "PUT",
      headers: base.headers,
      body,
    });
  }

  test("unfinished bodies do not block enrollment, deletion, or cleanup", async () => {
    const f = fixture();
    let controller!: ReadableStreamDefaultController<Uint8Array>;
    const pending = f.allocator.fetch(
      streamed(
        new ReadableStream({
          start(value) {
            controller = value;
            value.enqueue(new TextEncoder().encode("{"));
          },
        }),
      ),
    );
    let deadline: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([
        (async () => {
          const id = crypto.randomUUID();
          expect((await f.allocator.fetch(request(id))).status).toBe(200);
          expect(
            (await f.allocator.fetch(request(id, { method: "DELETE" }))).status,
          ).toBe(200);
          expect(await f.allocator.cleanup()).toEqual({
            reclaimed: 0,
            failed: 0,
          });
        })(),
        new Promise<never>((_, reject) => {
          deadline = setTimeout(
            () => reject(new Error("Allocator queue blocked")),
            1000,
          );
        }),
      ]);
    } finally {
      clearTimeout(deadline);
      controller.close();
      expect((await pending).status).toBe(400);
    }
  });

  test("bounds actual streamed bytes and cancels oversized bodies", async () => {
    const f = fixture();
    let cancelled = false;
    const req = streamed(
      new ReadableStream({
        start(controller) {
          controller.enqueue(new Uint8Array(600).fill(32));
          controller.enqueue(new Uint8Array(600).fill(32));
        },
        cancel() {
          cancelled = true;
        },
      }),
    );
    req.headers.set("content-length", "2");
    expect((await f.allocator.fetch(req)).status).toBe(413);
    expect(cancelled).toBe(true);
    expect(f.storage.values.size).toBe(0);
    expect(f.calls).toEqual([]);
    expect((await f.allocator.fetch(request(crypto.randomUUID()))).status).toBe(
      200,
    );
  });

  test("times out stalled bodies without waiting for cancellation to complete", async () => {
    const f = fixture();
    let cancelled = false;
    const response = await f.allocator.fetch(
      streamed(
        new ReadableStream({
          cancel() {
            cancelled = true;
            return new Promise<void>(() => {});
          },
        }),
      ),
    );
    expect(response.status).toBe(408);
    expect(cancelled).toBe(true);
    expect(f.storage.values.size).toBe(0);
    expect(f.calls).toEqual([]);
    expect((await f.allocator.fetch(request(crypto.randomUUID()))).status).toBe(
      200,
    );
  }, 10_000);

  test("rejects malformed JSON and failed uploads without reserving capacity", async () => {
    const f = fixture();
    expect(
      (
        await f.allocator.fetch(
          streamed(
            new ReadableStream({
              start(controller) {
                controller.enqueue(new TextEncoder().encode("{"));
                controller.close();
              },
            }),
          ),
        )
      ).status,
    ).toBe(400);
    expect(
      (
        await f.allocator.fetch(
          streamed(
            new ReadableStream({
              start(controller) {
                controller.error(new Error("upload disconnected"));
              },
            }),
          ),
        )
      ).status,
    ).toBe(400);
    expect(f.storage.values.size).toBe(0);
    expect(f.calls).toEqual([]);
  });
});

describe("verified origin TLS", () => {
  test("provisions HTTPS, persists trust settings, and reconfigures existing HTTP tunnels", async () => {
    const f = fixture();
    const id = crypto.randomUUID();
    const original = (await (await f.allocator.fetch(request(id))).json()) as {
      hostname: string;
    };
    const tls = {
      server_name: "localhost",
      ca_pool: "/etc/amarcode/origin-ca.pem",
    };
    const enroll = () =>
      new Request(request(id), {
        body: JSON.stringify({ port: 43721, origin_tls: tls }),
      });
    const response = await f.allocator.fetch(enroll());
    expect(response.status).toBe(200);
    const body = (await response.json()) as {
      hostname: string;
      origin_tls: unknown;
    };
    expect(body.hostname).toBe(original.hostname);
    expect(body.origin_tls).toEqual(tls);
    expect(f.services.at(-1)).toBe("https://127.0.0.1:43721");
    expect(f.originSettings.at(-1)).toEqual({
      originServerName: "localhost",
      caPool: tls.ca_pool,
      noTLSVerify: false,
    });
    const calls = f.services.length;
    expect((await f.restart().fetch(enroll())).status).toBe(200);
    expect(f.services.length).toBe(calls);
  });
  test("rejects malformed origin trust settings before provisioning", async () => {
    for (const tls of [
      null,
      true,
      {},
      { server_name: "bad\nname" },
      { server_name: "localhost", ca_pool: "relative.pem" },
    ]) {
      const f = fixture();
      const req = new Request(request(crypto.randomUUID()), {
        body: JSON.stringify({ port: 43721, origin_tls: tls }),
      });
      expect((await f.allocator.fetch(req)).status).toBe(400);
      expect(f.calls).toEqual([]);
    }
  });
});

function gate() {
  let release!: () => void;
  const wait = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { wait, release };
}
async function promptly<T>(promise: Promise<T>): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("Unrelated installation was blocked")),
          1000,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

describe("independent provisioning", () => {
  test("stalled provisioning does not block other installations, capacity checks, or deletion", async () => {
    const stalled = crypto.randomUUID();
    const entered = gate();
    const blocked = gate();
    const f = fixture("2", async (url) => {
      if (url.searchParams.get("name") === `amarcode-${stalled}`) {
        entered.release();
        await blocked.wait;
      }
    });
    const pending = f.allocator.fetch(request(stalled));
    await promptly(entered.wait);
    const other = crypto.randomUUID();
    const replacement = crypto.randomUUID();
    try {
      expect((await promptly(f.allocator.fetch(request(other)))).status).toBe(
        200,
      );
      expect(
        (await promptly(f.allocator.fetch(request(replacement)))).status,
      ).toBe(409);
      expect(
        (
          await promptly(
            f.allocator.fetch(request(other, { method: "DELETE" })),
          )
        ).status,
      ).toBe(200);
      expect(
        (await promptly(f.allocator.fetch(request(replacement)))).status,
      ).toBe(200);
    } finally {
      blocked.release();
    }
    expect((await pending).status).toBe(200);
    const db =
      await f.storage.get<Record<string, { status: string }>>("installations");
    expect(db![stalled].status).toBe("ready");
    expect(db![other].status).toBe("deleted");
    expect(db![replacement].status).toBe("ready");
    expect(f.tunnels.size).toBe(2);
    // All independent commits survive an allocator restart.
    expect((await f.restart().fetch(request(stalled))).status).toBe(200);
    expect(f.tunnels.size).toBe(2);
  });

  test("concurrent operations on one installation run in order, including after failure", async () => {
    const id = crypto.randomUUID();
    const entered = gate();
    const blocked = gate();
    const f = fixture("2", async (url) => {
      if (url.searchParams.get("name") === `amarcode-${id}`) {
        entered.release();
        await blocked.wait;
      }
    });
    f.failConfiguration();
    const first = f.allocator.fetch(request(id));
    await promptly(entered.wait);
    const second = f.allocator.fetch(request(id, { port: 43722 }));
    const third = f.allocator.fetch(request(id, { port: 43723 }));
    blocked.release();
    expect((await first).status).toBe(502);
    expect((await second).status).toBe(200);
    expect((await third).status).toBe(200);
    expect(f.services).toEqual([
      "http://127.0.0.1:43722",
      "http://127.0.0.1:43723",
    ]);
    expect(
      (await f.allocator.fetch(request(id, { method: "DELETE" }))).status,
    ).toBe(200);
    expect(
      f.calls.filter(
        (call) => call === "POST /client/v4/accounts/account/cfd_tunnel",
      ),
    ).toHaveLength(1);
    expect(f.tunnels.size).toBe(0);
    expect((await f.allocator.fetch(request(id))).status).toBe(410);
  });

  test("a stalled cleanup does not block unrelated enrollment or overwrite its state", async () => {
    const entered = gate();
    const blocked = gate();
    let stallStatus = false;
    const f = fixture("2", async (url, method) => {
      if (
        stallStatus &&
        method === "GET" &&
        /\/cfd_tunnel\/[^/]+$/.test(url.pathname)
      ) {
        entered.release();
        await blocked.wait;
      }
    });
    const old = crypto.randomUUID();
    expect((await f.allocator.fetch(request(old))).status).toBe(200);
    stallStatus = true;
    const cleanup = f.allocator.cleanup(
      Date.now() + TUNNEL_IDLE_GRACE_MS + 1000,
    );
    await promptly(entered.wait);
    const fresh = crypto.randomUUID();
    try {
      expect((await promptly(f.allocator.fetch(request(fresh)))).status).toBe(
        200,
      );
    } finally {
      blocked.release();
    }
    expect(await cleanup).toEqual({ reclaimed: 1, failed: 0 });
    const db =
      await f.storage.get<Record<string, { status: string }>>("installations");
    expect(db![old].status).toBe("reclaimed");
    expect(db![fresh].status).toBe("ready");
    expect(f.tunnels.size).toBe(1);
  });

  test("limits upstream concurrency while allowing independent requests to progress", async () => {
    const blocked = gate();
    const fourStarted = gate();
    let active = 0;
    let maximum = 0;
    let starts = 0;
    const f = fixture("8", async () => {
      active++;
      maximum = Math.max(maximum, active);
      if (++starts === 4) fourStarted.release();
      await blocked.wait;
      active--;
    });
    const requests = Array.from({ length: 8 }, () =>
      f.allocator.fetch(request(crypto.randomUUID())),
    );
    try {
      await promptly(fourStarted.wait);
      expect(starts).toBe(4);
      expect(maximum).toBe(4);
    } finally {
      blocked.release();
    }
    expect((await Promise.all(requests)).map((r) => r.status)).toEqual(
      Array(8).fill(200),
    );
    expect(maximum).toBeLessThanOrEqual(4);
    expect(f.tunnels.size).toBe(8);
  });
});
