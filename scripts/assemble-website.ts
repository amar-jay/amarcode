import { cp, mkdir, rm } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const docs = fileURLToPath(new URL("../crates/docs/dist/", import.meta.url));
const destination = fileURLToPath(
  new URL("../crates/landing/dist/docs/", import.meta.url),
);

// Astro's base affects URLs, not the output directory. Preserve /docs asset,
// search, sitemap, and page paths by mounting its complete output here.
await rm(destination, { recursive: true, force: true });
await mkdir(destination, { recursive: true });
await cp(docs, destination, { recursive: true });
console.log("Assembled landing and documentation in crates/landing/dist");
