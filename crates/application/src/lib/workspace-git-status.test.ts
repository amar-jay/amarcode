import { expect, test } from "vitest";
import type { WorkspaceChange } from "@/types";
import {
  changedDirectoryStatuses,
  fileGitStatus,
  gitStatusClass,
} from "./workspace-git-status";

function change(path: string, status: string): WorkspaceChange {
  return { path, status, staged: false, unstaged: true };
}

test("marks every ancestor, including deleted and deeply nested files", () => {
  const directories = changedDirectoryStatuses([
    change("src/nested/deep/file.rs", "M"),
    change("removed/old.rs", "D"),
    change("README.md", "M"),
  ]);
  expect([...directories]).toEqual([
    ["src", "M"],
    ["src/nested", "M"],
    ["src/nested/deep", "M"],
    ["removed", "D"],
  ]);
  expect(directories.has("README.md")).toBe(false);
  expect(directories.has("src-other")).toBe(false);
});

test("mixed folder statuses are order independent and new-only folders stay green", () => {
  const changes = [
    change("src/new.ts", "A"),
    change("src/old.ts", "D"),
    change("new/file.ts", "A"),
  ];
  const forward = changedDirectoryStatuses(changes);
  const reverse = changedDirectoryStatuses([...changes].reverse());
  expect(forward.get("src")).toBe("M");
  expect(reverse.get("src")).toBe("M");
  expect(gitStatusClass(forward.get("new"))).toBe(gitStatusClass("A"));
  expect(gitStatusClass(undefined)).toBeUndefined();
});

test("directory-level Git entries color descendants without affecting siblings", () => {
  const changes = new Map([
    ["new/", change("new/", "A")],
    ["new/nested/file.ts", change("new/nested/file.ts", "M")],
  ]);
  expect(fileGitStatus("new/other.ts", changes)).toBe("A");
  expect(fileGitStatus("new/nested/file.ts", changes)).toBe("M");
  expect(fileGitStatus("new-other/file.ts", changes)).toBeUndefined();
  expect(changedDirectoryStatuses(changes.values()).get("new")).toBe("M");
});
