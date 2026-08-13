import { atom } from "jotai";
import { daemonApi } from "@/api";
import type {
  Chat,
  ChatDetail,
  JsonValue,
  RunStatus,
  TurnStatus,
} from "@/types";
import type { PendingAgentRequest } from "@/components/pending-agent-request";
import type { SessionMode } from "./session-mode";
import { getLatestTurnForChat } from "./daemon-events";
import { refreshChatsAtom } from "./chats";
import { activeSessionAtom } from "./navigation";

function isChatDetail(value: Chat | ChatDetail): value is ChatDetail {
  return "messages" in value;
}

/**
 * Runtime state for the open conversation.
 * Reset whenever navigation opens a different chat id.
 */
export type LiveChatState = {
  chatId: string;
  detail: ChatDetail | null;
  runId: string | null;
  runStatus: RunStatus | null;
  turnStatus: TurnStatus | null;
  pendingRequest: PendingAgentRequest | null;
  contextRestoration: string | null;
  sessionMode: SessionMode;
  loading: boolean;
  error: string | null;
};

const emptyLiveChat = (
  chatId: string,
  seed?: Partial<LiveChatState>,
): LiveChatState => ({
  chatId,
  detail: null,
  runId: null,
  runStatus: null,
  turnStatus: null,
  pendingRequest: null,
  contextRestoration: null,
  sessionMode: "build",
  loading: true,
  error: null,
  ...seed,
});

export const liveChatAtom = atom<LiveChatState | null>(null);

/** Busy = open prompt turn (run/session can stay alive across many turns). */
export const liveChatIsWorkingAtom = atom(
  (get) => get(liveChatAtom)?.turnStatus === "started",
);

export type OpenLiveChatInput = {
  chatId: string;
  initialRunId?: string | null;
  initialTurnActive?: boolean;
  sessionMode?: SessionMode;
};

/**
 * Reset conversation-owned state when navigation selects a chat.
 * Prefer calling with just `chatId` — seed fields are read from
 * `activeSessionAtom` so the live-chat effect only depends on identity.
 */
export const openLiveChatAtom = atom(
  null,
  (get, set, input: string | OpenLiveChatInput) => {
    const session = get(activeSessionAtom);
    const chatId = typeof input === "string" ? input : input.chatId;
    const seed: OpenLiveChatInput =
      typeof input === "string"
        ? {
            chatId,
            initialRunId: session?.initialRunId,
            initialTurnActive: session?.initialTurnActive,
            sessionMode: session?.sessionMode,
          }
        : input;

    // Same chat already open — don't wipe streaming state on incidental re-renders.
    // (The live-chat effect only depends on chatId, so this mainly guards Strict Mode.)
    const existing = get(liveChatAtom);
    if (existing?.chatId === chatId && existing.detail) return;

    const cached = getLatestTurnForChat(chatId);
    const turnStatus =
      cached?.status ?? (seed.initialTurnActive ? "started" : null);
    set(
      liveChatAtom,
      emptyLiveChat(chatId, {
        runId: cached?.run_id ?? seed.initialRunId ?? null,
        runStatus: turnStatus === "started" ? "running" : null,
        // Prefer observed turn status over the navigation "just started" flag.
        turnStatus,
        sessionMode: seed.sessionMode ?? "build",
        loading: true,
      }),
    );
  },
);

export const clearLiveChatAtom = atom(null, (_get, set) => {
  set(liveChatAtom, null);
});

function patchLive(
  get: () => LiveChatState | null,
  set: (v: LiveChatState) => void,
  patch: Partial<LiveChatState>,
) {
  const current = get();
  if (!current) return;
  set({ ...current, ...patch });
}

/** Fetch ChatDetail for the open chat. */
export const loadLiveChatAtom = atom(
  null,
  async (get, set, input?: string | { chatId?: string; silent?: boolean }) => {
    const live = get(liveChatAtom);
    const chatId = typeof input === "string" ? input : input?.chatId;
    const silent = typeof input === "object" ? Boolean(input?.silent) : false;
    const id = chatId ?? live?.chatId;
    if (!id) return;

    if (!silent) {
      patchLive(
        () => get(liveChatAtom),
        (v) => set(liveChatAtom, v),
        { loading: true },
      );
    }

    try {
      const result = await daemonApi.getChat(id, true);
      const current = get(liveChatAtom);
      // Ignore stale responses after navigation away.
      if (!current || current.chatId !== id) return;
      if (isChatDetail(result)) {
        set(liveChatAtom, {
          ...current,
          detail: result,
          loading: false,
          error: null,
        });
      } else {
        set(liveChatAtom, { ...current, loading: false });
      }
    } catch (cause) {
      const current = get(liveChatAtom);
      if (!current || current.chatId !== id) return;
      set(liveChatAtom, {
        ...current,
        loading: false,
        error:
          cause instanceof Error ? cause.message : "Unable to load this chat.",
      });
    }
  },
);

/** Coalesce rapid message stream events into one detail reload. */
let refreshTimer: ReturnType<typeof setTimeout> | undefined;
export const scheduleLiveChatRefreshAtom = atom(null, (get, set) => {
  const live = get(liveChatAtom);
  if (!live) return;
  if (refreshTimer) clearTimeout(refreshTimer);
  const chatId = live.chatId;
  refreshTimer = setTimeout(() => {
    void set(loadLiveChatAtom, { chatId, silent: true });
  }, 80);
});

