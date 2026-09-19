import {
  createHash,
  createPrivateKey,
  createPublicKey,
  sign,
} from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { cargoLockPath, projectRoot } from "./commands";
import type { BinaryProduct } from "./products";

export type Artifact = {
  target: string;
  url: string;
  sha256: string;
  size: number;
};

export type Manifest = {
  version: string;
  protocolVersion?: number;
  publishedAt: string;
  sourceCommit: string;
  sourceDirty?: boolean;
  artifacts: Record<string, Artifact>;
};

export const releasePublicKey =
  "5ef56cd7772e8c601ca9c5a15378b7088fc558e7edcde73770cbb116d9e255d2";

export function cargoValue(manifestPath: string, key: string): string {
  const contents = readFileSync(manifestPath, "utf8");
  const match = contents.match(new RegExp(`^${key}\\s*=\\s*"([^"]+)"`, "m"));
  if (!match) throw new Error(`could not find ${key} in ${manifestPath}`);
  return match[1];
}

export function packageVersion(product: BinaryProduct): string {
  return cargoValue(product.cargoToml, "version");
}

export function setPackageVersion(product: BinaryProduct, version: string) {
  const current = packageVersion(product);
  if (current === version) return;
  const manifest = readFileSync(product.cargoToml, "utf8");
  const updatedManifest = manifest.replace(
    /^version\s*=\s*"[^"]+"/m,
    `version = "${version}"`,
  );
  const lock = readFileSync(cargoLockPath, "utf8");
  const updatedLock = lock.replace(
    new RegExp(
      `(?<=\\[\\[package\\]\\]\\nname = "${product.cargoLockName}"\\nversion = ")[^"]+(?=")`,
    ),
    version,
  );
  if (updatedManifest === manifest || updatedLock === lock) {
    throw new Error(`could not update the ${product.label} package version`);
  }
  writeFileSync(product.cargoToml, updatedManifest);
  writeFileSync(cargoLockPath, updatedLock);
  console.log(`Updated ${product.label} version: ${current} -> ${version}`);
}

export function protocolVersion(): number {
  const contents = readFileSync(
    join(projectRoot, "crates", "protocol", "src", "lib.rs"),
    "utf8",
  );
  const match = contents.match(
    /pub const PROTOCOL_VERSION:\s*u32\s*=\s*(\d+)\s*;/,
  );
  if (!match) throw new Error("could not read PROTOCOL_VERSION");
  return Number(match[1]);
}

export function bump(
  version: string,
  kind: "major" | "minor" | "patch",
): string {
  const match = version.match(/^(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z.-]+)?$/);
  if (!match)
    throw new Error(`cannot ${kind}-bump non-semver version ${version}`);
  const [major, minor, patch] = match.slice(1).map(Number);
  if (kind === "major") return `${major + 1}.0.0`;
  if (kind === "minor") return `${major}.${minor + 1}.0`;
  return `${major}.${minor}.${patch + 1}`;
}

export function hashFile(path: string): { sha256: string; size: number } {
  const bytes = readFileSync(path);
  return {
    sha256: createHash("sha256").update(bytes).digest("hex"),
    size: bytes.byteLength,
  };
}

export function signManifest(contents: string): string {
  const keyPath =
    process.env.AMARCODE_ACP_SIGNING_KEY ??
    process.env.AMARCODE_DAEMON_SIGNING_KEY ??
    join(homedir(), ".config", "amarcode", "daemon-release-signing-key.pem");
  if (!existsSync(keyPath)) {
    throw new Error(`release signing key not found at ${keyPath}`);
  }
  const privateKey = createPrivateKey(readFileSync(keyPath));
  if (privateKey.asymmetricKeyType !== "ed25519") {
    throw new Error("release signing key must be Ed25519");
  }
  const publicDer = createPublicKey(privateKey).export({
    format: "der",
    type: "spki",
  });
  const actual = publicDer.subarray(publicDer.byteLength - 32).toString("hex");
  if (actual !== releasePublicKey) {
    throw new Error(`release signing key has unexpected public key ${actual}`);
  }
  return sign(null, Buffer.from(contents), privateKey).toString("base64");
}
