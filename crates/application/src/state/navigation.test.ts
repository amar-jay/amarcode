import { createStore } from "jotai";
import { expect, test, vi } from "vitest";

vi.mock("./preferences", async () => {
  const { atom } = await import("jotai");
  return {
    defaultWorkspacePathAtom: atom("/default"),
    defaultAgentIdAtom: atom(""),
  };
});
vi.mock("@/api", () => ({ daemonApi: {} }));

import { activeSessionAtom, startNewChatAtom } from "./navigation";
import {
  sidePanelMaximizedAtom,
  sidePanelOpenAtom,
  sidePanelPinnedAtom,
  workspaceFileOpenRequestAtom,
} from "./ui";
import { workspacePathAtom } from "./workspace";

test("new chat closes the workspace and clears focus mode while retaining pin preference", () => {
  const store = createStore();
  store.set(activeSessionAtom, {
    chat: {
      id: "chat",
      title: "Chat",
      workspace_path: "/old",
      created_at: "",
      updated_at: "",
      archived_at: null,
    },
    initialRunId: null,
  });
  store.set(workspacePathAtom, "/old");
  store.set(sidePanelOpenAtom, true);
  store.set(sidePanelMaximizedAtom, true);
  store.set(sidePanelPinnedAtom, true);
  store.set(workspaceFileOpenRequestAtom, { path: "old.rs", line: 5 });

  store.set(startNewChatAtom);

  expect(store.get(activeSessionAtom)).toBeNull();
  expect(store.get(sidePanelOpenAtom)).toBe(false);
  expect(store.get(sidePanelMaximizedAtom)).toBe(false);
  expect(store.get(sidePanelPinnedAtom)).toBe(true);
  expect(store.get(workspaceFileOpenRequestAtom)).toBeNull();
  expect(store.get(workspacePathAtom)).toBe("/default");
});
