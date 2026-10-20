// Preserve the video bitstream and re-mux to exactly 20 seconds.
import { execFileSync } from "node:child_process";
import { renameSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const output = `${root}out/promo-launch.mp4`;
const temp = `${root}out/promo-launch-final.mp4`;
execFileSync(
  "ffmpeg",
  [
    "-hide_banner",
    "-loglevel",
    "error",
    "-y",
    "-i",
    output,
    "-t",
    "20",
    "-c:v",
    "copy",
    "-an",
    "-movflags",
    "+faststart",
    temp,
  ],
  { stdio: "inherit" },
);
renameSync(temp, output);
console.log("Finalized exact 20-second MP4 with streaming-friendly metadata.");
