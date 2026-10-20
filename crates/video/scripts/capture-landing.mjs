import { chromium } from "playwright";
import { spawn } from "node:child_process";
import { mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
const root = fileURLToPath(new URL("../", import.meta.url));
const server = spawn(
  "bun",
  ["run", "dev", "--", "--host", "127.0.0.1", "--port", "1421"],
  { cwd: `${root}../landing`, stdio: "ignore" },
);
let browser;
try {
  for (let i = 0; i < 80; i++) {
    try {
      if ((await fetch("http://127.0.0.1:1421")).ok) break;
    } catch {}
    await new Promise((r) => setTimeout(r, 250));
  }
  browser = await chromium.launch({
    headless: true,
    ...(process.env.CHROME_PATH
      ? { executablePath: process.env.CHROME_PATH }
      : {}),
    args: ["--no-sandbox"],
  });
  const page = await browser.newPage({
    viewport: { width: 1440, height: 1000 },
  });
  await page.goto("http://127.0.0.1:1421");
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(2000);
  await mkdir(`${root}out`, { recursive: true });
  await page.screenshot({ path: `${root}out/landing-reference.png` });
  await page.locator("h1").evaluate((element) => {
    element.parentElement.style.visibility = "hidden";
  });
  await page
    .locator("canvas")
    .screenshot({ path: `${root}public/landing/sky.png` });
} finally {
  await browser?.close();
  server.kill();
}
