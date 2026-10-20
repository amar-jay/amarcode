import { execFileSync } from "node:child_process";
import { writeFileSync, mkdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
const root = fileURLToPath(new URL("../", import.meta.url));
const file = `${root}out/amarcode-launch.mp4`;
const info = JSON.parse(
  execFileSync(
    "ffprobe",
    ["-v", "error", "-show_streams", "-show_format", "-of", "json", file],
    { encoding: "utf8" },
  ),
);
const v = info.streams.find((s) => s.codec_type === "video");
const a = info.streams.find((s) => s.codec_type === "audio");
const capture = JSON.parse(
  readFileSync(`${root}public/walkthrough/manifest.json`, "utf8"),
);
const recordedActions = capture.actions.map((action) => action.action);
const checks = {
  duration: Math.abs(Number(info.format.duration) - 30) < 0.05,
  resolution: v.width === 1920 && v.height === 1080,
  codec: v.codec_name === "h264",
  frameRate: v.avg_frame_rate === "60/1",
  frameCount: Number(v.nb_frames) === 1800,
  stereoAudio: a?.channels === 2,
  audioDuration: Math.abs(Number(a?.duration) - 30) < 0.1,
  firstHandoff: recordedActions.includes("switch to Claude"),
  returnHandoff: recordedActions.includes("return to Codex"),
  savedConversation: recordedActions.includes("reopen saved conversation"),
  browserErrors: capture.errors.length === 0,
};
console.log(checks);
mkdirSync(`${root}out`, { recursive: true });
writeFileSync(
  `${root}out/verification.json`,
  JSON.stringify(
    { checks, video: v, audio: a, duration: info.format.duration },
    null,
    2,
  ),
);
if (Object.values(checks).some((ok) => !ok)) process.exit(1);
console.log(
  "Verified: 30 seconds · 1080p · 60 fps · 1,800 frames · H.264 · stereo audio",
);
