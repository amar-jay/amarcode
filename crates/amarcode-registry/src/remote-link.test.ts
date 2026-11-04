import { describe, expect, test } from "bun:test";
import { brotliCompressSync } from "node:zlib";

import { handleRequest, type Env } from "./index";
import { decodeRemoteLink, REMOTE_LINK_PREFIX } from "./remote-link";

function encode(target: string): string {
  const bytes = brotliCompressSync(new TextEncoder().encode(target));
  return btoa(String.fromCharCode(...bytes))
    .replaceAll("+", "-")
    .replaceAll("/", "_")
    .replaceAll("=", "");
}

function environment(): Env {
  return {
    GITHUB_TOKEN: "test-token",
    DAEMON_ARTIFACTS: {
      get: async () => null,
      head: async () => null,
      list: async () => ({ objects: [], truncated: false }),
    } as unknown as R2Bucket,
  };
}

describe("remote short links", () => {
  test("decodes only cloudflare quick-tunnel websocket targets", () => {
    const target = "wss://my-remote.trycloudflare.com/rpc";
    expect(decodeRemoteLink(encode(target))).toBe(target);
    expect(decodeRemoteLink(encode("wss://evil.example/rpc"))).toBeNull();
    expect(decodeRemoteLink("not-valid")).toBeNull();
    expect(decodeRemoteLink("")).toBeNull();
    for (const invalid of [
      "wss://evil.trycloudflare.com.example/rpc",
      "wss://nested.host.trycloudflare.com/rpc",
      "wss://trycloudflare.com/rpc",
      "wss://user:secret@host.trycloudflare.com/rpc",
      "wss://host.trycloudflare.com:8443/rpc",
      "wss://host.trycloudflare.com/rpc?token=private",
      "wss://host.trycloudflare.com/rpc#fragment",
      "https://host.trycloudflare.com/rpc",
      "wss://host.trycloudflare.com/terminal",
      "a".repeat(100_000),
    ])
      expect(decodeRemoteLink(encode(invalid))).toBeNull();
  });

  test("redirects browsers and returns json for clients", async () => {
    const target = "wss://my-remote.trycloudflare.com/rpc";
    const url = `https://updates.amarcode.amarjay.com${REMOTE_LINK_PREFIX}${encode(target)}`;

    const redirect = await handleRequest(new Request(url), environment());
    expect(redirect.status).toBe(302);
    expect(redirect.headers.get("location")).toBe(
      "https://my-remote.trycloudflare.com/rpc",
    );
    expect(redirect.headers.get("access-control-allow-origin")).toBe("*");

    const resolved = await handleRequest(
      new Request(url, { headers: { accept: "application/json" } }),
      environment(),
    );
    expect(resolved.status).toBe(200);
    expect((await resolved.json()) as { url: string }).toEqual({ url: target });
    expect(resolved.headers.get("vary")).toBe("Accept");
    const head = await handleRequest(
      new Request(url, {
        method: "HEAD",
        headers: { accept: "application/json" },
      }),
      environment(),
    );
    expect(head.status).toBe(200);
    expect(await head.text()).toBe("");
    const post = await handleRequest(
      new Request(url, { method: "POST" }),
      environment(),
    );
    expect(post.status).toBe(405);
  });

  test("rejects open redirects and bad payloads", async () => {
    const evil = encode("wss://evil.example/rpc");
    const response = await handleRequest(
      new Request(
        `https://updates.amarcode.amarjay.com${REMOTE_LINK_PREFIX}${evil}`,
      ),
      environment(),
    );
    expect(response.status).toBe(404);

    const missing = await handleRequest(
      new Request(
        `https://updates.amarcode.amarjay.com${REMOTE_LINK_PREFIX}!!!!`,
      ),
      environment(),
    );
    expect(missing.status).toBe(404);
    expect(missing.headers.get("access-control-allow-origin")).toBe("*");
  });
});
