import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const file = `${root}out/promo-launch.mp4`;

const probe = (args) =>
  execFileSync("ffprobe", ["-v", "error", "-of", "json", ...args, file]).toString();

const format = JSON.parse(probe(["-show_format"]));
const streams = JSON.parse(probe(["-show_streams"])).streams;

const duration = Number(format.format.duration);
const video = streams.find((s) => s.codec_type === "video");
const audio = streams.find((s) => s.codec_type === "audio");

const problems = [];
if (Math.abs(duration - 20) > 0.05) problems.push(`duration ${duration}`);
if (video?.width !== 1920) problems.push(`width ${video?.width}`);
if (video?.height !== 1080) problems.push(`height ${video?.height}`);
if (video?.codec_name !== "h264") problems.push(`codec ${video?.codec_name}`);
if (audio) problems.push(`unexpected audio stream`);
const fps = eval(video?.r_frame_rate ?? "0");
if (fps !== 60) problems.push(`fps ${fps}`);

if (problems.length) {
  console.error(`verify failed: ${problems.join(", ")}`);
  process.exit(1);
}
console.log(`verified: ${duration}s, ${video.width}x${video.height}, ${video.codec_name}, ${fps}fps, no audio`);
