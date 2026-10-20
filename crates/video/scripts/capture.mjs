import { chromium } from "playwright";
import { spawn, execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { installDemoBridge } from "./demo-bridge.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const target = `${root}public/walkthrough`;
await mkdir(`${root}out/raw`, { recursive: true });
await mkdir(target, { recursive: true });
const server = spawn("bun", ["run", "dev"], {
  cwd: `${root}../application`,
  stdio: "ignore",
});
let browser;
try {
  for (let i = 0; i < 80; i++) {
    try {
      if ((await fetch("http://127.0.0.1:1420")).ok) break;
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
  const context = await browser.newContext({
    viewport: { width: 1920, height: 1080 },
    deviceScaleFactor: 1,
    recordVideo: {
      dir: `${root}out/raw`,
      size: { width: 1920, height: 1080 },
      fps: 60,
    },
  });
  await context.addInitScript(installDemoBridge);
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => {
    errors.push(error.message);
    console.error(error.message);
  });
  await page.goto("http://127.0.0.1:1420");
  await page.getByRole("button", { name: "Codex", exact: true }).waitFor();
  const zoom = 1;
  await page.evaluate((zoom) => {
    document.documentElement.style.zoom = String(zoom);
  }, zoom);
  await page.evaluate(() => document.fonts.ready);
  await page.evaluate(() => {
    const cursor = document.createElement("div");
    cursor.id = "video-cursor";
    cursor.innerHTML =
      '<svg width="22" height="28" viewBox="0 0 30 38"><path d="M3 2v29l7-7 6 12 5-3-6-11 10-1Z" fill="#242c2b" stroke="#fafaf6" stroke-width="2"/></svg>';
    cursor.style.cssText =
      "position:fixed;left:0;top:0;transform:translate(1000px,500px);z-index:2147483646;pointer-events:none;filter:drop-shadow(0 1px 2px #0003)";
    document.body.appendChild(cursor);
    window.__pointer = { x: 1000, y: 500 };
    const sync = document.createElement("div");
    sync.id = "video-sync";
    sync.style.cssText =
      "position:fixed;inset:0;z-index:2147483647;background:#ff00ff";
    document.body.appendChild(sync);
  });
  await page.waitForTimeout(350);
  await page.evaluate(() => document.getElementById("video-sync").remove());
  const start = Date.now();
  const actions = [];
  const elapsed = () => Math.round((Date.now() - start) / 10) / 100;
  async function at(seconds) {
    const wait = seconds * 1000 - (Date.now() - start);
    if (wait > 0) await page.waitForTimeout(wait);
  }
  async function move(locator) {
    const box = await locator.boundingBox();
    if (!box) throw new Error("Missing interaction target");
    const x = box.x + box.width / 2,
      y = box.y + box.height / 2;
    await page.evaluate(
      async ({ x, y, zoom }) => {
        const el = document.getElementById("video-cursor");
        x /= zoom;
        y /= zoom;
        const from = window.__pointer;
        const start = performance.now();
        await new Promise((resolve) => {
          const step = (now) => {
            const t = Math.min((now - start) / 360, 1),
              e = t * t * (3 - 2 * t);
            el.style.transform = `translate(${from.x + (x - from.x) * e}px,${from.y + (y - from.y) * e}px)`;
            if (t < 1) requestAnimationFrame(step);
            else resolve();
          };
          requestAnimationFrame(step);
        });
        window.__pointer = { x, y };
      },
      { x, y, zoom },
    );
    await page.mouse.move(x, y);
  }
  async function click(locator, name) {
    await move(locator);
    await locator.click();
    actions.push({ time: elapsed(), action: name });
    console.log(`${elapsed()}s ${name}`);
    await page.evaluate(() =>
      document
        .getElementById("video-cursor")
        .animate([{ opacity: 1 }, { opacity: 0.4 }, { opacity: 1 }], {
          duration: 180,
        }),
    );
  }
  async function type(text) {
    const input = page.locator("textarea").last();
    await click(input, "focus prompt");
    await input.pressSequentially(text, { delay: 24 });
  }
  async function shot(name) {
    await page.screenshot({ path: `${target}/${name}.png` });
  }
  await page.getByText("Center the down icon").first().waitFor();
  await shot("start");
  await at(0.6);
  await click(
    page.getByText("Center the down icon").first(),
    "open the conversation",
  );
  await page.getByText("Reasoning").first().waitFor();
  await at(3.2);
  const history = page.getByText(/git log/).first();
  await history.waitFor();
  await click(history, "open git history");
  await at(6.4);
  await shot("history");
  await at(7.2);
  await click(
    page.getByRole("button", { name: "Codex", exact: true }),
    "open subscribed agents",
  );
  await at(8.6);
  await shot("agents");
  await click(page.getByRole("option", { name: /Claude/ }), "switch to Claude");
  await at(9.4);
  await type("Keep the icon centered and check the diff.");
  await at(12.2);
  await page.locator("textarea").last().press("Enter");
  actions.push({ time: elapsed(), action: "continue with Claude" });
  await at(16.2);
  await shot("claude");
  await click(
    page.getByRole("button", { name: "Open workspace panel" }),
    "open workspace",
  );
  await at(16.4);
  await click(page.getByText("src", { exact: true }), "expand source folder");
  await at(17.0);
  await click(
    page.getByText("CommandPalette.tsx", { exact: true }),
    "open edited component",
  );
  await at(17.7);
  await click(
    page.getByRole("button", { name: "Show unstaged diff", exact: true }),
    "review diff",
  );
  await at(19.6);
  await shot("diff");
  await at(20.2);
  await page.keyboard.press("Escape");
  actions.push({ time: elapsed(), action: "close workspace" });
  await at(20.8);
  await click(
    page.getByRole("button", { name: "New chat", exact: false }).first(),
    "leave conversation",
  );
  await at(22.0);
  await click(
    page
      .getByText("Build a command palette for Orbit.", { exact: true })
      .first(),
    "reopen saved conversation",
  );
  await at(24.4);
  await shot("saved");
  await at(30.4);
  const raw = await page.video().path();
  await context.close();
  if (errors.length) throw new Error(errors.join("\n"));
  const pixels = execFileSync(
    "ffmpeg",
    [
      "-v",
      "error",
      "-i",
      raw,
      "-vf",
      "fps=60,crop=2:2:14:14,scale=1:1",
      "-pix_fmt",
      "rgb24",
      "-f",
      "rawvideo",
      "pipe:1",
    ],
    { maxBuffer: 4 * 1024 * 1024 },
  );
  let marker = false,
    offset = null;
  for (let i = 0; i < pixels.length; i += 3) {
    const magenta =
      pixels[i] > 180 && pixels[i + 1] < 70 && pixels[i + 2] > 180;
    if (magenta) marker = true;
    else if (marker) {
      offset = i / 3 / 60;
      break;
    }
  }
  if (offset === null) throw new Error("Missing video synchronization marker");
  execFileSync(
    "ffmpeg",
    [
      "-hide_banner",
      "-loglevel",
      "error",
      "-y",
      "-ss",
      String(offset),
      "-i",
      raw,
      "-t",
      "30",
      "-an",
      "-c:v",
      "libx264",
      "-preset",
      "fast",
      "-crf",
      "14",
      "-pix_fmt",
      "yuv420p",
      "-r",
      "60",
      "-movflags",
      "+faststart",
      `${target}/app-walkthrough.mp4`,
    ],
    { stdio: "inherit" },
  );
  await writeFile(
    `${target}/manifest.json`,
    JSON.stringify(
      {
        source: "crates/application",
        viewport: { width: 1920, height: 1080 },
        recording: { width: 1920, height: 1080 },
        browserZoom: zoom,
        fps: 60,
        duration: 30,
        fixture: "scripts/demo-bridge.mjs",
        recordingOffset: offset,
        actions,
        errors,
      },
      null,
      2,
    ),
  );
  console.log("Captured one continuous 30-second walkthrough.");
} finally {
  await browser?.close();
  server.kill();
}
