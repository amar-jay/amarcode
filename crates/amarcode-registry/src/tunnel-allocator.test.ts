import { describe, expect, test, spyOn } from "bun:test";
import {
  Allocator,
  TUNNEL_IDLE_GRACE_MS,
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
function fixture(capacity = "5") {
  const storage = new Memory();
  const env: TunnelEnv = {
    TUNNEL_CF_API_TOKEN: "account-secret",
    TUNNEL_CF_ACCOUNT_ID: "account",
    TUNNEL_CF_ZONE_ID: "zone",
    TUNNEL_DOMAIN: "example.com",
    TUNNEL_CAPACITY: capacity,
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
  let loseCreateResponse = false;
  let failConfiguration = false;
  let loseDnsResponse = false;
  let failDisconnect = false;
  let redirectApi = false;
  let tunnelStatus = "inactive";
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
    expect(new Headers(init?.headers).get("authorization")).toBe(
      "Bearer account-secret",
    );
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
      expect(ingress[0].service).toMatch(/^http:\/\/127\.0\.0\.1:\d+$/);
      services.push(ingress[0].service);
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
    redirectApi: () => {
      redirectApi = true;
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
  } = {},
) =>
  new Request(`https://allocator.example/v1/tunnels/${id}`, {
    method: options.method ?? "PUT",
    headers: {
      authorization: `Bearer ${options.secret ?? "a".repeat(64)}`,
      "content-type": "application/json",
      "x-amarcode-tunnel-hostnames": "short-v2",
    },
    body:
      options.method === "DELETE"
        ? undefined
        : JSON.stringify({ port: options.port ?? 43721 }),
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
    };
    expect(assignment.url).toMatch(/^wss:\/\/[a-f0-9]{8}\.example\.com\/rpc$/);
    expect(assignment.tunnel_token).toBe("connector-token");
    expect(assignment.dns_ready_after).toBeGreaterThan(Date.now());
    expect(assignment.dns_ready_after).toBeLessThanOrEqual(Date.now() + 15_000);
    expect(first.headers.get("cache-control")).toBe("no-store");
    const reconnect = await f.restart().fetch(request(id, {}));
    expect((await reconnect.json()) as typeof assignment).toEqual(assignment);
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

  test("older daemons get compatible first-level names and newer ones can migrate to short names", async () => {
    const f = fixture();
    const id = crypto.randomUUID();
    const old = request(id);
    old.headers.delete("x-amarcode-tunnel-hostnames");
    const response = await f.allocator.fetch(old);
    expect(response.status).toBe(200);
    expect(((await response.json()) as { hostname: string }).hostname).toBe(
      `m-${id}.example.com`,
    );
    const upgraded = await f.restart().fetch(request(id));
    expect(upgraded.status).toBe(200);
    expect(((await upgraded.json()) as { hostname: string }).hostname).toMatch(
      /^[a-f0-9]{8}\.example\.com$/,
    );
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
    expect(((await fresh.json()) as { hostname: string }).hostname).not.toBe(
      `m-${id}.example.com`,
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
