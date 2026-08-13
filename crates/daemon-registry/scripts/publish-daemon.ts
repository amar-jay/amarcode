#!/usr/bin/env bun

import { createHash, createPrivateKey, createPublicKey, sign } from "node:crypto";
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";

type Artifact = {
  target: string;
  url: string;
  sha256: string;
  size: number;
};

type Manifest = {
  version: string;
  protocolVersion: number;
  publishedAt: string;
  sourceCommit?: string;
  artifacts: Record<string, Artifact>;
};

type Options = {
  version: string;
  targets: string[];
  bucket: string;
  skipBuild: boolean;
  skipDeploy: boolean;
  dryRun: boolean;
};

type BuiltArtifact = {
  artifact: Artifact;
  binaryPath: string;
  key: string;
};

const requiredLifecycleCommands = ["install", "start", "restart", "status"];

const projectRoot = resolve(import.meta.dir, "..", "..", "..");
const workerDirectory = join(projectRoot, "crates", "daemon-registry");
const wranglerConfig = join(workerDirectory, "wrangler.jsonc");
const releasePublicKey = "5ef56cd7772e8c601ca9c5a15378b7088fc558e7edcde73770cbb116d9e255d2";

function run(command: string[], options: { cwd?: string; quiet?: boolean; allowFailure?: boolean } = {}) {
  if (!options.quiet) console.log(`$ ${command.join(" ")}`);
  const result = Bun.spawnSync(command, {
    cwd: options.cwd ?? projectRoot,
    stdout: options.quiet ? "pipe" : "inherit",
    stderr: options.quiet ? "pipe" : "inherit",
  });
  if (!result.success && !options.allowFailure) {
    const detail = options.quiet ? `\n${result.stderr.toString().trim()}` : "";
    throw new Error(`command failed (${result.exitCode}): ${command.join(" ")}${detail}`);
  }
  return result;
}

function output(command: string[]): string {
  const result = run(command, { quiet: true });
  return result.stdout.toString().trim();
}

function cargoValue(manifestPath: string, key: string): string {
  const contents = readFileSync(manifestPath, "utf8");
  const match = contents.match(new RegExp(`^${key}\\s*=\\s*\"([^\"]+)\"`, "m"));
  if (!match) throw new Error(`could not find ${key} in ${manifestPath}`);
  return match[1];
}

function protocolVersion(): number {
  const contents = readFileSync(join(projectRoot, "crates", "protocol", "src", "lib.rs"), "utf8");
  const match = contents.match(/pub const PROTOCOL_VERSION:\s*u32\s*=\s*(\d+)\s*;/);
  if (!match) throw new Error("could not read PROTOCOL_VERSION");
  return Number(match[1]);
}

function hostTarget(): string {
  const match = output(["rustc", "-vV"]).match(/^host:\s*(.+)$/m);
  if (!match) throw new Error("rustc did not report a host target");
  return match[1].trim();
}

function verifyLifecycleCli(binaryPath: string) {
  const result = run([binaryPath, "--help"], { quiet: true });
  const help = result.stdout.toString();
  for (const command of requiredLifecycleCommands) {
    const advertised = help
      .split(/\r?\n/)
      .some((line) => line.trimStart().startsWith(command));
    if (!advertised) {
      throw new Error(
        `daemon release binary is missing the required ${command} lifecycle command`,
      );
    }
  }
}

