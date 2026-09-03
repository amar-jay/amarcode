import { describe, expect, it } from "vitest";
import { automaticApprovalResult, shouldAutoApprove } from "./permission-mode";

const command = {
  toolCall: { kind: "execute", rawInput: { command: "bun", args: ["test"] } },
  options: [
    { optionId: "once", kind: "allow_once" },
    { optionId: "always", kind: "allow_always" },
  ],
};

describe("permission modes", () => {
  it("only full agentic approves commands", () => {
    expect(shouldAutoApprove("confirm", command)).toBe(false);
    expect(shouldAutoApprove("auto-edit", command)).toBe(false);
    expect(shouldAutoApprove("full-agentic", command)).toBe(true);
  });

  it("auto-edit approves file edits", () => {
    expect(
      shouldAutoApprove("auto-edit", {
        toolCall: { kind: "edit", rawInput: { path: "src/app.ts" } },
      }),
    ).toBe(true);
  });

  it("prefers an allow-always option", () => {
    expect(automaticApprovalResult(command)).toEqual({
      outcome: { outcome: "selected", optionId: "always" },
    });
  });
});