/** Apply one daemon event to the open live chat (caller filters relevance). */
export const applyLiveChatEventAtom = atom(
  null,
  (get, set, event: import("@/types").EditorEvent) => {
    const live = get(liveChatAtom);
    if (!live) return;

    if (event.type === "chatUpdated" && event.payload.chat_id === live.chatId) {
      void set(scheduleLiveChatRefreshAtom);
      void set(refreshChatsAtom);
      return;
    }

    if (event.type === "turnUpdated" && event.payload.chat_id === live.chatId) {
      const next: LiveChatState = {
        ...live,
        turnStatus: event.payload.status,
        runId: event.payload.run_id,
        pendingRequest:
          event.payload.status !== "started" ? null : live.pendingRequest,
        contextRestoration:
          event.payload.status !== "started" ? null : live.contextRestoration,
        error:
          event.payload.status === "failed" && event.payload.error_message
            ? event.payload.error_message
            : live.error,
      };
      set(liveChatAtom, next);
      void set(scheduleLiveChatRefreshAtom);
      return;
    }

    if (
      event.type === "contextRestoration" &&
      event.payload.chat_id === live.chatId
    ) {
      set(liveChatAtom, {
        ...live,
        runId: event.payload.run_id,
        contextRestoration: event.payload.source,
      });
      return;
    }

    if (event.type === "runUpdated" && event.payload.run_id === live.runId) {
      const ended = ["completed", "stopped", "failed"].includes(
        event.payload.status,
      );
      set(liveChatAtom, {
        ...live,
        runStatus: event.payload.status,
        turnStatus:
          ended && live.turnStatus === "started"
            ? "cancelled"
            : live.turnStatus,
        pendingRequest: ended ? null : live.pendingRequest,
      });
      void set(scheduleLiveChatRefreshAtom);
      return;
    }

    if (
      (event.type === "approvalRequired" ||
        event.type === "questionRequired") &&
      (event.payload.run_id === live.runId || live.turnStatus === "started")
    ) {
      set(liveChatAtom, {
        ...live,
        runId: event.payload.run_id,
        pendingRequest: {
          kind: event.type === "approvalRequired" ? "approval" : "input",
          requestId: event.payload.request_id,
          details: event.payload.details,
        },
      });
      return;
    }

    // Message events carry no content — they only make streaming feel immediate.
    if (event.type === "messageUpdated" || event.type === "messagePartAdded") {
      void set(scheduleLiveChatRefreshAtom);
    }
  },
);

export const submitLivePromptAtom = atom(
  null,
  async (
    get,
    set,
    input: { text: string; mode: SessionMode; agentId: string },
  ) => {
    const live = get(liveChatAtom);
    if (!live || !input.text.trim() || live.turnStatus === "started") return;

    set(liveChatAtom, {
      ...live,
      turnStatus: "started",
      sessionMode: input.mode,
      error: null,
    });

    try {
      const result = await daemonApi.prompt(
        live.chatId,
        input.agentId,
        input.text.trim(),
        input.mode,
      );
      const current = get(liveChatAtom);
      if (!current || current.chatId !== live.chatId) return;
      set(liveChatAtom, {
        ...current,
        runId: result.run_id,
        // If turnUpdated was missed, the RPC return means the turn finished.
        turnStatus:
          current.turnStatus === "started" ? "completed" : current.turnStatus,
      });
      await set(refreshChatsAtom);
      await set(loadLiveChatAtom, live.chatId);
    } catch (cause) {
      const current = get(liveChatAtom);
      if (!current || current.chatId !== live.chatId) return;
      set(liveChatAtom, {
        ...current,
        turnStatus: "failed",
        error:
          cause instanceof Error ? cause.message : "Unable to send prompt.",
      });
    }
  },
);

export const setLiveSessionModeAtom = atom(
  null,
  async (get, set, mode: SessionMode) => {
    const live = get(liveChatAtom);
    if (!live) return;
    set(liveChatAtom, { ...live, sessionMode: mode });
    try {
      await daemonApi.setSessionMode(live.chatId, mode);
    } catch (cause) {
      // Historical chat may have no live ACP session yet.
      console.info("Session mode will apply when this chat starts:", cause);
    }
  },
);

export const stopLiveChatAtom = atom(null, async (get, set) => {
  const live = get(liveChatAtom);
  if (!live) return;
  try {
    await daemonApi.cancel(live.chatId);
    const current = get(liveChatAtom);
    if (!current || current.chatId !== live.chatId) return;
    set(liveChatAtom, {
      ...current,
      turnStatus:
        current.turnStatus === "started" ? "cancelled" : current.turnStatus,
      pendingRequest: null,
    });
    await set(loadLiveChatAtom, live.chatId);
  } catch (cause) {
    const current = get(liveChatAtom);
    if (!current) return;
    set(liveChatAtom, {
      ...current,
      error:
        cause instanceof Error ? cause.message : "Unable to cancel the run.",
    });
  }
});

export const respondLiveRequestAtom = atom(
  null,
  async (get, set, result: JsonValue) => {
    const live = get(liveChatAtom);
    if (!live?.pendingRequest) return;
    try {
      if (live.pendingRequest.kind === "approval") {
        await daemonApi.respondPermission(live.pendingRequest.requestId, {
          result,
        });
      } else {
        await daemonApi.respondInput(live.pendingRequest.requestId, { result });
      }
      const current = get(liveChatAtom);
      if (!current) return;
      set(liveChatAtom, { ...current, pendingRequest: null });
    } catch (cause) {
      const current = get(liveChatAtom);
      if (!current) return;
      set(liveChatAtom, {
        ...current,
        error:
          cause instanceof Error
            ? cause.message
            : "Unable to respond to the agent.",
      });
    }
  },
);
