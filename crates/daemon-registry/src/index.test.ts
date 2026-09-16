import { describe, expect, test } from "bun:test";

import { handleRequest, resolveArtifactRoute, type Env } from "./index";

const encoder = new TextEncoder();

function fakeObject(key: string, value: string): R2ObjectBody {
  const bytes = encoder.encode(value);
  return {
    key,
    version: "test-version",
    size: bytes.byteLength,
    etag: "test-etag",
    httpEtag: '"test-etag"',
    uploaded: new Date("2026-01-01T00:00:00Z"),
    httpMetadata: { contentType: "application/octet-stream" },
    customMetadata: {},
    // The live R2 object exposes all range properties with unused values set to
    // undefined, rather than omitting them from the object.
    range: {
      offset: 0,
      length: bytes.byteLength,
      suffix: undefined,
    } as R2Range,
    checksums: {} as R2Checksums,
    storageClass: "Standard",
    body: new Blob([bytes]).stream(),
    bodyUsed: false,
    writeHttpMetadata(headers) {
      headers.set("content-type", "application/octet-stream");
    },
    async arrayBuffer() {
      return bytes.slice().buffer as ArrayBuffer;
    },
    async text() {
      return value;
    },
    async json<T>() {
      return JSON.parse(value) as T;
    },
    async blob() {
      return new Blob([bytes]);
    },
    async bytes() {
      return bytes;
    },
  };
}

function environment(entries: Record<string, string>): Env {
  return {
    GITHUB_TOKEN: "test-github-token",
    DAEMON_ARTIFACTS: {
      async get(key: string) {
        const value = entries[key];
        return value === undefined ? null : fakeObject(key, value);
      },
      async head(key: string) {
        const value = entries[key];
        return value === undefined ? null : fakeObject(key, value);
      },
      async list() {
        return {
          objects: [],
          truncated: false,
          delimitedPrefixes: Object.keys(entries)
            .filter((key) => key.startsWith("daemon/"))
            .map((key) => key.slice(0, key.indexOf("/", "daemon/".length) + 1))
            .filter((prefix) => prefix !== "daemon/")
            .filter(
              (prefix, index, prefixes) => prefixes.indexOf(prefix) === index,
            ),
        };
      },
    } as unknown as R2Bucket,
  };
}

describe("resolveArtifactRoute", () => {
  test("maps public routes to private R2 keys", () => {
    expect(resolveArtifactRoute("/v1/daemon/latest.json")?.key).toBe(
      "daemon/latest.json",
    );
    expect(resolveArtifactRoute("/v1/daemon/0.1.0/manifest.json")?.key).toBe(
      "daemon/0.1.0/manifest.json",
    );
    expect(resolveArtifactRoute("/v1/daemon/latest.json.sig")?.key).toBe(
      "daemon/latest.json.sig",
    );
    expect(
      resolveArtifactRoute("/v1/daemon/0.1.0/manifest.json.sig")?.key,
    ).toBe("daemon/0.1.0/manifest.json.sig");
    expect(
      resolveArtifactRoute("/v1/daemon/0.1.0/x86_64-pc-windows-msvc")?.key,
    ).toBe("daemon/0.1.0/x86_64-pc-windows-msvc/amarcode-daemon.exe");
  });

  test("rejects traversal and extra path segments", () => {
    expect(
      resolveArtifactRoute("/v1/daemon/../x86_64-unknown-linux-gnu"),
    ).toBeNull();
    expect(resolveArtifactRoute("/v1/daemon/0.1.0/linux/extra")).toBeNull();
    expect(
      resolveArtifactRoute("/v1/daemon/%2e%2e/x86_64-unknown-linux-gnu"),
    ).toBeNull();
  });
});

