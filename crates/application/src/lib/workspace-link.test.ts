import { describe, expect, test } from "vitest";
import {
  looksLikeWorkspaceFileTarget,
  workspaceFileTarget,
} from "./workspace-link";

const root = "/home/person/code/amarcode";

describe("workspaceFileTarget", () => {
  test("resolves relative paths with line and column", () => {
    expect(workspaceFileTarget("crates/app/src/main.rs:42:7", root)).toEqual({
      path: "crates/app/src/main.rs",
      line: 42,
      column: 7,
    });
  });

  test("resolves absolute and file URLs inside the workspace", () => {
    expect(workspaceFileTarget(`${root}/README.md#L8`, root)).toEqual({
      path: "README.md",
      line: 8,
      column: undefined,
    });
    expect(
      workspaceFileTarget(`file://${root}/crates/app.ts#L3C2`, root),
    ).toEqual({ path: "crates/app.ts", line: 3, column: 2 });
  });

  test("rejects traversal and absolute paths outside the workspace", () => {
    expect(workspaceFileTarget("../secret.txt", root)).toBeNull();
    expect(workspaceFileTarget("/tmp/secret.txt", root)).toBeNull();
  });

  test("does not classify web, mail, telephone, or fragment links as files", () => {
    for (const link of [
      "https://example.com/a",
      "http://example.com",
      "mailto:user@example.com",
      "tel:+123",
      "#heading",
    ]) {
      expect(looksLikeWorkspaceFileTarget(link)).toBe(false);
    }
  });
});
