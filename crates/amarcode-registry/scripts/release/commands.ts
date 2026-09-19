import { join, relative, resolve } from "node:path";

export const projectRoot = resolve(import.meta.dir, "..", "..", "..", "..");
export const registryDirectory = join(
  projectRoot,
  "crates",
  "amarcode-registry",
);
export const wranglerConfig = join(registryDirectory, "wrangler.jsonc");
export const cargoLockPath = join(projectRoot, "Cargo.lock");

export function run(
  command: string[],
  options: { cwd?: string; quiet?: boolean; allowFailure?: boolean } = {},
) {
  if (!options.quiet) console.log(`$ ${command.join(" ")}`);
  const result = Bun.spawnSync(command, {
    cwd: options.cwd ?? projectRoot,
    stdout: options.quiet ? "pipe" : "inherit",
    stderr: options.quiet ? "pipe" : "inherit",
  });
  if (!result.success && !options.allowFailure) {
    const detail = options.quiet ? `\n${result.stderr.toString().trim()}` : "";
    throw new Error(
      `command failed (${result.exitCode}): ${command.join(" ")}${detail}`,
    );
  }
  return result;
}

export function output(command: string[]): string {
  return run(command, { quiet: true }).stdout.toString().trim();
}

export function gitCommit(): string {
  const result = run(["git", "rev-parse", "--verify", "HEAD^{commit}"], {
    quiet: true,
    allowFailure: true,
  });
  if (!result.success) throw new Error("publishing requires a Git commit");
  return result.stdout.toString().trim();
}

export function gitDirty(): boolean {
  const result = run(["git", "status", "--porcelain"], {
    quiet: true,
    allowFailure: true,
  });
  return !result.success || result.stdout.toString().trim().length > 0;
}

export function gitStatusShort(): string {
  return output(["git", "status", "--short"]);
}

export function gitPorcelainPaths(): string[] {
  const text = run(["git", "status", "--porcelain"], {
    quiet: true,
    allowFailure: true,
  })
    .stdout.toString()
    .trim();
  if (!text) return [];
  return text.split("\n").flatMap((line) => {
    const renamed = line.match(/^R. (.+) -> (.+)$/);
    if (renamed) return [renamed[2]];
    const path = line.slice(3);
    return path ? [path] : [];
  });
}

export function repoPath(absolutePath: string): string {
  return relative(projectRoot, absolutePath);
}

export function gitBranch(): string {
  return output(["git", "branch", "--show-current"]);
}

export function hostTarget(): string {
  const match = output(["rustc", "-vV"]).match(/^host:\s*(.+)$/m);
  if (!match) throw new Error("rustc did not report a host target");
  return match[1].trim();
}

export function validateSegment(label: string, value: string) {
  if (!/^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$/.test(value)) {
    throw new Error(`${label} contains unsupported characters: ${value}`);
  }
}

export const defaultTargets = [
  "x86_64-unknown-linux-gnu",
  "x86_64-pc-windows-gnu",
];
