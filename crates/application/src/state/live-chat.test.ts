import { createStore } from "jotai";
import { describe, expect, it, vi } from "vitest";

vi.hoisted(() => {
  const values = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
      removeItem: (key: string) => values.delete(key),
    },
  });
});
import { activeSessionAtom } from "./navigation";
import {
  failStartedPromptAtom,
  liveChatAtom,
  type LiveChatState,
} from "./live-chat";

const chat = {
  id: "chat-1",
  workspace_path: "/workspace",
  title: "Test",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  archived_at: null,
};

describe("failed home-composer prompts", () => {
  it("clears the optimistic flag before the live chat mounts", () => {
    const store = createStore();
    store.set(activeSessionAtom, {
      chat,
      initialRunId: null,
      initialTurnActive: true,
    });

    store.set(failStartedPromptAtom, {
      chatId: chat.id,
      error: "Agent executable was not found",
    });

    expect(store.get(activeSessionAtom)?.initialTurnActive).toBe(false);
  });

  it("ends an already-mounted live turn with the startup error", () => {
    const store = createStore();
    const live: LiveChatState = {
      chatId: chat.id,
      detail: null,
      runId: null,
      runStatus: null,
      turnStatus: "started",
      pendingRequest: null,
      contextRestoration: null,
      sessionMode: "build",
      loading: true,
      error: null,
    };
    store.set(liveChatAtom, live);

    store.set(failStartedPromptAtom, {
      chatId: chat.id,
      error: "ACP connection closed",
    });

    expect(store.get(liveChatAtom)).toMatchObject({
      turnStatus: "failed",
      error: "ACP connection closed",
    });
  });
});
