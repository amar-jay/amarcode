import { describe, expect, test } from "bun:test";
import {
  Allocator,
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
    TUNNEL_ENROLLMENT_CODE: "invite-secret",
    TUNNEL_CAPACITY: capacity,
  };
  const tunnels = new Map<string, { id: string; name: string }>();
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
      records.delete(path.split("/").at(-1)!);
      result = {};
    } else if (method === "DELETE" && path.includes("/cfd_tunnel/")) {
      tunnels.delete(path.split("/").at(-1)!);
      result = {};
    } else throw new Error(`Unexpected API request ${method} ${path}`);
    return Response.json({ success: true, result });
  } as typeof fetch;
  const restart = () => new Allocator(storage, env, fetcher);
  return {
    allocator: restart(),
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
    enrollment?: string;
    port?: number;
  } = {},
) =>
  new Request(`https://allocator.example/v1/tunnels/${id}`, {
    method: options.method ?? "PUT",
    headers: {
      authorization: `Bearer ${options.secret ?? "a".repeat(64)}`,
      "x-amarcode-enrollment": options.enrollment ?? "invite-secret",
      "content-type": "application/json",
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
  test("enrolls once and reconnects after allocator restart without enrollment code", async () => {
    const f = fixture();
    const id = crypto.randomUUID();
    const first = await f.allocator.fetch(request(id));
    expect(first.status).toBe(200);
    const assignment = (await first.json()) as {
      url: string;
      tunnel_token: string;
      dns_ready_after: number;
    };
    expect(assignment.url).toBe(`wss://m-${id}.example.com/rpc`);
    expect(assignment.tunnel_token).toBe("connector-token");
    expect(assignment.dns_ready_after).toBeGreaterThan(Date.now());
    expect(assignment.dns_ready_after).toBeLessThanOrEqual(Date.now() + 15_000);
    expect(first.headers.get("cache-control")).toBe("no-store");
    const reconnect = await f.restart().fetch(request(id, { enrollment: "" }));
    expect((await reconnect.json()) as typeof assignment).toEqual(assignment);
    expect(f.tunnels.size).toBe(1);
    expect(f.records.size).toBe(1);
  });
  test("listener port changes keep the installation hostname and update ingress", async () => {
    const f = fixture();
    const id = crypto.randomUUID();
    const first = (await (await f.allocator.fetch(request(id))).json()) as {
      url: string;
    };
    const next = await f
      .restart()
      .fetch(request(id, { port: 43888, enrollment: "" }));
    expect(next.status).toBe(200);
    const value = (await next.json()) as { url: string; port: number };
    expect(value.url).toBe(first.url);
    expect(value.port).toBe(43888);
    expect(f.services.at(-1)).toBe("http://127.0.0.1:43888");
    expect(f.tunnels.size).toBe(1);
    expect(f.records.size).toBe(1);
  });
  test("authenticates enrollment, reconnect and deletion before touching Cloudflare", async () => {
    const f = fixture();
    const id = crypto.randomUUID();
    expect(
      (await f.allocator.fetch(request(id, { enrollment: "wrong" }))).status,
    ).toBe(403);
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
    expect(
      (await f.restart().fetch(request(id, { enrollment: "" }))).status,
    ).toBe(200);
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
