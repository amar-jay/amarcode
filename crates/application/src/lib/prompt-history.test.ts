import { expect, it } from "vitest";
import { PromptHistory } from "./prompt-history";

it("groups typing and restores selection on undo and redo", () => {
  const h = new PromptHistory("a");
  h.set("ab", { start: 2, end: 2 }, 100);
  h.set("abc", { start: 3, end: 3 }, 200);
  expect(h.undo()).toEqual({ value: "a", start: 1, end: 1 });
  expect(h.redo()?.value).toBe("abc");
});
it("keeps replacements atomic and discards redo after new edits", () => {
  const h = new PromptHistory("draft");
  h.selection(0, 5);
  h.set("replacement");
  expect(h.undo()).toEqual({ value: "draft", start: 0, end: 5 });
  h.set("different");
  expect(h.redo()).toBeNull();
});
it("separates paused typing and cursor moves and bounds history", () => {
  const h = new PromptHistory();
  h.set("a", { start: 1, end: 1 }, 100);
  h.set("ab", { start: 2, end: 2 }, 1000);
  h.selection(0, 0);
  h.set("cab", { start: 1, end: 1 }, 1100);
  expect(h.undo()?.value).toBe("ab");
  expect(h.undo()?.value).toBe("a");
  for (let i = 0; i < 150; i++) h.set(String(i));
  expect(h.past).toHaveLength(100);
});
