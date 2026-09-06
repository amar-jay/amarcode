import { atom } from "jotai";
import { daemonApi } from "@/api";
import type {
  AgentFailureKind,
  Chat,
  ChatDetail,
  JsonValue,
  PromptAttachment,
  RunStatus,
  TurnStatus,
} from "@/types";
import type { PendingAgentRequest } from "@/components/pending-agent-request";
import type { SessionMode } from "./session-mode";
import { automaticApprovalResult, shouldAutoApprove } from "./permission-mode";
import { permissionModeAtom } from "./preferences";
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
  errorKind: AgentFailureKind | null;
  authRequired: {
    agentId: string;
    runId: string | null;
    methods: JsonValue;
  } | null;
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
  errorKind: null,
  authRequired: null,
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

/** Clear the live-chat failure banner after the user recovers (e.g. sign-in). */
export const clearLiveChatFailureAtom = atom(null, (get, set) => {
  const live = get(liveChatAtom);
  if (!live) return;
  set(liveChatAtom, {
    ...live,
    error: null,
    errorKind: null,
    authRequired: null,
  });
});

/** End the optimistic working state when a home-composer prompt fails. */
export const failStartedPromptAtom = atom(
  null,
  (
    get,
    set,
    input: {
      chatId: string;
      error: string;
    },
  ) => {
    const session = get(activeSessionAtom);
    if (session?.chat.id === input.chatId) {
      set(activeSessionAtom, { ...session, initialTurnActive: false });
    }

    const live = get(liveChatAtom);
    if (live?.chatId === input.chatId) {
      set(liveChatAtom, {
        ...live,
        turnStatus: "failed",
        pendingRequest: null,
        error: input.error,
        errorKind: "error",
      });
    }
  },
);

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
            : event.payload.status === "started"
              ? null
              : live.error,
        errorKind:
          event.payload.status === "failed"
            ? (event.payload.error_kind ?? "error")
            : event.payload.status === "started"
              ? null
              : live.errorKind,
      };
      set(liveChatAtom, next);
      void set(scheduleLiveChatRefreshAtom);
      return;
    }

    if (event.type === "agentAuthRequired") {
      set(liveChatAtom, {
        ...live,
        authRequired: {
          agentId: event.payload.agent_id,
          runId: event.payload.run_id,
          methods: event.payload.methods,
        },
        errorKind: "auth_required",
        error: live.error ?? "Sign in required before this agent can run",
      });
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
      const failed = event.payload.status === "failed";
      set(liveChatAtom, {
        ...live,
        runStatus: event.payload.status,
        turnStatus:
          failed && live.turnStatus === "started"
            ? "failed"
            : ended && live.turnStatus === "started"
              ? "cancelled"
              : live.turnStatus,
        pendingRequest: ended ? null : live.pendingRequest,
        error:
          failed && event.payload.error_message
            ? event.payload.error_message
            : live.error,
        errorKind: failed
          ? (event.payload.error_kind ?? live.errorKind ?? "error")
          : live.errorKind,
      });
      void set(scheduleLiveChatRefreshAtom);
      return;
    }

    if (
      (event.type === "approvalRequired" ||
        event.type === "questionRequired") &&
      (event.payload.run_id === live.runId || live.turnStatus === "started")
    ) {
      if (
        event.type === "approvalRequired" &&
        shouldAutoApprove(get(permissionModeAtom), event.payload.details)
      ) {
        void daemonApi
          .respondPermission(event.payload.request_id, {
            result: automaticApprovalResult(event.payload.details),
          })
          .catch((cause: unknown) => {
            const current = get(liveChatAtom);
            if (!current) return;
            set(liveChatAtom, {
              ...current,
              error:
                cause instanceof Error
                  ? cause.message
                  : "Unable to auto-approve the agent action.",
            });
          });
        return;
      }
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
    input: {
      text: string;
      attachments: PromptAttachment[];
      mode: SessionMode;
      agentId: string;
    },
  ) => {
    const live = get(liveChatAtom);
    if (
      !live ||
      (!input.text.trim() && input.attachments.length === 0) ||
      live.turnStatus === "started"
    )
      return;

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
        input.attachments,
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
      throw cause;
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