describe("handleRequest", () => {
  test("proxies the desktop updater manifest from the rolling GitHub release", async () => {
    let upstreamRequest: Request | undefined;
    const response = await handleRequest(
      new Request("https://downloads.example/v1/app/latest.json"),
      environment({}),
      async (request) => {
        upstreamRequest = request;
        return new Response('{"version":"0.1.1"}', {
          headers: {
            "content-type": "application/octet-stream",
            etag: '"app-manifest"',
            "set-cookie": "not-forwarded=true",
          },
        });
      },
    );

    expect(upstreamRequest?.url).toBe(
      "https://github.com/amar-jay/amarcode/releases/latest/download/latest.json",
    );
    expect(upstreamRequest?.headers.get("accept")).toBe("application/json");
    expect(upstreamRequest?.headers.get("authorization")).toBe(
      "Bearer test-github-token",
    );
    expect(upstreamRequest?.headers.get("user-agent")).toBe(
      "amarcode-update-proxy",
    );
    expect(response.status).toBe(200);
    expect(response.headers.get("content-type")).toContain("application/json");
    expect(response.headers.get("cache-control")).toContain("max-age=60");
    expect(response.headers.get("etag")).toBe('"app-manifest"');
    expect(response.headers.has("set-cookie")).toBe(false);
    expect(response.headers.has("authorization")).toBe(false);
    expect((await response.json()) as unknown).toEqual({ version: "0.1.1" });
  });

  test("supports HEAD for the desktop updater manifest", async () => {
    let upstreamMethod: string | undefined;
    const response = await handleRequest(
      new Request("https://downloads.example/v1/app/latest.json", {
        method: "HEAD",
      }),
      environment({}),
      async (request) => {
        upstreamMethod = request.method;
        return new Response(null, { headers: { etag: '"app-manifest"' } });
      },
    );

    expect(upstreamMethod).toBe("HEAD");
    expect(response.status).toBe(200);
    expect(response.headers.get("etag")).toBe('"app-manifest"');
    expect(await response.text()).toBe("");
  });

  test("returns a non-cacheable 502 when the app manifest origin fails", async () => {
    const response = await handleRequest(
      new Request("https://downloads.example/v1/app/latest.json"),
      environment({}),
      async () => {
        throw new Error("origin unavailable");
      },
    );

    expect(response.status).toBe(502);
    expect(response.headers.get("cache-control")).toBe("no-store");
    expect((await response.json()) as unknown).toEqual({
      error: "app update manifest unavailable",
    });
  });

  test("lists published daemon versions", async () => {
    const response = await handleRequest(
      new Request("https://downloads.example/v1/daemon/versions.json"),
      environment({
        "daemon/0.2.0/manifest.json": "manifest",
        "daemon/0.1.0/manifest.json": "manifest",
        "daemon/latest.json": "manifest",
      }),
    );

    expect(response.status).toBe(200);
    expect(response.headers.get("cache-control")).toContain("max-age=60");
    expect((await response.json()) as { versions: string[] }).toEqual({
      versions: ["0.1.0", "0.2.0"],
    });
  });

  test("supports HEAD for the versions route", async () => {
    const response = await handleRequest(
      new Request("https://downloads.example/v1/daemon/versions.json", {
        method: "HEAD",
      }),
      environment({}),
    );

    expect(response.status).toBe(200);
    expect(await response.text()).toBe("");
  });

  test("serves a binary with immutable download headers", async () => {
    const env = environment({
      "daemon/0.1.0/x86_64-unknown-linux-gnu/amarcode-daemon": "binary",
    });
    const response = await handleRequest(
      new Request(
        "https://downloads.example/v1/daemon/0.1.0/x86_64-unknown-linux-gnu",
      ),
      env,
    );

    expect(response.status).toBe(200);
    expect(await response.text()).toBe("binary");
    expect(response.headers.get("etag")).toBe('"test-etag"');
    expect(response.headers.get("cache-control")).toContain("immutable");
    expect(response.headers.get("content-disposition")).toContain(
      "amarcode-daemon",
    );
  });

  test("keeps the Worker read-only", async () => {
    const response = await handleRequest(
      new Request("https://downloads.example/v1/daemon/latest.json", {
        method: "PUT",
      }),
      environment({}),
    );
    expect(response.status).toBe(405);
    expect(response.headers.get("allow")).toBe("GET, HEAD");
  });

  test("returns valid partial-content headers", async () => {
    const env = environment({
      "daemon/0.1.0/x86_64-unknown-linux-gnu/amarcode-daemon": "binary",
    });
    const response = await handleRequest(
      new Request(
        "https://downloads.example/v1/daemon/0.1.0/x86_64-unknown-linux-gnu",
        {
          headers: { range: "bytes=0-5" },
        },
      ),
      env,
    );

    expect(response.status).toBe(206);
    expect(response.headers.get("content-range")).toBe("bytes 0-5/6");
    expect(response.headers.get("content-length")).toBe("6");
  });

  test("returns JSON 404 responses", async () => {
    const response = await handleRequest(
      new Request(
        "https://downloads.example/v1/daemon/0.1.0/aarch64-apple-darwin",
      ),
      environment({}),
    );
    expect(response.status).toBe(404);
    expect(response.headers.get("content-type")).toContain("application/json");
  });
});
