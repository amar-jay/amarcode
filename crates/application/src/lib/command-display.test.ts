import { describe, expect, it } from "vitest";
import { exactCommand } from "./command-display";

describe("exactCommand", () => {
  it("shows the executable and every argument", () => {
    expect(exactCommand("bash", ["-lc", "wc -l src/main.rs"])).toBe(
      "bash -lc 'wc -l src/main.rs'",
    );
  });

  it("preserves argument boundaries and quotes", () => {
    expect(exactCommand("printf", ["it's", "two words", ""])).toBe(
      `printf 'it'"'"'s' 'two words' ''`,
    );
  });
});
