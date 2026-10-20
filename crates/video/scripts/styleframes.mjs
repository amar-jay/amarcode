import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const out = path.join(root, "styleframes");
await mkdir(out, { recursive: true });
const browser = await chromium.launch({
  headless: true,
  ...(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {}),
  args: ["--no-sandbox", "--allow-file-access-from-files"],
});
const page = await browser.newPage({
  viewport: { width: 1920, height: 1080 },
  deviceScaleFactor: 1,
});
const html = pathToFileURL(path.join(out, "index.html")).href;
for (const id of ["open", "push", "end"]) {
  await page.goto(`${html}?frame=${id}`);
  await page.screenshot({
    path: path.join(out, `${id}.png`),
    scale: "css",
    clip: { x: 0, y: 0, width: 1920, height: 1080 },
  });
}
await page.setViewportSize({ width: 1480, height: 900 });
await page.goto(html);
await page.screenshot({
  path: path.join(out, "comparison.png"),
  fullPage: true,
  scale: "css",
});
await browser.close();
console.log(out);
