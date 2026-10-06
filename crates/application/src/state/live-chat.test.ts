import { createStore } from "jotai";
import { beforeEach, describe, expect, it, vi } from "vitest";

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

const notifyMock = vi.fn();
vi.mock("@/lib/notify", () => ({
  notify: (...args: unknown[]) => notifyMock(...args),
  notifyToast: vi.fn(),
  notifyAttention: vi.fn(),
}));

vi.mock("@/api", () => ({
  daemonApi: {
    getChat: vi.fn(async () => ({
      chat: {
        id: "chat-1",
        workspace_path: "/workspace",
        title: "Test",
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
        archived_at: null,
      },
      messages: [],
      session_config: [],
      context_usage: null,
    })),
    setSessionConfigOption: vi.fn(),
    prompt: vi.fn(),
    respondPermission: vi.fn(),
  },
}));

import { activeSessionAtom } from "./navigation";
import {
  applyLiveChatEventAtom,
  failStartedPromptAtom,
  liveChatAtom,
  setLiveSessionConfigOptionAtom,
  type LiveChatState,
} from "./live-chat";

beforeEach(() => {
  notifyMock.mockClear();
});

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
      contextUsage: null,
      sessionConfig: [],
      sessionConfigAgentId: null,
      loading: true,
      error: null,
      errorKind: null,
      authRequired: null,
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

describe("agent failure notifications", () => {
  it("keeps the failure banner and toasts the specific error", () => {
    const store = createStore();
    store.set(liveChatAtom, {
      chatId: chat.id,
      detail: null,
      runId: "run-1",
      runStatus: "running",
      turnStatus: "started",
      pendingRequest: null,
      contextRestoration: null,
      contextUsage: null,
      sessionConfig: [],
      sessionConfigAgentId: null,
      loading: false,
      error: null,
      errorKind: null,
      authRequired: null,
    });

    store.set(applyLiveChatEventAtom, {
      type: "turnUpdated",
      payload: {
        chat_id: chat.id,
        run_id: "run-1",
        user_message_id: "msg-1",
        status: "failed",
        stop_reason: null,
        error_message:
          "Internal error: provider returned 401 Unauthorized: API key expired.",
        error_kind: "error",
      },
    });

    expect(store.get(liveChatAtom)).toMatchObject({
      turnStatus: "failed",
      error:
        "Internal error: provider returned 401 Unauthorized: API key expired.",
      errorKind: "error",
    });
    expect(notifyMock).toHaveBeenCalledWith(
      "Internal error: provider returned 401 Unauthorized: API key expired.",
      "error",
      expect.objectContaining({
        id: "agent-failure:Internal error: provider returned 401 Unauthorized: API key expired.",
        duration: 12_000,
      }),
    );
  });
});

describe("ACP context usage", () => {
  it("applies live usage updates to the open chat", () => {
    const store = createStore();
    store.set(liveChatAtom, {
      chatId: chat.id,
      detail: null,
      runId: "run-1",
      runStatus: "running",
      turnStatus: "started",
      pendingRequest: null,
      contextRestoration: null,
      contextUsage: null,
      sessionConfig: [],
      sessionConfigAgentId: null,
      loading: false,
      error: null,
      errorKind: null,
      authRequired: null,
    });

    store.set(applyLiveChatEventAtom, {
      type: "contextUsageUpdated",
      payload: {
        chat_id: chat.id,
        run_id: "run-1",
        usage: {
          used: 53_000,
          size: 200_000,
          cost: { amount: 0.42, currency: "USD" },
        },
      },
    });

    expect(store.get(liveChatAtom)?.contextUsage).toEqual({
      used: 53_000,
      size: 200_000,
      cost: { amount: 0.42, currency: "USD" },
    });
  });
});

describe("agent-owned session configuration", () => {
  it("does not modify the previous agent's live options after switching agents", async () => {
    const store = createStore();
    store.set(activeSessionAtom, {
      chat,
      agent: {
        id: "agent-2",
        name: "Agent 2",
        icon: null,
        command: "agent-2",
        arguments: [],
        environment: [],
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
        available: true,
        resolved_command: "agent-2",
        unavailable_reason: null,
      },
      initialRunId: null,
      initialTurnActive: false,
    });
    store.set(liveChatAtom, {
      chatId: chat.id,
      detail: null,
      runId: "run-1",
      runStatus: "running",
      turnStatus: "completed",
      pendingRequest: null,
      contextRestoration: null,
      contextUsage: null,
      sessionConfig: [
        {
          id: "model",
          name: "Previous model",
          description: null,
          category: null,
          type: "select",
          current_value: "old-model",
          options: [
            { value: "old-model", name: "Old model", description: null },
          ],
        },
      ],
      sessionConfigAgentId: "agent-1",
      loading: false,
      error: null,
      errorKind: null,
      authRequired: null,
    });

    await store.set(setLiveSessionConfigOptionAtom, {
      configId: "model",
      value: { type: "id", value: "new-model" },
    });

    expect(store.get(liveChatAtom)?.sessionConfig[0].current_value).toBe(
      "old-model",
    );
    expect(store.get(liveChatAtom)?.sessionConfigAgentId).toBe("agent-1");
  });
});
