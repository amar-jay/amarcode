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
    DAEMON_ARTIFACTS: {
      async get(key: string) {
        const value = entries[key];
        return value === undefined ? null : fakeObject(key, value);
      },
      async head(key: string) {
        const value = entries[key];
        return value === undefined ? null : fakeObject(key, value);
      },
    } as R2Bucket,
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
