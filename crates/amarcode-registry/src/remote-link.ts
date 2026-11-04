import { brotliDecompressSync } from "node:zlib";

export const REMOTE_LINK_PREFIX = "/r/";
const MAX_PAYLOAD = 512;
const MAX_TARGET = 256;
const PAYLOAD_PATTERN = /^[A-Za-z0-9_-]+$/;

function base64UrlDecode(payload: string): Uint8Array | null {
  try {
    const padded = payload + "=".repeat((4 - (payload.length % 4)) % 4);
    const binary = atob(padded.replaceAll("-", "+").replaceAll("_", "/"));
    return Uint8Array.from(binary, (char) => char.charCodeAt(0));
  } catch {
    return null;
  }
}

/** Decode a short-link payload to a validated Cloudflare quick-tunnel WSS URL. */
export function decodeRemoteLink(payload: string): string | null {
  if (
    !payload ||
    payload.length > MAX_PAYLOAD ||
    !PAYLOAD_PATTERN.test(payload)
  ) {
    return null;
  }
  const compressed = base64UrlDecode(payload);
  if (!compressed) return null;
  let text: string;
  try {
    text = new TextDecoder("utf-8", { fatal: true }).decode(
      brotliDecompressSync(compressed, { maxOutputLength: MAX_TARGET }),
    );
  } catch {
    return null;
  }
  try {
    const url = new URL(text);
    if (
      url.toString() !== text ||
      url.protocol !== "wss:" ||
      !/^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.trycloudflare\.com$/.test(
        url.hostname,
      ) ||
      url.port ||
      url.username ||
      url.password ||
      url.pathname !== "/rpc" ||
      url.search ||
      url.hash
    ) {
      return null;
    }
    return url.toString();
  } catch {
    return null;
  }
}

export function remoteLinkPayload(pathname: string): string | null {
  if (!pathname.startsWith(REMOTE_LINK_PREFIX)) return null;
  const payload = pathname.slice(REMOTE_LINK_PREFIX.length);
  return payload.includes("/") ? null : payload;
}

export function wantsJsonResolve(request: Request): boolean {
  const accept = request.headers.get("accept") ?? "";
  return accept.includes("application/json");
}

export function resolveRemoteLinkResponse(
  request: Request,
  pathname: string,
): Response | null {
  const payload = remoteLinkPayload(pathname);
  if (payload === null) return null;
  const headers = {
    "access-control-allow-origin": "*",
    "cache-control": "no-store",
    "x-content-type-options": "nosniff",
    "referrer-policy": "no-referrer",
    vary: "Accept",
  };
  if (request.method !== "GET" && request.method !== "HEAD") {
    return Response.json(
      { error: "method not allowed" },
      { status: 405, headers: { ...headers, allow: "GET, HEAD" } },
    );
  }
  const target = decodeRemoteLink(payload);
  if (!target) {
    return new Response(
      request.method === "HEAD" ? null : JSON.stringify({ error: "not found" }),
      {
        status: 404,
        headers: { ...headers, "content-type": "application/json" },
      },
    );
  }
  const httpsTarget = new URL(target);
  httpsTarget.protocol = "https:";
  if (wantsJsonResolve(request)) {
    return new Response(
      request.method === "HEAD" ? null : JSON.stringify({ url: target }),
      {
        status: 200,
        headers: {
          ...headers,
          "content-type": "application/json; charset=utf-8",
        },
      },
    );
  }
  return new Response(null, {
    status: 302,
    headers: {
      ...headers,
      location: httpsTarget.toString(),
    },
  });
}