function parseArgs(args: string[]): Options {
  const values = new Map<string, string>();
  const targets: string[] = [];
  const flags = new Set<string>();
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (["--skip-build", "--skip-deploy", "--dry-run"].includes(argument)) {
      flags.add(argument);
      continue;
    }
    if (["--version", "--target", "--bucket"].includes(argument)) {
      const value = args[index + 1];
      if (!value || value.startsWith("--")) throw new Error(`${argument} requires a value`);
      if (argument === "--target") targets.push(value);
      else values.set(argument, value);
      index += 1;
      continue;
    }
    if (argument === "--help" || argument === "-h") {
      console.log(`Usage: bun run daemon:publish [options]

Options:
  --version <version>  Release version (default: crates/daemon/Cargo.toml)
  --target <triple>    Rust target triple; repeat to publish multiple targets
                       (default: rustc host)
  --bucket <name>      R2 bucket (default: AMARCODE_DAEMON_BUCKET or amarcode-daemons)
  --skip-build         Publish an already-built release binary
  --skip-deploy        Upload artifacts without deploying the Worker
  --dry-run            Build and generate metadata without Cloudflare writes
`);
      process.exit(0);
    }
    throw new Error(`unknown argument: ${argument}`);
  }

  return {
    version:
      values.get("--version") ??
      cargoValue(join(projectRoot, "crates", "daemon", "Cargo.toml"), "version"),
    targets: [...new Set(targets.length > 0 ? targets : [hostTarget()])],
    bucket: values.get("--bucket") ?? process.env.AMARCODE_DAEMON_BUCKET ?? "amarcode-daemons",
    skipBuild: flags.has("--skip-build"),
    skipDeploy: flags.has("--skip-deploy"),
    dryRun: flags.has("--dry-run"),
  };
}

function validateSegment(label: string, value: string) {
  if (!/^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$/.test(value)) {
    throw new Error(`${label} contains unsupported characters: ${value}`);
  }
}

function loadRemoteManifest(bucket: string, key: string, path: string): Manifest | null {
  const result = run(
    [
      "bunx",
      "wrangler",
      "r2",
      "object",
      "get",
      `${bucket}/${key}`,
      "--file",
      path,
      "--remote",
      "--config",
      wranglerConfig,
    ],
    { quiet: true, allowFailure: true },
  );
  if (!result.success) {
    const stderr = result.stderr.toString();
    if (stderr.includes("The specified key does not exist")) return null;
    throw new Error(`could not read existing manifest from R2:\n${stderr.trim()}`);
  }
  if (!existsSync(path)) {
    throw new Error("Wrangler reported a successful manifest download but created no file");
  }
  return JSON.parse(readFileSync(path, "utf8")) as Manifest;
}

function upload(bucket: string, key: string, path: string, contentType: string, cacheControl: string) {
  run([
    "bunx",
    "wrangler",
    "r2",
    "object",
    "put",
    `${bucket}/${key}`,
    "--file",
    path,
    "--content-type",
    contentType,
    "--cache-control",
    cacheControl,
    "--remote",
    "--config",
    wranglerConfig,
  ]);
}

