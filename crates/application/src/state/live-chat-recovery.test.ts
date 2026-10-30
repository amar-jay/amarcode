import { createStore } from "jotai";
import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import type { ChatSessionSnapshot, ChatDetail, PromptResultDto } from "@/types";

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
vi.mock("@/lib/notify", () => ({
  notify: vi.fn(),
  notifyToast: vi.fn(),
  notifyAttention: vi.fn(),
}));
vi.mock("@/api", () => ({
  daemonApi: {
    getChat: vi.fn(),
    respondPermission: vi.fn(),
    respondInput: vi.fn(),
    prompt: vi.fn(),
    listChats: vi.fn(async () => []),
  },
}));
import { daemonApi } from "@/api";
import { permissionModeAtom } from "./preferences";
import {
  daemonConnectionGenerationAtom,
  daemonEventStreamStateAtom,
  sessionSnapshotsAtom,
} from "./daemon-events";
import {
  applySessionSnapshotAtom,
  applyLiveChatEventAtom,
  liveChatAtom,
  liveChatIsWorkingAtom,
  loadLiveChatAtom,
  openLiveChatAtom,
  respondLiveRequestAtom,
  submitLivePromptAtom,
} from "./live-chat";

function snapshot(
  patch: Partial<ChatSessionSnapshot> = {},
): ChatSessionSnapshot {
  return {
    daemon_instance_id: "daemon-1",
    chat_id: "chat-1",
    revision: 1,
    run_id: "run-1",
    agent_id: "agent-1",
    run_status: "running",
    turn: {
      user_message_id: "user-1",
      status: "started",
      stop_reason: null,
      error_message: null,
      error_kind: null,
    },
    pending_requests: [],
    error_message: null,
    error_kind: null,
    context_restoration: null,
    ...patch,
  };
}
function detail(state: ChatSessionSnapshot): ChatDetail {
  return {
    chat: {
      id: state.chat_id,
      workspace_path: "/workspace",
      title: "Test",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
      archived_at: null,
    },
    messages: [],
    session_config: [],
    session_commands: null,
    context_usage: null,
    session_state: state,
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}
function setup() {
  const store = createStore();
  store.set(permissionModeAtom, "confirm");
  store.set(openLiveChatAtom, "chat-1");
  return store;
}
beforeEach(() => vi.clearAllMocks());
afterEach(() => vi.useRealTimers());

describe("authoritative session recovery", () => {
  it("restores active work and every pending approval/input after a renderer reload", async () => {
    const store = setup();
    const state = snapshot({
      pending_requests: [
        {
          request_id: "approval-1",
          kind: "approval",
          details: { toolCall: { title: "Run command" } },
        },
        {
          request_id: "question-1",
          kind: "input",
          details: { message: "Which file?" },
        },
      ],
    });
    vi.mocked(daemonApi.getChat).mockResolvedValueOnce(detail(state));
    await store.set(loadLiveChatAtom, "chat-1");
    expect(store.get(liveChatIsWorkingAtom)).toBe(true);
    expect(store.get(liveChatAtom)?.pendingRequests).toHaveLength(2);
    expect(store.get(liveChatAtom)?.pendingRequest?.requestId).toBe(
      "approval-1",
    );
    store.set(applySessionSnapshotAtom, {
      ...state,
      revision: 2,
      pending_requests: state.pending_requests.slice(1),
    });
    expect(store.get(liveChatAtom)?.pendingRequest?.requestId).toBe(
      "question-1",
    );
  });

  it.each(["completed", "failed", "cancelled"] as const)(
    "recovers a turn that %s while disconnected",
    async (status) => {
      const store = setup();
      store.set(applySessionSnapshotAtom, snapshot());
      const state = snapshot({
        revision: 2,
        turn: {
          user_message_id: "user-1",
          status,
          stop_reason: null,
          error_message: status === "failed" ? "Agent failed" : null,
          error_kind: status === "failed" ? "error" : null,
        },
        error_message: status === "failed" ? "Agent failed" : null,
        error_kind: status === "failed" ? "error" : null,
      });
      vi.mocked(daemonApi.getChat).mockResolvedValueOnce(detail(state));
      await store.set(loadLiveChatAtom, "chat-1");
      expect(store.get(liveChatIsWorkingAtom)).toBe(false);
      expect(store.get(liveChatAtom)?.turnStatus).toBe(status);
      expect(store.get(liveChatAtom)?.pendingRequest).toBeNull();
    },
  );

  it("does not resurrect an approval when a slower snapshot follows its resolution", async () => {
    const store = setup();
    const old = snapshot({
      pending_requests: [
        { request_id: "approval-1", kind: "approval", details: {} },
      ],
    });
    const response = deferred<ChatDetail>();
    vi.mocked(daemonApi.getChat).mockReturnValueOnce(response.promise);
    const loading = store.set(loadLiveChatAtom, "chat-1");
    store.set(applySessionSnapshotAtom, {
      ...old,
      revision: 3,
      pending_requests: [],
    });
    response.resolve(detail(old));
    await loading;
    expect(store.get(liveChatAtom)?.sessionState?.revision).toBe(3);
    expect(store.get(liveChatAtom)?.pendingRequest).toBeNull();
    store.set(applyLiveChatEventAtom, {
      type: "approvalRequired",
      payload: { run_id: "run-1", request_id: "approval-1", details: {} },
    });
    expect(store.get(liveChatAtom)?.pendingRequest).toBeNull();
  });

  it("discards out-of-order detail responses for the same chat", async () => {
    const store = setup();
    const old = deferred<ChatDetail>();
    vi.mocked(daemonApi.getChat)
      .mockReturnValueOnce(old.promise)
      .mockResolvedValueOnce(detail(snapshot({ revision: 3 })));
    const first = store.set(loadLiveChatAtom, "chat-1");
    await store.set(loadLiveChatAtom, "chat-1");
    const stale = detail(snapshot());
    stale.chat.title = "stale";
    old.resolve(stale);
    await first;
    expect(store.get(liveChatAtom)?.detail?.chat.title).toBe("Test");
    expect(store.get(liveChatAtom)?.sessionState?.revision).toBe(3);
  });

  it("ignores responses from a previous connection and clears old state on daemon restart", async () => {
    const store = setup();
    store.set(
      applySessionSnapshotAtom,
      snapshot({
        pending_requests: [
          { request_id: "old", kind: "approval", details: {} },
        ],
      }),
    );
    const old = deferred<ChatDetail>();
    vi.mocked(daemonApi.getChat).mockReturnValueOnce(old.promise);
    const loading = store.set(loadLiveChatAtom, "chat-1");
    store.set(daemonConnectionGenerationAtom, 1);
    const fresh = snapshot({
      daemon_instance_id: "daemon-2",
      revision: 0,
      run_id: null,
      agent_id: null,
      run_status: null,
      turn: null,
    });
    store.set(applySessionSnapshotAtom, fresh);
    old.resolve(detail(snapshot({ revision: 100 })));
    await loading;
    expect(store.get(liveChatAtom)?.sessionState?.daemon_instance_id).toBe(
      "daemon-2",
    );
    expect(store.get(liveChatIsWorkingAtom)).toBe(false);
    expect(store.get(liveChatAtom)?.pendingRequest).toBeNull();
    expect(Object.values(store.get(sessionSnapshotsAtom))).toEqual([fresh]);
  });

  it("ignores a load after navigating away and back to the same chat", async () => {
    const store = setup();
    const old = deferred<ChatDetail>();
    vi.mocked(daemonApi.getChat).mockReturnValueOnce(old.promise);
    const loading = store.set(loadLiveChatAtom, "chat-1");
    store.set(openLiveChatAtom, "chat-2");
    store.set(openLiveChatAtom, "chat-1");
    old.resolve(detail(snapshot()));
    await loading;
    expect(store.get(liveChatAtom)?.detail).toBeNull();
  });

  it("retries failed recovery reads while connected", async () => {
    vi.useFakeTimers();
    const store = setup();
    store.set(daemonEventStreamStateAtom, {
      status: "connected",
      error: null,
      reconnectAttempt: 0,
      retryInMs: null,
    });
    vi.mocked(daemonApi.getChat)
      .mockRejectedValueOnce(new Error("temporary"))
      .mockResolvedValueOnce(detail(snapshot()));
    await store.set(loadLiveChatAtom, "chat-1");
    await vi.advanceTimersByTimeAsync(250);
    expect(store.get(liveChatAtom)?.sessionState?.run_id).toBe("run-1");
  });

  it("auto-approves a recovered request only once", () => {
    const store = setup();
    store.set(permissionModeAtom, "full-agentic");
    vi.mocked(daemonApi.respondPermission).mockReturnValue(
      new Promise(() => {}),
    );
    const state = snapshot({
      pending_requests: [
        { request_id: "approve", kind: "approval", details: {} },
      ],
    });
    store.set(applySessionSnapshotAtom, state);
    store.set(applySessionSnapshotAtom, { ...state, revision: 2 });
    expect(daemonApi.respondPermission).toHaveBeenCalledTimes(1);
  });

  it("does not let an old reply completion clear the next pending request", async () => {
    const store = setup();
    store.set(
      applySessionSnapshotAtom,
      snapshot({
        pending_requests: [
          { request_id: "old", kind: "approval", details: {} },
        ],
      }),
    );
    const reply =
      deferred<Awaited<ReturnType<typeof daemonApi.respondPermission>>>();
    vi.mocked(daemonApi.respondPermission).mockReturnValueOnce(reply.promise);
    const responding = store.set(respondLiveRequestAtom, {});
    const next = snapshot({
      revision: 2,
      pending_requests: [{ request_id: "next", kind: "input", details: {} }],
    });
    store.set(applySessionSnapshotAtom, next);
    vi.mocked(daemonApi.getChat).mockResolvedValueOnce(detail(next));
    reply.resolve({ ok: true, request_id: "old" });
    await responding;
    expect(store.get(liveChatAtom)?.pendingRequest?.requestId).toBe("next");
  });

  it("keeps home-composer feedback while the agent is starting", () => {
    const store = createStore();
    store.set(openLiveChatAtom, { chatId: "chat-1", initialTurnActive: true });
    store.set(
      applySessionSnapshotAtom,
      snapshot({
        revision: 0,
        run_id: null,
        agent_id: null,
        run_status: null,
        turn: null,
      }),
    );
    expect(store.get(liveChatIsWorkingAtom)).toBe(true);
    store.set(
      applySessionSnapshotAtom,
      snapshot({ revision: 1, run_status: "starting", turn: null }),
    );
    expect(store.get(liveChatIsWorkingAtom)).toBe(true);
    store.set(
      applySessionSnapshotAtom,
      snapshot({
        revision: 3,
        turn: {
          user_message_id: "user-1",
          status: "completed",
          stop_reason: null,
          error_message: null,
          error_kind: null,
        },
      }),
    );
    expect(store.get(liveChatIsWorkingAtom)).toBe(false);
  });

  it("does not let an old prompt completion end a newer turn", async () => {
    const store = setup();
    store.set(applySessionSnapshotAtom, snapshot({ turn: null }));
    const result = deferred<PromptResultDto>();
    vi.mocked(daemonApi.prompt).mockReturnValueOnce(result.promise);
    const prompting = store.set(submitLivePromptAtom, {
      text: "hello",
      attachments: [],
      agentId: "agent-1",
    });
    const next = snapshot({
      revision: 4,
      turn: {
        user_message_id: "new-user",
        status: "started",
        stop_reason: null,
        error_message: null,
        error_kind: null,
      },
    });
    store.set(applySessionSnapshotAtom, next);
    vi.mocked(daemonApi.getChat).mockResolvedValueOnce(detail(next));
    result.resolve({
      run_id: "run-1",
      chat_id: "chat-1",
      agent_id: "agent-1",
      user_message_id: "old-user",
      acp_session_id: "session-1",
    });
    await prompting;
    expect(store.get(liveChatAtom)?.turnStatus).toBe("started");
    expect(store.get(liveChatAtom)?.sessionState?.turn?.user_message_id).toBe(
      "new-user",
    );
  });
});
