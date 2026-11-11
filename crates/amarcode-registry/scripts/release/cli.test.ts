import { describe, expect, test } from "bun:test";
import { parseArgs, interactive, inquireRegistryDeployment } from "./cli";
import { parseGitPorcelain } from "./commands";
import { releaseCommitMessage } from "./products";

describe("parseArgs", () => {
  test("parses positional products and independent versions", () => {
    const options = parseArgs([
      "daemon",
      "acp",
      "--daemon-version",
      "0.6.13",
      "--acp-version",
      "0.1.0",
      "--target",
      "x86_64-unknown-linux-gnu",
      "--non-interactive",
    ]);
    expect(options.products).toEqual(["daemon", "acp"]);
    expect(options.versions.daemon).toBe("0.6.13");
    expect(options.versions.acp).toBe("0.1.0");
    expect(options.targets).toEqual(["x86_64-unknown-linux-gnu"]);
    expect(options.nonInteractive).toBe(true);
  });

  test("expands all", () => {
    const options = parseArgs(["--products", "all"]);
    expect(options.products).toEqual(["daemon", "acp", "app"]);
  });

  test("rejects shared --version for multiple binary products", () => {
    expect(() => parseArgs(["daemon", "acp", "--version", "1.0.0"])).toThrow(
      /--version applies to a single binary product/,
    );
  });

  test("maps --version onto a preset product", () => {
    const options = parseArgs(["--version", "0.6.13"], ["daemon"]);
    expect(options.products).toEqual(["daemon"]);
    expect(options.versions.daemon).toBe("0.6.13");
    expect(options.versionsProvided.daemon).toBe(true);
  });
});

describe("version release commit", () => {
  test("parses root and nested porcelain paths", () => {
    expect(
      parseGitPorcelain(
        " M Cargo.lock\nM  crates/daemon/Cargo.toml\nR  old.toml -> crates/amarcode-acp/Cargo.toml",
      ),
    ).toEqual([
      "Cargo.lock",
      "crates/daemon/Cargo.toml",
      "crates/amarcode-acp/Cargo.toml",
    ]);
  });

  test("writes a chore release message", () => {
    expect(
      releaseCommitMessage(["daemon", "acp"], {
        daemon: "0.6.13",
        acp: "0.1.1",
      }),
    ).toBe("chore: release amarcode-daemon 0.6.13, amarcode-acp 0.1.1");
  });
});

describe("registry deployment decision", () => {
  test("registry remains outside the product list", () => {
    expect(() => parseArgs(["registry"])).toThrow(/unknown product/);
    expect(parseArgs(["all"]).products).toEqual(["daemon", "acp", "app"]);
  });

  test("requires a deployment decision without a terminal regardless of Git changes", async () => {
    await expect(
      interactive(
        parseArgs(["acp", "--version", "1.2.3", "--non-interactive"]),
      ),
    ).rejects.toThrow(/--deploy-registry or --skip-deploy/);
  });

  test("non-interactive flags supply either answer", async () => {
    for (const [flag, expected] of [
      ["--deploy-registry", true],
      ["--skip-deploy", false],
    ] as const) {
      const plan = await interactive(
        parseArgs(["acp", "--version", "1.2.3", "--non-interactive", flag]),
      );
      expect(plan?.deployRegistry).toBe(expected);
    }
    expect(() => parseArgs(["--deploy-registry", "--skip-deploy"])).toThrow(
      /cannot be combined/,
    );
  });

  test("always asks yes/no unless a flag supplies the answer", async () => {
    const options = { skipDeploy: false, deployRegistry: false };
    for (const answer of [true, false]) {
      let calls = 0;
      const confirm = async () => {
        calls++;
        return answer;
      };
      expect(await inquireRegistryDeployment(options, confirm)).toBe(answer);
      expect(calls).toBe(1);
      expect(
        await inquireRegistryDeployment(
          { ...options, skipDeploy: true },
          confirm,
        ),
      ).toBe(false);
      expect(calls).toBe(1);
      expect(
        await inquireRegistryDeployment(
          { ...options, deployRegistry: true },
          confirm,
        ),
      ).toBe(true);
      expect(calls).toBe(1);
    }
  });
});
