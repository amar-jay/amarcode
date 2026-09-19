#!/usr/bin/env bun

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { buildProduct, type BuiltArtifact } from "./release/build";
import {
  gitCommit,
  gitPorcelainPaths,
  gitStatusShort,
  refreshCargoLockfile,
  registryDirectory,
  run,
  validateSegment,
  wranglerConfig,
} from "./release/commands";
import {
  interactive,
  parseArgs,
  type AppPublicationPlan,
  type ReleasePlan,
} from "./release/cli";
import {
  packageVersion,
  protocolVersion,
  setPackageVersion,
  signManifest,
  type Manifest,
} from "./release/manifest";
import {
  binaryProductIds,
  binaryProducts,
  releaseCommitMessage,
  type BinaryProductId,
} from "./release/products";
import { loadRemoteManifest, upload } from "./release/r2";

export async function publish(args: string[]) {
  const selected = await interactive(parseArgs(args));
  if (!selected) return;
  await execute(selected);
}

function commitWorkingTree(message: string) {
  run(["git", "add", "-A"]);
  const staged = run(["git", "diff", "--cached", "--name-only"], {
    quiet: true,
    allowFailure: true,
  })
    .stdout.toString()
    .trim();
  if (!staged) return;
  run(["git", "commit", "-m", message]);
}

function executeAppPublication(plan: AppPublicationPlan) {
  const changes = gitStatusShort();
  if (changes) {
    throw new Error(
      `uncommitted changes remain before desktop publication:\n${changes}`,
    );
  }
  if (plan.push) {
    run(["git", "push", "origin", "main"]);
    run(["bun", "run", "app:publish"]);
  }
}

async function execute(plan: ReleasePlan) {
  const binaryIds = plan.products.filter(
    (id): id is BinaryProductId => id !== "app",
  );
  for (const target of plan.targets) validateSegment("target", target);
  for (const id of binaryIds) validateSegment("version", plan.versions[id]);

  for (const id of binaryIds) {
    const product = binaryProducts[id];
    const current = packageVersion(product);
    if (plan.versions[id] !== current) {
      if (plan.skipBuild) {
        throw new Error(
          `cannot change ${product.label} version with --skip-build`,
        );
      }
      setPackageVersion(product, plan.versions[id]);
    }
  }

  if (binaryIds.length > 0 && !plan.skipBuild) {
    refreshCargoLockfile();
  }

  const built: Record<BinaryProductId, BuiltArtifact[]> = {
    daemon: [],
    acp: [],
  };
  for (const id of binaryIds) {
    built[id] = buildProduct(
      binaryProducts[id],
      plan.versions[id],
      plan.targets,
      plan.skipBuild,
    );
  }

  if (!plan.dryRun) {
    const versionCommitIds =
      binaryIds.length > 0 ? binaryIds : binaryProductIds;
    commitWorkingTree(
      plan.appPublication?.commitMessage ??
        releaseCommitMessage(versionCommitIds, plan.versions),
    );
  }

  const sourceCommit = gitCommit();
  const remainingDirty = gitPorcelainPaths();
  if (!plan.dryRun && remainingDirty.length > 0 && !plan.allowDirty) {
    throw new Error(
      `worktree still dirty after git add -A (${remainingDirty.join(", ")}); refusing to publish a dirty source snapshot`,
    );
  }
  const dirty = !plan.dryRun && remainingDirty.length > 0;
  if (dirty) {
    console.warn(
      `Publishing from a dirty worktree (${remainingDirty.join(", ")}); manifests will record commit ${sourceCommit}.`,
    );
  }

  const temporaryDirectory = mkdtempSync(
    join(tmpdir(), "amarcode-registry-publish-"),
  );
  try {
    const prepared = binaryIds.map((id) => {
      const product = binaryProducts[id];
      const version = plan.versions[id];
      const artifacts = built[id];
      const versionManifestPath = join(
        temporaryDirectory,
        `${id}-manifest.json`,
      );
      const versionManifestKey = `${product.objectPrefix}/${version}/manifest.json`;
      const existing = plan.dryRun
        ? null
        : loadRemoteManifest(
            plan.bucket,
            versionManifestKey,
            versionManifestPath,
          );
      if (existing && !plan.overwrite) {
        throw new Error(
          `${product.label} version ${version} already exists; use --overwrite intentionally`,
        );
      }
      const manifest: Manifest = {
        version,
        ...(product.includeProtocolVersion
          ? { protocolVersion: protocolVersion() }
          : {}),
        publishedAt: new Date().toISOString(),
        sourceCommit,
        ...(dirty ? { sourceDirty: true } : {}),
        artifacts: {
          ...(existing?.version === version ? existing.artifacts : {}),
          ...Object.fromEntries(
            artifacts.map(({ artifact }) => [artifact.target, artifact]),
          ),
        },
      };
      const contents = `${JSON.stringify(manifest, null, 2)}\n`;
      writeFileSync(versionManifestPath, contents);
      const signaturePath = join(temporaryDirectory, `${id}-manifest.json.sig`);
      writeFileSync(signaturePath, `${signManifest(contents)}\n`);
      console.log(`\n${product.label} ${version}`);
      for (const { artifact, binaryPath } of artifacts) {
        console.log(`  ${artifact.target}`);
        console.log(`    binary: ${binaryPath}`);
        console.log(`    size:   ${artifact.size} bytes`);
        console.log(`    sha256: ${artifact.sha256}`);
      }
      console.log(contents);
      return {
        id,
        product,
        artifacts,
        versionManifestPath,
        versionManifestKey,
        signaturePath,
      };
    });

    if (plan.dryRun) {
      console.log("\nDry run complete; no Cloudflare resources were changed.");
      return;
    }

    for (const item of prepared) {
      for (const { key, binaryPath } of item.artifacts) {
        upload(
          plan.bucket,
          key,
          binaryPath,
          "application/octet-stream",
          "public, max-age=31536000, immutable",
        );
      }
    }
    for (const item of prepared) {
      upload(
        plan.bucket,
        `${item.versionManifestKey}.sig`,
        item.signaturePath,
        "text/plain; charset=utf-8",
        "public, max-age=60, must-revalidate",
      );
      upload(
        plan.bucket,
        item.versionManifestKey,
        item.versionManifestPath,
        "application/json",
        "public, max-age=60, must-revalidate",
      );
    }
    for (const item of prepared) {
      upload(
        plan.bucket,
        `${item.product.objectPrefix}/latest.json.sig`,
        item.signaturePath,
        "text/plain; charset=utf-8",
        "public, max-age=60, must-revalidate",
      );
      upload(
        plan.bucket,
        `${item.product.objectPrefix}/latest.json`,
        item.versionManifestPath,
        "application/json",
        "public, max-age=60, must-revalidate",
      );
    }

    if (binaryIds.length > 0 && !plan.skipDeploy) {
      run(["bunx", "wrangler", "deploy", "--config", wranglerConfig], {
        cwd: registryDirectory,
      });
    }
  } finally {
    rmSync(temporaryDirectory, { recursive: true, force: true });
  }

  if (binaryIds.length > 0) {
    console.log("\nBinary publication completed successfully.");
  }
  if (plan.appPublication) executeAppPublication(plan.appPublication);
}

if (import.meta.main) {
  publish(process.argv.slice(2)).catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  });
}
