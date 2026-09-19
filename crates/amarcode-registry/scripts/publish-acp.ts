#!/usr/bin/env bun

import {
  createHash,
  createPrivateKey,
  createPublicKey,
  sign,
} from "node:crypto";
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";
import * as prompts from "@clack/prompts";

type Artifact = {
  target: string;
  url: string;
  sha256: string;
  size: number;
};

type Manifest = {
  version: string;
  publishedAt: string;
  sourceCommit: string;
  sourceDirty?: boolean;
  artifacts: Record<string, Artifact>;
};

type Options = {
  version: string;
  versionProvided: boolean;
  targets: string[];
  bucket: string;
  overwrite: boolean;
  skipBuild: boolean;
  skipDeploy: boolean;
  dryRun: boolean;
};

const projectRoot = resolve(import.meta.dir, "..", "..", "..");
const registryDirectory = join(projectRoot, "crates", "amarcode-registry");
const wranglerConfig = join(registryDirectory, "wrangler.jsonc");
const packageManifest = join(
  projectRoot,
  "crates",
  "amarcode-acp",
  "Cargo.toml",
);
const cargoLock = join(projectRoot, "Cargo.lock");
const releasePublicKey =
  "5ef56cd7772e8c601ca9c5a15378b7088fc558e7edcde73770cbb116d9e255d2";

function run(
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

function output(command: string[]): string {
  return run(command, { quiet: true }).stdout.toString().trim();
}

function packageVersion(): string {
  const match = readFileSync(packageManifest, "utf8").match(
    /^version\s*=\s*"([^"]+)"/m,
  );
  if (!match) throw new Error(`could not read version from ${packageManifest}`);
  return match[1];
}

function setPackageVersion(version: string) {
  const current = packageVersion();
  if (current === version) return;
  const manifest = readFileSync(packageManifest, "utf8");
  const updatedManifest = manifest.replace(
    /^version\s*=\s*"[^"]+"/m,
    `version = "${version}"`,
  );
  const lock = readFileSync(cargoLock, "utf8");
  const updatedLock = lock.replace(
    /(?<=\[\[package\]\]\nname = "amarcode-acp"\nversion = ")[^"]+(?=")/,
    version,
  );
  if (updatedManifest === manifest || updatedLock === lock) {
    throw new Error("could not update the amarcode-acp package version");
  }
  writeFileSync(packageManifest, updatedManifest);
  writeFileSync(cargoLock, updatedLock);
  console.log(`Updated amarcode-acp package version: ${current} -> ${version}`);
}

function hostTarget(): string {
  const match = output(["rustc", "-vV"]).match(/^host:\s*(.+)$/m);
  if (!match) throw new Error("rustc did not report a host target");
  return match[1].trim();
}

function bump(version: string, kind: "major" | "minor" | "patch"): string {
  const match = version.match(/^(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z.-]+)?$/);
  if (!match)
    throw new Error(`cannot ${kind}-bump non-semver version ${version}`);
  const [major, minor, patch] = match.slice(1).map(Number);
  if (kind === "major") return `${major + 1}.0.0`;
  if (kind === "minor") return `${major}.${minor + 1}.0`;
  return `${major}.${minor}.${patch + 1}`;
}

function validateSegment(label: string, value: string) {
  if (!/^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$/.test(value)) {
    throw new Error(`${label} contains unsupported characters: ${value}`);
  }
}

