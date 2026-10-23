import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const debug =
  process.argv.includes("--debug") || process.env.TAURI_ENV_DEBUG === "true";
const targetArgument = process.argv.indexOf("--target");
const requestedTarget =
  targetArgument < 0 ? undefined : process.argv[targetArgument + 1];
if (
  targetArgument >= 0 &&
  (!requestedTarget || requestedTarget.startsWith("--"))
) {
  throw new Error("--target requires a Rust target triple");
}
function run(command: string, args: string[], capture = false): string {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: "utf8",
    stdio: capture ? "pipe" : "inherit",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(
      `${command} failed (${result.status}): ${result.stderr ?? ""}`,
    );
  }
  return result.stdout?.trim() ?? "";
}

// Tauri supplies this for its hooks, including cross-compilation. The explicit
// argument also supports preparing sidecars before invoking Cargo directly.
const target =
  requestedTarget ??
  process.env.TAURI_ENV_TARGET_TRIPLE ??
  process.env.CARGO_BUILD_TARGET ??
  run("rustc", ["--print", "host-tuple"], true);
if (!/^[a-zA-Z0-9_]+(?:-[a-zA-Z0-9_]+)+$/.test(target)) {
  throw new Error(`Invalid Rust target triple: ${target}`);
}
const profile = debug ? "debug" : "release";
run("cargo", [
  "build",
  "--locked",
  "-p",
  "amarcode-daemon",
  "--bin",
  "amarcode-daemon",
  "--target",
  target,
  ...(debug ? [] : ["--release"]),
]);
const extension = target.includes("windows") ? ".exe" : "";
const targetDir = process.env.CARGO_TARGET_DIR
  ? resolve(root, process.env.CARGO_TARGET_DIR)
  : join(root, "target");
const destination = join(
  root,
  "crates/application/src-tauri/binaries",
  `amarcode-bootstrap-${target}${extension}`,
);
mkdirSync(dirname(destination), { recursive: true });
copyFileSync(
  join(targetDir, target, profile, `amarcode-daemon${extension}`),
  destination,
);
console.log(`Prepared bundled daemon: ${destination}`);
