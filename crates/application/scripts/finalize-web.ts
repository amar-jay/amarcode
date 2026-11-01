import { copyFile, rename } from "node:fs/promises";
// Vite builds the independent web entry without changing desktop index.html.
await rename(
  new URL("../dist-web/web.html", import.meta.url),
  new URL("../dist-web/index.html", import.meta.url),
);
await copyFile(
  new URL("../web/_headers", import.meta.url),
  new URL("../dist-web/_headers", import.meta.url),
);
