import { describe, expect, test } from "bun:test";
import { parseArgs } from "./cli";
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
    expect(() =>
      parseArgs(["daemon", "acp", "--version", "1.0.0"]),
    ).toThrow(/--version applies to a single binary product/);
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
