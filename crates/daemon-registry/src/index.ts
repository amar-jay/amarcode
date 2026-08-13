export interface Env {
  DAEMON_ARTIFACTS: R2Bucket;
}

type ArtifactRoute = {
  key: string;
  cacheControl: string;
  downloadName?: string;
};

const JSON_CACHE_CONTROL = "public, max-age=60, must-revalidate";
const BINARY_CACHE_CONTROL = "public, max-age=31536000, immutable";
const SEGMENT_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$/;

function json(
  value: unknown,
  status = 200,
  extraHeaders?: HeadersInit,
): Response {
  const headers = new Headers(extraHeaders);
  headers.set("content-type", "application/json; charset=utf-8");
  headers.set("x-content-type-options", "nosniff");
  return Response.json(value, { status, headers });
}

function safeSegment(value: string): string | null {
  try {
    const decoded = decodeURIComponent(value);
    return SEGMENT_PATTERN.test(decoded) ? decoded : null;
  } catch {
    return null;
  }
}

export function resolveArtifactRoute(pathname: string): ArtifactRoute | null {
  if (pathname === "/v1/daemon/latest.json") {
    return {
      key: "daemon/latest.json",
      cacheControl: JSON_CACHE_CONTROL,
    };
  }

  if (pathname === "/v1/daemon/latest.json.sig") {
    return {
      key: "daemon/latest.json.sig",
      cacheControl: JSON_CACHE_CONTROL,
    };
  }

  const manifestMatch = pathname.match(
    /^\/v1\/daemon\/([^/]+)\/(manifest\.json(?:\.sig)?)$/,
  );
  if (manifestMatch) {
    const version = safeSegment(manifestMatch[1]);
    if (!version) return null;
    return {
      key: `daemon/${version}/${manifestMatch[2]}`,
      cacheControl: JSON_CACHE_CONTROL,
    };
  }

  const artifactMatch = pathname.match(/^\/v1\/daemon\/([^/]+)\/([^/]+)$/);
  if (!artifactMatch) return null;

  const version = safeSegment(artifactMatch[1]);
  const target = safeSegment(artifactMatch[2]);
  if (!version || !target) return null;

  const filename = target.includes("windows")
    ? "amarcode-daemon.exe"
    : "amarcode-daemon";
  return {
    key: `daemon/${version}/${target}/${filename}`,
    cacheControl: BINARY_CACHE_CONTROL,
    downloadName: filename,
  };
}

function rangeHeaders(
  object: R2Object,
  request: Request,
  headers: Headers,
): number {
  if (!request.headers.has("range") || !object.range) return 200;

  const range = object.range as {
    offset?: number;
    length?: number;
    suffix?: number;
  };
  let start: number;
  let length: number;
  if (typeof range.suffix === "number") {
    length = Math.min(range.suffix, object.size);
    start = object.size - length;
  } else {
    start = range.offset ?? 0;
    length = range.length ?? object.size - start;
  }

  headers.set(
    "content-range",
    `bytes ${start}-${start + length - 1}/${object.size}`,
  );
  headers.set("content-length", String(length));
  return 206;
}

async function serveArtifact(
  request: Request,
  env: Env,
  route: ArtifactRoute,
): Promise<Response> {
  if (request.method === "HEAD") {
    const object = await env.DAEMON_ARTIFACTS.head(route.key);
    if (!object) return json({ error: "artifact not found" }, 404);

    const headers = new Headers();
    object.writeHttpMetadata(headers);
    headers.set("etag", object.httpEtag);
    headers.set("content-length", String(object.size));
    headers.set("cache-control", route.cacheControl);
    headers.set("accept-ranges", "bytes");
    headers.set("x-content-type-options", "nosniff");
    if (route.downloadName) {
      headers.set(
        "content-disposition",
        `attachment; filename=\"${route.downloadName}\"`,
      );
    }
    return new Response(null, { status: 200, headers });
  }

  const object = await env.DAEMON_ARTIFACTS.get(route.key, {
    onlyIf: request.headers,
    range: request.headers,
  });
  if (!object) return json({ error: "artifact not found" }, 404);

  const headers = new Headers();
  object.writeHttpMetadata(headers);
  headers.set("etag", object.httpEtag);
  headers.set("cache-control", route.cacheControl);
  headers.set("accept-ranges", "bytes");
  headers.set("x-content-type-options", "nosniff");
  if (route.downloadName) {
    headers.set(
      "content-disposition",
      `attachment; filename=\"${route.downloadName}\"`,
    );
  }

  if (!("body" in object)) {
    const notModified =
      request.headers.has("if-none-match") ||
      request.headers.has("if-modified-since");
    return new Response(null, { status: notModified ? 304 : 412, headers });
  }

  const status = rangeHeaders(object, request, headers);
  return new Response(object.body, { status, headers });
}

export async function handleRequest(
  request: Request,
  env: Env,
): Promise<Response> {
  const url = new URL(request.url);

  if (url.pathname === "/health") {
    if (request.method !== "GET" && request.method !== "HEAD") {
      return json({ error: "method not allowed" }, 405, { allow: "GET, HEAD" });
    }
    return request.method === "HEAD"
      ? new Response(null, { status: 200 })
      : json({ status: "ok", service: "amarcode-daemon-distribution" });
  }

  if (request.method !== "GET" && request.method !== "HEAD") {
    return json({ error: "method not allowed" }, 405, { allow: "GET, HEAD" });
  }

  const route = resolveArtifactRoute(url.pathname);
  if (!route) return json({ error: "not found" }, 404);
  return serveArtifact(request, env, route);
}

export default {
  fetch: handleRequest,
} satisfies ExportedHandler<Env>;