function parseArgs(args: string[]): Options {
  const values = new Map<string, string>();
  const targets: string[] = [];
  const flags = new Set<string>();
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (
      ["--overwrite", "--skip-build", "--skip-deploy", "--dry-run"].includes(
        argument,
      )
    ) {
      flags.add(argument);
      continue;
    }
    if (["--version", "--target", "--bucket"].includes(argument)) {
      const value = args[++index];
      if (!value || value.startsWith("--"))
        throw new Error(`${argument} requires a value`);
      if (argument === "--target") targets.push(value);
      else values.set(argument, value);
      continue;
    }
    if (argument === "--help" || argument === "-h") {
      console.log(`Usage: bun run acp:publish [options]

Options:
  --version <version>  Release version; updates Cargo.toml and Cargo.lock
  --overwrite          Replace an existing version intentionally
  --target <triple>    Rust target triple; repeat for multiple targets
  --bucket <name>      R2 bucket (default: AMARCODE_ACP_BUCKET or amarcode-daemons)
  --skip-build         Publish existing release binaries
  --skip-deploy        Upload without deploying the registry Worker
  --dry-run            Build and generate metadata without Cloudflare writes`);
      process.exit(0);
    }
    throw new Error(`unknown argument: ${argument}`);
  }
  const suppliedVersion = values.get("--version");
  return {
    version: suppliedVersion ?? packageVersion(),
    versionProvided: Boolean(suppliedVersion),
    targets: [...new Set(targets.length ? targets : [hostTarget()])],
    bucket:
      values.get("--bucket") ??
      process.env.AMARCODE_ACP_BUCKET ??
      "amarcode-daemons",
    overwrite: flags.has("--overwrite"),
    skipBuild: flags.has("--skip-build"),
    skipDeploy: flags.has("--skip-deploy"),
    dryRun: flags.has("--dry-run"),
  };
}

async function interactive(options: Options): Promise<Options | null> {
  if (
    options.versionProvided ||
    !process.stdin.isTTY ||
    !process.stdout.isTTY
  ) {
    if (!options.versionProvided) {
      throw new Error("no interactive terminal; supply --version <version>");
    }
    return options;
  }
  const current = packageVersion();
  prompts.intro("Amarcode ACP release");
  const kind = await prompts.select({
    message: "Release version",
    initialValue: "patch",
    options: [
      { value: "patch", label: "Patch", hint: bump(current, "patch") },
      { value: "minor", label: "Minor", hint: bump(current, "minor") },
      { value: "major", label: "Major", hint: bump(current, "major") },
      {
        value: "current",
        label: "Keep current",
        hint: "deliberate replacement",
      },
    ],
  });
  if (prompts.isCancel(kind)) return null;
  const version = kind === "current" ? current : bump(current, kind);
  const confirmed = await prompts.confirm({
    message: `Build and publish amarcode-acp ${version} for ${options.targets.join(", ")}?`,
    initialValue: false,
  });
  if (prompts.isCancel(confirmed) || !confirmed) return null;
  return {
    ...options,
    version,
    versionProvided: true,
    overwrite: kind === "current",
  };
}

function loadRemoteManifest(
  bucket: string,
  key: string,
  destination: string,
): Manifest | null {
  const result = run(
    [
      "bunx",
      "wrangler",
      "r2",
      "object",
      "get",
      `${bucket}/${key}`,
      "--file",
      destination,
      "--remote",
      "--config",
      wranglerConfig,
    ],
    { quiet: true, allowFailure: true },
  );
  if (!result.success) {
    const detail = result.stderr.toString();
    if (detail.includes("The specified key does not exist")) return null;
    throw new Error(
      `could not read existing ACP manifest from R2:\n${detail.trim()}`,
    );
  }
  return JSON.parse(readFileSync(destination, "utf8")) as Manifest;
}

function upload(
  bucket: string,
  key: string,
  path: string,
  contentType: string,
  cacheControl: string,
) {
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
    process.env.AMARCODE_ACP_SIGNING_KEY ??
    process.env.AMARCODE_DAEMON_SIGNING_KEY ??
    join(homedir(), ".config", "amarcode", "daemon-release-signing-key.pem");
  if (!existsSync(keyPath))
    throw new Error(`ACP signing key not found at ${keyPath}`);
  const privateKey = createPrivateKey(readFileSync(keyPath));
  if (privateKey.asymmetricKeyType !== "ed25519")
    throw new Error("ACP signing key must be Ed25519");
  const publicDer = createPublicKey(privateKey).export({
    format: "der",
    type: "spki",
  });
  const actual = publicDer.subarray(publicDer.byteLength - 32).toString("hex");
  if (actual !== releasePublicKey)
    throw new Error(`ACP signing key has unexpected public key ${actual}`);
  return sign(null, Buffer.from(contents), privateKey).toString("base64");
}

