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

import {
  activeSessionAtom,
  startNewChatAtom,
  applyDaemonWorkspaceAtom,
  selectChatAtom,
} from "./navigation";
import {
  sidePanelMaximizedAtom,
  sidePanelOpenAtom,
  sidePanelPinnedAtom,
  workspaceFileOpenRequestAtom,
} from "./ui";
import { workspacePathAtom, daemonWorkspacePathAtom } from "./workspace";
import { defaultWorkspacePathAtom } from "./preferences";
import { chatsAtom } from "./chats";

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

test("daemon workspace seeds new chats without persisting a user preference", () => {
  const store = createStore();
  store.set(defaultWorkspacePathAtom, "");
  store.set(applyDaemonWorkspaceAtom, "/remote/project");
  expect(store.get(workspacePathAtom)).toBe("/remote/project");
  expect(store.get(defaultWorkspacePathAtom)).toBe("");
  store.set(workspacePathAtom, "/temporary");
  store.set(startNewChatAtom);
  expect(store.get(workspacePathAtom)).toBe("/remote/project");
  store.set(defaultWorkspacePathAtom, "/my/preference");
  store.set(startNewChatAtom);
  expect(store.get(workspacePathAtom)).toBe("/my/preference");
});

test("server suggestions preserve open chats and manually selected folders", () => {
  const store = createStore();
  store.set(defaultWorkspacePathAtom, "");
  store.set(workspacePathAtom, "/chosen");
  store.set(applyDaemonWorkspaceAtom, "/remote/project");
  expect(store.get(workspacePathAtom)).toBe("/chosen");
  const chat = {
    id: "old",
    title: "Old",
    workspace_path: "/chosen",
    created_at: "",
    updated_at: "",
    archived_at: null,
  };
  store.set(chatsAtom, [chat]);
  store.set(selectChatAtom, chat.id);
  store.set(applyDaemonWorkspaceAtom, "/another/default");
  expect(store.get(activeSessionAtom)?.chat.workspace_path).toBe("/chosen");
  expect(store.get(workspacePathAtom)).toBe("/chosen");
  store.set(startNewChatAtom);
  expect(store.get(workspacePathAtom)).toBe("/another/default");
});

test("reconnecting refreshes the daemon suggestion and old daemons clear it", () => {
  const store = createStore();
  store.set(defaultWorkspacePathAtom, "");
  store.set(applyDaemonWorkspaceAtom, "/remote/first");
  store.set(applyDaemonWorkspaceAtom, "/remote/second");
  expect(store.get(workspacePathAtom)).toBe("/remote/second");
  store.set(applyDaemonWorkspaceAtom, "");
  expect(store.get(daemonWorkspacePathAtom)).toBe("");
  expect(store.get(workspacePathAtom)).toBe("");
});
