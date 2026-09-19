import { existsSync } from "node:fs";
import { join } from "node:path";
import { hostTarget, projectRoot, run } from "./commands";
import { hashFile, type Artifact } from "./manifest";
import type { BinaryProduct } from "./products";

const requiredLifecycleCommands = ["install", "start", "restart", "status"];

export type BuiltArtifact = {
  artifact: Artifact;
  binaryPath: string;
  key: string;
};

export function verifyLifecycleCli(binaryPath: string) {
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

export function buildProduct(
  product: BinaryProduct,
  version: string,
  targets: string[],
  skipBuild: boolean,
): BuiltArtifact[] {
  const currentHost = hostTarget();
  return targets.map((target) => {
    if (!skipBuild) {
      run([
        "cargo",
        "build",
        "--release",
        "--locked",
        "-p",
        product.crate,
        "--bin",
        product.bin,
        "--target",
        target,
      ]);
    }
    const filename = target.includes("windows")
      ? `${product.bin}.exe`
      : product.bin;
    const binaryPath = join(projectRoot, "target", target, "release", filename);
    if (!existsSync(binaryPath)) {
      throw new Error(`${product.label} binary not found at ${binaryPath}`);
    }
    if (product.verifyLifecycleCli && target === currentHost) {
      verifyLifecycleCli(binaryPath);
    }
    const { sha256, size } = hashFile(binaryPath);
    const artifact: Artifact = {
      target,
      url: `${product.urlPrefix}/${version}/${target}`,
      sha256,
      size,
    };
    return {
      artifact,
      binaryPath,
      key: `${product.objectPrefix}/${version}/${target}/${filename}`,
    };
  });
}