function signManifest(contents: string): string {
  const keyPath =
    process.env.AMARCODE_DAEMON_SIGNING_KEY ??
    join(homedir(), ".config", "amarcode", "daemon-release-signing-key.pem");
  if (!existsSync(keyPath)) {
    throw new Error(
      `daemon signing key not found at ${keyPath}; set AMARCODE_DAEMON_SIGNING_KEY`,
    );
  }

  const privateKey = createPrivateKey(readFileSync(keyPath));
  if (privateKey.asymmetricKeyType !== "ed25519") {
    throw new Error("daemon signing key must be an Ed25519 private key");
  }
  const publicDer = createPublicKey(privateKey).export({ format: "der", type: "spki" });
  const actualPublicKey = publicDer.subarray(publicDer.byteLength - 32).toString("hex");
  if (actualPublicKey !== releasePublicKey) {
    throw new Error(
      `daemon signing key does not match the application public key (${actualPublicKey})`,
    );
  }

  return sign(null, Buffer.from(contents), privateKey).toString("base64");
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const currentHostTarget = hostTarget();
  validateSegment("version", options.version);
  for (const target of options.targets) validateSegment("target", target);

  const builtArtifacts: BuiltArtifact[] = options.targets.map((target) => {
    if (!options.skipBuild) {
      run([
        "cargo",
        "build",
        "--release",
        "--locked",
        "-p",
        "amarcode-daemon",
        "--bin",
        "amarcode-daemon",
        "--target",
        target,
      ]);
    }

    const filename = target.includes("windows") ? "amarcode-daemon.exe" : "amarcode-daemon";
    const binaryPath = join(projectRoot, "target", target, "release", filename);
    if (!existsSync(binaryPath)) throw new Error(`daemon binary not found at ${binaryPath}`);
    if (target === currentHostTarget) verifyLifecycleCli(binaryPath);

    const binary = readFileSync(binaryPath);
    const artifact: Artifact = {
      target,
      url: `/v1/daemon/${options.version}/${target}`,
      sha256: createHash("sha256").update(binary).digest("hex"),
      size: binary.byteLength,
    };
    return {
      artifact,
      binaryPath,
      key: `daemon/${options.version}/${target}/${filename}`,
    };
  });

  const temporaryDirectory = mkdtempSync(join(tmpdir(), "amarcode-daemon-publish-"));
  try {
    const versionManifestPath = join(temporaryDirectory, "manifest.json");
    const versionManifestKey = `daemon/${options.version}/manifest.json`;
    const existing = options.dryRun
      ? null
      : loadRemoteManifest(options.bucket, versionManifestKey, versionManifestPath);
    const commitResult = run(["git", "rev-parse", "HEAD"], { quiet: true, allowFailure: true });
    const worktreeResult = run(["git", "status", "--porcelain"], {
      quiet: true,
      allowFailure: true,
    });
    const worktreeClean =
      worktreeResult.success && worktreeResult.stdout.toString().trim().length === 0;
    const sourceCommit =
      commitResult.success && worktreeClean
        ? commitResult.stdout.toString().trim()
        : undefined;
    if (!worktreeClean) {
      console.warn("Publishing from a dirty worktree; sourceCommit will be omitted.");
    }
    const manifest: Manifest = {
      version: options.version,
      protocolVersion: protocolVersion(),
      publishedAt: new Date().toISOString(),
      ...(sourceCommit ? { sourceCommit } : {}),
      artifacts: {
        ...(existing?.version === options.version ? existing.artifacts : {}),
        ...Object.fromEntries(
          builtArtifacts.map(({ artifact }) => [artifact.target, artifact]),
        ),
      },
    };
    const manifestContents = `${JSON.stringify(manifest, null, 2)}\n`;
    writeFileSync(versionManifestPath, manifestContents);

    const latestPath = join(temporaryDirectory, "latest.json");
    writeFileSync(latestPath, manifestContents);
    const signaturePath = join(temporaryDirectory, "manifest.sig");
    writeFileSync(signaturePath, `${signManifest(manifestContents)}\n`);

    console.log(`\nDaemon ${options.version}`);
    for (const { artifact, binaryPath } of builtArtifacts) {
      console.log(`  ${artifact.target}`);
      console.log(`    binary: ${binaryPath}`);
      console.log(`    size:   ${artifact.size} bytes`);
      console.log(`    sha256: ${artifact.sha256}`);
    }

    if (options.dryRun) {
      console.log("\nDry run complete; no Cloudflare resources were changed.");
      console.log(readFileSync(versionManifestPath, "utf8"));
      return;
    }

    for (const { key, binaryPath } of builtArtifacts) {
      upload(
        options.bucket,
        key,
        binaryPath,
        "application/octet-stream",
        "public, max-age=31536000, immutable",
      );
    }
    upload(
      options.bucket,
      `${versionManifestKey}.sig`,
      signaturePath,
      "text/plain; charset=utf-8",
      "public, max-age=60, must-revalidate",
    );
    upload(options.bucket, versionManifestKey, versionManifestPath, "application/json", "public, max-age=60, must-revalidate");
    upload(
      options.bucket,
      "daemon/latest.json.sig",
      signaturePath,
      "text/plain; charset=utf-8",
      "public, max-age=60, must-revalidate",
    );
    upload(options.bucket, "daemon/latest.json", latestPath, "application/json", "public, max-age=60, must-revalidate");

    if (!options.skipDeploy) {
      run(["bunx", "wrangler", "deploy", "--config", wranglerConfig], { cwd: workerDirectory });
    }
    console.log("\nDaemon publication completed successfully.");
  } finally {
    rmSync(temporaryDirectory, { recursive: true, force: true });
  }
}

if (import.meta.main) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  });
}
