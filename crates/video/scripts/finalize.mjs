// AAC packet padding can extend a nominally 30-second Remotion file by ~59ms.
// Preserve the video bitstream and trim the audio/container to exactly 30s.
import { execFileSync } from "node:child_process";
import { renameSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const output = `${root}out/amarcode-launch.mp4`;
const temp = `${root}out/amarcode-launch-final.mp4`;
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
    "30",
    "-c:v",
    "copy",
    "-c:a",
    "aac",
    "-b:a",
    "320k",
    "-movflags",
    "+faststart",
    temp,
  ],
  { stdio: "inherit" },
);
renameSync(temp, output);
console.log("Finalized exact 30-second MP4 with streaming-friendly metadata.");
