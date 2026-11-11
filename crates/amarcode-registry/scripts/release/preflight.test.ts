import { describe, expect, test } from "bun:test";
import {
  mkdtempSync,
  writeFileSync,
  rmSync,
  chmodSync,
  readFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { projectRoot } from "./commands";
import { requireAuthenticatedWrangler } from "./preflight";

describe("Wrangler authentication preflight", () => {
  test("requires affirmative machine-readable authentication", () => {
    expect(() =>
      requireAuthenticatedWrangler('{"loggedIn":true}'),
    ).not.toThrow();
    for (const value of [
      '{"loggedIn":false}',
      "{}",
      "null",
      "You are not authenticated.",
      "",
    ]) {
      expect(() => requireAuthenticatedWrangler(value)).toThrow(
        /authentication|authenticated/,
      );
    }
  });

  test("authentication failure aborts before the menu or version updates", () => {
    const directory = mkdtempSync(
      join(tmpdir(), "amarcode-publish-preflight-"),
    );
    const manifest = join(projectRoot, "crates/amarcode-acp/Cargo.toml");
    const before = readFileSync(manifest, "utf8");
    try {
      const executable = join(directory, "bunx");
      writeFileSync(
        executable,
        "#!/bin/sh\nprintf '{\"loggedIn\":false}\\n'\n",
      );
      chmodSync(executable, 0o755);
      const result = Bun.spawnSync(
        [
          process.execPath,
          "crates/amarcode-registry/scripts/publish.ts",
          "acp",
          "--version",
          "99.0.0",
          "--non-interactive",
        ],
        {
          cwd: projectRoot,
          env: { ...process.env, PATH: `${directory}:${process.env.PATH}` },
          stdout: "pipe",
          stderr: "pipe",
        },
      );
      expect(result.exitCode).not.toBe(0);
      expect(result.stderr.toString()).toContain(
        "Wrangler is not authenticated",
      );
      expect(result.stdout.toString()).not.toContain("cargo");
      expect(readFileSync(manifest, "utf8")).toBe(before);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
  test("authenticated dry run prepares the release without deployment", () => {
    const directory = mkdtempSync(join(tmpdir(), "amarcode-publish-dry-run-"));
    try {
      const executable = join(directory, "bunx");
      writeFileSync(
        executable,
        '#!/bin/sh\nif [ "$1" != "wrangler" ] || [ "$2" != "whoami" ]; then exit 77; fi\nprintf \'{"loggedIn":true}\\n\'\n',
      );
      chmodSync(executable, 0o755);
      const result = Bun.spawnSync(
        [
          process.execPath,
          "crates/amarcode-registry/scripts/publish.ts",
          "app",
          "--push",
          "--dry-run",
          "--non-interactive",
          "--skip-deploy",
        ],
        {
          cwd: projectRoot,
          env: { ...process.env, PATH: `${directory}:${process.env.PATH}` },
          stdout: "pipe",
          stderr: "pipe",
        },
      );
      expect(result.exitCode).toBe(0);
      // A successful check is silent; only failures report the fix.
      expect(result.stdout.toString()).not.toContain("Cloudflare authentication");
      expect(result.stdout.toString()).toContain("Dry run complete");
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
});
