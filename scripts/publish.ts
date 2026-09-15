import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const WORKFLOW = "release-desktop.yml";
const REQUIRED_BRANCH = "main";
const repositoryRoot = dirname(dirname(fileURLToPath(import.meta.url)));

function fail(message: string): never {
  console.error(`publish: ${message}`);
  process.exit(1);
}

function run(
  command: string,
  args: string[],
  options: { capture?: boolean } = {},
) {
  const result = spawnSync(command, args, {
    cwd: repositoryRoot,
    encoding: "utf8",
    stdio: options.capture ? "pipe" : "inherit",
  });

  if (result.error) {
    if ((result.error as NodeJS.ErrnoException).code === "ENOENT") {
      fail(`'${command}' is not installed or is not available on PATH.`);
    }
    fail(`could not run '${command}': ${result.error.message}`);
  }

  return result;
}

function output(command: string, args: string[]): string {
  const result = run(command, args, { capture: true });
  if (result.status !== 0) {
    const detail = result.stderr.trim();
    fail(detail || `'${command} ${args.join(" ")}' failed.`);
  }
  return result.stdout.trim();
}

run("gh", ["--version"], { capture: true });
run("git", ["--version"], { capture: true });

if (output("git", ["rev-parse", "--is-inside-work-tree"]) !== "true") {
  fail("this script is not inside a Git repository.");
}

const branch = output("git", ["branch", "--show-current"]);
if (!branch) {
  fail("HEAD is detached; switch to the main branch before publishing.");
}
if (branch !== REQUIRED_BRANCH) {
  fail(`current branch is '${branch}'; switch to '${REQUIRED_BRANCH}' first.`);
}

const auth = run("gh", ["auth", "status"]);
if (auth.status !== 0) {
  fail("GitHub CLI is not authenticated. Run 'gh auth login' first.");
}

const workflowPath = join(repositoryRoot, ".github", "workflows", WORKFLOW);
if (!existsSync(workflowPath)) {
  fail(`workflow '.github/workflows/${WORKFLOW}' does not exist.`);
}

console.log(`Triggering ${WORKFLOW} on ${REQUIRED_BRANCH}...`);
const dispatch = run("gh", [
  "workflow",
  "run",
  WORKFLOW,
  "--ref",
  REQUIRED_BRANCH,
]);
if (dispatch.status !== 0) {
  fail("GitHub rejected the workflow dispatch.");
}

console.log("Desktop release workflow triggered successfully.");
