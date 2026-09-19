import { existsSync, readFileSync } from "node:fs";
import { run, wranglerConfig } from "./commands";
import type { Manifest } from "./manifest";

export function loadRemoteManifest(
  bucket: string,
  key: string,
  destination: string,
): Manifest | null {
  const result = run(
    [
      "bunx",
      "wrangler",
      "r2",
      "object",
      "get",
      `${bucket}/${key}`,
      "--file",
      destination,
      "--remote",
      "--config",
      wranglerConfig,
    ],
    { quiet: true, allowFailure: true },
  );
  if (!result.success) {
    const detail = result.stderr.toString();
    if (detail.includes("The specified key does not exist")) return null;
    throw new Error(
      `could not read existing manifest from R2:\n${detail.trim()}`,
    );
  }
  if (!existsSync(destination)) {
    throw new Error(
      "Wrangler reported a successful manifest download but created no file",
    );
  }
  return JSON.parse(readFileSync(destination, "utf8")) as Manifest;
}

export function upload(
  bucket: string,
  key: string,
  path: string,
  contentType: string,
  cacheControl: string,
) {
  run([
    "bunx",
    "wrangler",
    "r2",
    "object",
    "put",
    `${bucket}/${key}`,
    "--file",
    path,
    "--content-type",
    contentType,
    "--cache-control",
    cacheControl,
    "--remote",
    "--config",
    wranglerConfig,
  ]);
}