async function main() {
  const selected = await interactive(parseArgs(process.argv.slice(2)));
  if (!selected) return;
  const options = selected;
  validateSegment("version", options.version);
  options.targets.forEach((target) => validateSegment("target", target));
  if (options.version !== packageVersion()) {
    if (options.skipBuild)
      throw new Error("cannot change version with --skip-build");
    setPackageVersion(options.version);
  }

  const artifacts = options.targets.map((target) => {
    if (!options.skipBuild)
      run([
        "cargo",
        "build",
        "--release",
        "--locked",
        "-p",
        "amarcode-acp",
        "--bin",
        "amarcode-acp",
        "--target",
        target,
      ]);
    const filename = target.includes("windows")
      ? "amarcode-acp.exe"
      : "amarcode-acp";
    const binaryPath = join(projectRoot, "target", target, "release", filename);
    if (!existsSync(binaryPath))
      throw new Error(`ACP binary not found at ${binaryPath}`);
    const bytes = readFileSync(binaryPath);
    const artifact: Artifact = {
      target,
      url: `/v1/acp/${options.version}/${target}`,
      sha256: createHash("sha256").update(bytes).digest("hex"),
      size: bytes.byteLength,
    };
    return {
      artifact,
      binaryPath,
      key: `acp/${options.version}/${target}/${filename}`,
    };
  });

  const temporaryDirectory = mkdtempSync(
    join(tmpdir(), "amarcode-acp-publish-"),
  );
  try {
    const manifestPath = join(temporaryDirectory, "manifest.json");
    const manifestKey = `acp/${options.version}/manifest.json`;
    const existing = options.dryRun
      ? null
      : loadRemoteManifest(options.bucket, manifestKey, manifestPath);
    if (existing && !options.overwrite) {
      throw new Error(
        `ACP version ${options.version} already exists; use --overwrite intentionally`,
      );
    }
    const dirty = output(["git", "status", "--porcelain"]).length > 0;
    const manifest: Manifest = {
      version: options.version,
      publishedAt: new Date().toISOString(),
      sourceCommit: output(["git", "rev-parse", "--verify", "HEAD^{commit}"]),
      ...(dirty ? { sourceDirty: true } : {}),
      artifacts: {
        ...(existing?.version === options.version ? existing.artifacts : {}),
        ...Object.fromEntries(
          artifacts.map(({ artifact }) => [artifact.target, artifact]),
        ),
      },
    };
    const contents = `${JSON.stringify(manifest, null, 2)}\n`;
    writeFileSync(manifestPath, contents);
    const signaturePath = join(temporaryDirectory, "manifest.json.sig");
    writeFileSync(signaturePath, `${signManifest(contents)}\n`);
    console.log(contents);
    if (options.dryRun)
      return console.log("Dry run complete; Cloudflare was not changed.");

    for (const artifact of artifacts) {
      upload(
        options.bucket,
        artifact.key,
        artifact.binaryPath,
        "application/octet-stream",
        "public, max-age=31536000, immutable",
      );
    }
    for (const key of [`${manifestKey}.sig`, "acp/latest.json.sig"]) {
      upload(
        options.bucket,
        key,
        signaturePath,
        "text/plain; charset=utf-8",
        "public, max-age=60, must-revalidate",
      );
    }
    for (const key of [manifestKey, "acp/latest.json"]) {
      upload(
        options.bucket,
        key,
        manifestPath,
        "application/json",
        "public, max-age=60, must-revalidate",
      );
    }
    if (!options.skipDeploy)
      run(["bunx", "wrangler", "deploy", "--config", wranglerConfig], {
        cwd: registryDirectory,
      });
    console.log(
      `Amarcode ACP ${options.version} publication completed successfully.`,
    );
  } finally {
    rmSync(temporaryDirectory, { recursive: true, force: true });
  }
}

if (import.meta.main)
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  });
