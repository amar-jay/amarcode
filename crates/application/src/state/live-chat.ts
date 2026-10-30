import { atom } from "jotai";
import { daemonApi } from "@/api";
import type {
  AgentFailureKind,
  ChatSessionSnapshot,
  SessionPendingRequest,
  Chat,
  ChatDetail,
  ContextUsage,
  JsonValue,
  PromptAttachment,
  RunStatus,
  SessionCommands,
  SessionConfigAssignment,
  SessionConfigOption,
  SessionConfigValue,
  TurnStatus,
} from "@/types";
import type { PendingAgentRequest } from "@/components/pending-agent-request";
import { notify } from "@/lib/notify";
import { automaticApprovalResult, shouldAutoApprove } from "./permission-mode";
import { permissionModeAtom } from "./preferences";
import {
  getLatestTurnForChat,
  daemonConnectionGenerationAtom,
  daemonEventStreamStateAtom,
  sessionSnapshotsAtom,
  rememberSessionSnapshotAtom,
} from "./daemon-events";
import { refreshChatsAtom } from "./chats";
import { activeSessionAtom } from "./navigation";
import { hasAcpSessionMode, rememberSessionConfig } from "./session-config";

const announcedFailures = new Set<string>();

function announceAgentFailure(message: string | null | undefined) {
  const text = message?.trim();
  if (!text) return;
  if (announcedFailures.has(text)) return;
  announcedFailures.add(text);
  // Keep the set bounded for long-lived app sessions.
  if (announcedFailures.size > 100) {
    const oldest = announcedFailures.values().next().value;
    if (oldest) announcedFailures.delete(oldest);
  }
  notify(text, "error", {
    id: `agent-failure:${text}`,
    duration: 12_000,
  });
}

function isChatDetail(value: Chat | ChatDetail): value is ChatDetail {
  return "messages" in value;
}

/**
 * Runtime state for the open conversation.
 * Reset whenever navigation opens a different chat id.
 */
export type LiveChatState = {
  chatId: string;
  sessionState?: ChatSessionSnapshot | null;
  pendingRequests?: SessionPendingRequest[];
  detail: ChatDetail | null;
  runId: string | null;
  runStatus: RunStatus | null;
  turnStatus: TurnStatus | null;
  pendingRequest: PendingAgentRequest | null;
  contextRestoration: string | null;
  contextUsage: ContextUsage | null;
  sessionConfig: SessionConfigOption[];
  sessionCommands: SessionCommands | null;
  sessionConfigAgentId: string | null;
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
  sessionState: null,
  pendingRequests: [],
  detail: null,
  runId: null,
  runStatus: null,
  turnStatus: null,
  pendingRequest: null,
  contextRestoration: null,
  contextUsage: null,
  sessionConfig: [],
  sessionCommands: null,
  sessionConfigAgentId: null,
  loading: true,
  error: null,
  errorKind: null,
  authRequired: null,
  ...seed,
});

export const liveChatAtom = atom<LiveChatState | null>(null);
const liveChatIdentityAtom = atom(0);
const chatLoadSequenceAtom = atom(0);
const localPromptAtom = atom<{
  identity: number;
  generation: number;
  snapshot: ChatSessionSnapshot | null;
} | null>(null);
const attemptedAutoApprovalsAtom = atom<Set<string>>(new Set<string>());
const autoApprovalsInFlightAtom = atom<Set<string>>(new Set<string>());

/** Runtime snapshots and event payloads share one reconciliation path. */
export const applySessionSnapshotAtom = atom(
  null,
  (get, set, snapshot: ChatSessionSnapshot) => {
    if (!set(rememberSessionSnapshotAtom, snapshot)) return;
    const live = get(liveChatAtom);
    if (!live || live.chatId !== snapshot.chat_id) return;
    if (
      live.sessionState?.daemon_instance_id === snapshot.daemon_instance_id &&
      live.sessionState.revision > snapshot.revision
    )
      return;
    const request = snapshot.pending_requests[0];
    const attemptKey = `${snapshot.daemon_instance_id}:${request?.request_id}`;
    const shouldAutomaticallyApprove =
      request?.kind === "approval" &&
      !hasAcpSessionMode(live.sessionConfig) &&
      shouldAutoApprove(get(permissionModeAtom), request.details) &&
      !get(attemptedAutoApprovalsAtom).has(attemptKey);
    const automaticallyApproving =
      shouldAutomaticallyApprove ||
      get(autoApprovalsInFlightAtom).has(attemptKey);
    const localPrompt = get(localPromptAtom);
    const preserveOptimisticTurn =
      localPrompt &&
      localPrompt.identity === get(liveChatIdentityAtom) &&
      localPrompt.generation === get(daemonConnectionGenerationAtom) &&
      live.turnStatus === "started" &&
      ((!localPrompt.snapshot &&
        snapshot.revision === 0 &&
        snapshot.run_id === null) ||
        (localPrompt.snapshot?.daemon_instance_id ===
          snapshot.daemon_instance_id &&
          localPrompt.snapshot.revision >= snapshot.revision) ||
        ((localPrompt.snapshot === null ||
          localPrompt.snapshot.daemon_instance_id ===
            snapshot.daemon_instance_id) &&
          snapshot.turn === null &&
          snapshot.error_message === null &&
          (snapshot.run_status === "starting" ||
            snapshot.run_status === "running")));
    const unchangedRuntime =
      live.sessionState?.daemon_instance_id === snapshot.daemon_instance_id &&
      live.sessionState.revision === snapshot.revision;
    const preserveLocalFailure =
      live.turnStatus === "failed" &&
      snapshot.error_message === null &&
      (unchangedRuntime || (!live.sessionState && snapshot.revision === 0));
    const runChanged =
      live.sessionState?.daemon_instance_id !== snapshot.daemon_instance_id ||
      live.runId !== snapshot.run_id;
    set(liveChatAtom, {
      ...live,
      sessionState: snapshot,
      runId: snapshot.run_id,
      runStatus: snapshot.run_status,
      turnStatus: preserveOptimisticTurn
        ? "started"
        : preserveLocalFailure
          ? "failed"
          : (snapshot.turn?.status ?? null),
      pendingRequests: snapshot.pending_requests,
      pendingRequest:
        request && !automaticallyApproving
          ? {
              requestId: request.request_id,
              kind: request.kind,
              details: request.details,
            }
          : null,
      contextRestoration: snapshot.context_restoration,
      contextUsage: runChanged ? null : live.contextUsage,
      sessionCommands:
        (snapshot.run_status === "starting" ||
          snapshot.run_status === "running") &&
        live.sessionCommands?.run_id === snapshot.run_id
          ? live.sessionCommands
          : null,
      error: preserveLocalFailure ? live.error : snapshot.error_message,
      errorKind: preserveLocalFailure ? live.errorKind : snapshot.error_kind,
      authRequired:
        snapshot.error_kind === "auth_required" ? live.authRequired : null,
    });
    if (snapshot.error_message) announceAgentFailure(snapshot.error_message);
    if (request && shouldAutomaticallyApprove) {
      set(
        autoApprovalsInFlightAtom,
        new Set([...get(autoApprovalsInFlightAtom), attemptKey]),
      );
      set(
        attemptedAutoApprovalsAtom,
        new Set([...get(attemptedAutoApprovalsAtom), attemptKey].slice(-256)),
      );
      const generation = get(daemonConnectionGenerationAtom);
      const identity = get(liveChatIdentityAtom);
      void daemonApi
        .respondPermission(request.request_id, {
          result: automaticApprovalResult(request.details),
        })
        .catch((cause: unknown) => {
          const current = get(liveChatAtom);
          if (
            current?.chatId !== snapshot.chat_id ||
            generation !== get(daemonConnectionGenerationAtom) ||
            identity !== get(liveChatIdentityAtom)
          )
            return;
          if (
            current.sessionState?.pending_requests[0]?.request_id ===
            request.request_id
          )
            set(liveChatAtom, {
              ...current,
              error: String(cause),
              pendingRequest: {
                requestId: request.request_id,
                kind: request.kind,
                details: request.details,
              },
            });
        })
        .finally(() => {
          set(
            autoApprovalsInFlightAtom,
            new Set(
              [...get(autoApprovalsInFlightAtom)].filter(
                (key) => key !== attemptKey,
              ),
            ),
          );
          if (
            generation === get(daemonConnectionGenerationAtom) &&
            identity === get(liveChatIdentityAtom) &&
            get(liveChatAtom)?.chatId === snapshot.chat_id
          )
            void set(loadLiveChatAtom, {
              chatId: snapshot.chat_id,
              silent: true,
            });
        });
    }
  },
);

/** Busy = open prompt turn (run/session can stay alive across many turns). */
export const liveChatIsWorkingAtom = atom(
  (get) => get(liveChatAtom)?.turnStatus === "started",
);

export type OpenLiveChatInput = {
  chatId: string;
  initialRunId?: string | null;
  initialTurnActive?: boolean;
};

/**
 * Reset conversation-owned state when navigation selects a chat.
 * Prefer calling with just `chatId`, seed fields are read from
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
          }
        : input;

    // Same chat already open, don't wipe streaming state on incidental re-renders.
    // (The live-chat effect only depends on chatId, so this mainly guards Strict Mode.)
    const existing = get(liveChatAtom);
    if (existing?.chatId === chatId && existing.detail) return;

    set(liveChatIdentityAtom, get(liveChatIdentityAtom) + 1);
    const cached = getLatestTurnForChat(chatId);
    const turnStatus =
      cached?.status ?? (seed.initialTurnActive ? "started" : null);
    const failed = turnStatus === "failed";
    set(
      liveChatAtom,
      emptyLiveChat(chatId, {
        runId: cached?.run_id ?? seed.initialRunId ?? null,
        runStatus: turnStatus === "started" ? "running" : null,
        // Prefer observed turn status over the navigation "just started" flag.
        turnStatus,
        error: failed ? (cached?.error_message ?? null) : null,
        errorKind: failed ? "error" : null,
        loading: true,
      }),
    );
    const snapshot = get(sessionSnapshotsAtom)[chatId];
    if (seed.initialTurnActive && !cached)
      set(localPromptAtom, {
        identity: get(liveChatIdentityAtom),
        generation: get(daemonConnectionGenerationAtom),
        snapshot: snapshot ?? null,
      });
    if (snapshot) set(applySessionSnapshotAtom, snapshot);
  },
);

export const clearLiveChatAtom = atom(null, (get, set) => {
  set(liveChatIdentityAtom, get(liveChatIdentityAtom) + 1);
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
      const attempt = get(localPromptAtom);
      if (
        live.sessionState &&
        (!attempt ||
          attempt.identity !== get(liveChatIdentityAtom) ||
          attempt.generation !== get(daemonConnectionGenerationAtom) ||
          (!attempt.snapshot && live.sessionState.revision > 0) ||
          (attempt.snapshot &&
            (attempt.snapshot.daemon_instance_id !==
              live.sessionState.daemon_instance_id ||
              attempt.snapshot.revision < live.sessionState.revision)))
      ) {
        void set(loadLiveChatAtom, { chatId: input.chatId, silent: true });
        return;
      }
      set(localPromptAtom, null);
      set(liveChatAtom, {
        ...live,
        turnStatus: "failed",
        pendingRequest: null,
        error: input.error,
        errorKind: "error",
      });
    }
    announceAgentFailure(input.error);
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
  async (
    get,
    set,
    input?:
      string | { chatId?: string; silent?: boolean; retryAttempt?: number },
  ) => {
    const live = get(liveChatAtom);
    const chatId = typeof input === "string" ? input : input?.chatId;
    const silent = typeof input === "object" ? Boolean(input?.silent) : false;
    const id = chatId ?? live?.chatId;
    if (!id) return;
    if (live?.chatId !== id) return;
    const commandsAtRequest = live?.sessionCommands;
    const identity = get(liveChatIdentityAtom);
    const generation = get(daemonConnectionGenerationAtom);
    const sequence = get(chatLoadSequenceAtom) + 1;
    set(chatLoadSequenceAtom, sequence);
    const stillCurrent = () =>
      identity === get(liveChatIdentityAtom) &&
      generation === get(daemonConnectionGenerationAtom) &&
      sequence === get(chatLoadSequenceAtom) &&
      get(liveChatAtom)?.chatId === id;

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
      if (!current || !stillCurrent()) return;
      if (isChatDetail(result)) {
        const configAgentId = [...result.messages]
          .reverse()
          .find((message) => message.agent_id)?.agent_id;
        // A slow detail request must not overwrite a newer streamed catalog.
        const sessionCommands =
          current.sessionCommands !== commandsAtRequest
            ? current.sessionCommands
            : (result.session_commands ?? null);
        set(liveChatAtom, {
          ...current,
          detail: result,
          sessionCommands,
          runId: sessionCommands?.run_id ?? current.runId,
          sessionConfig: result.session_config ?? current.sessionConfig,
          sessionConfigAgentId:
            configAgentId ??
            current.sessionConfigAgentId ??
            get(activeSessionAtom)?.agent?.id ??
            null,
          contextUsage: result.context_usage ?? current.contextUsage,
          loading: false,
          // Keep failure state across detail reloads. Clearing it here made the
          // banner flash for ~80ms after turnUpdated(failed) scheduled a refresh.
        });
        if (result.session_state)
          set(applySessionSnapshotAtom, result.session_state);
      } else {
        set(liveChatAtom, { ...current, loading: false });
      }
    } catch (cause) {
      const current = get(liveChatAtom);
      if (!current || !stillCurrent()) return;
      set(liveChatAtom, {
        ...current,
        loading: false,
        error:
          cause instanceof Error ? cause.message : "Unable to load this chat.",
      });
      if (get(daemonEventStreamStateAtom).status === "connected") {
        const attempt =
          typeof input === "object" ? (input.retryAttempt ?? 0) : 0;
        setTimeout(
          () => {
            if (
              stillCurrent() &&
              get(daemonEventStreamStateAtom).status === "connected"
            )
              void set(retryLiveChatLoadAtom, {
                chatId: id,
                silent: true,
                retryAttempt: attempt + 1,
              });
          },
          Math.min(250 * 2 ** Math.min(attempt, 5), 5_000),
        );
      }
    }
  },
);

// A separate action is necessary: setting a write atom from its own writer sets
// its backing value instead of invoking that writer again.
const retryLiveChatLoadAtom = atom(
  null,
  (
    _get,
    set,
    input: { chatId: string; silent: boolean; retryAttempt: number },
  ) => set(loadLiveChatAtom, input),
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
    if (event.type === "sessionStateUpdated") {
      set(applySessionSnapshotAtom, event.payload.snapshot);
      if (event.payload.snapshot.chat_id === live.chatId)
        void set(scheduleLiveChatRefreshAtom);
      return;
    }

    if (event.type === "chatUpdated" && event.payload.chat_id === live.chatId) {
      void set(scheduleLiveChatRefreshAtom);
      return;
    }

    if (
      event.type === "sessionCommandsUpdated" &&
      event.payload.chat_id === live.chatId
    ) {
      const catalog = event.payload.catalog;
      if (
        live.sessionState &&
        (catalog.run_id !== live.sessionState.run_id ||
          !["starting", "running"].includes(live.sessionState.run_status ?? ""))
      )
        return;
      const current = live.sessionCommands;
      if (
        current?.run_id === catalog.run_id &&
        current.revision >= catalog.revision
      )
        return;
      set(liveChatAtom, {
        ...live,
        runId: catalog.run_id,
        sessionCommands: catalog,
      });
      return;
    }

    if (
      event.type === "sessionConfigUpdated" &&
      event.payload.chat_id === live.chatId
    ) {
      const options = event.payload.options;
      const session = get(activeSessionAtom);
      if (session?.agent?.id) rememberSessionConfig(session.agent.id, options);
      set(liveChatAtom, {
        ...live,
        sessionConfig: options,
        sessionConfigAgentId: session?.agent?.id ?? live.sessionConfigAgentId,
      });
      return;
    }

    if (
      event.type === "contextUsageUpdated" &&
      event.payload.chat_id === live.chatId
    ) {
      if (
        live.sessionState &&
        event.payload.run_id !== live.sessionState.run_id
      )
        return;
      set(liveChatAtom, {
        ...live,
        runId: event.payload.run_id,
        contextUsage: event.payload.usage,
      });
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

    // Message events carry no content, they only make streaming feel immediate.
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
      agentId: string;
      configValues?: SessionConfigAssignment[];
    },
  ) => {
    const live = get(liveChatAtom);
    if (
      !live ||
      (!input.text.trim() && input.attachments.length === 0) ||
      live.turnStatus === "started"
    )
      return;

    const identity = get(liveChatIdentityAtom);
    const generation = get(daemonConnectionGenerationAtom);
    const attempt = {
      identity,
      generation,
      snapshot: live.sessionState ?? null,
    };
    set(localPromptAtom, attempt);
    const stillCurrent = () =>
      get(liveChatIdentityAtom) === identity &&
      get(daemonConnectionGenerationAtom) === generation &&
      get(localPromptAtom) === attempt;
    set(liveChatAtom, {
      ...live,
      turnStatus: "started",
      error: null,
    });

    try {
      const result = await daemonApi.prompt(
        live.chatId,
        input.agentId,
        input.text.trim(),
        input.attachments,
        input.configValues ?? [],
      );
      const current = get(liveChatAtom);
      if (!current || !stillCurrent()) return;
      set(localPromptAtom, null);
      if (!current.sessionState)
        set(liveChatAtom, {
          ...current,
          runId: result.run_id,
          turnStatus:
            current.turnStatus === "started" ? "completed" : current.turnStatus,
        });
      await set(refreshChatsAtom);
      await set(loadLiveChatAtom, live.chatId);
    } catch (cause) {
      const current = get(liveChatAtom);
      if (!current || !stillCurrent()) return;
      set(localPromptAtom, null);
      if (current.sessionState && current.sessionState !== attempt.snapshot) {
        await set(loadLiveChatAtom, { chatId: live.chatId, silent: true });
        throw cause;
      }
      const error =
        cause instanceof Error ? cause.message : "Unable to send prompt.";
      set(liveChatAtom, {
        ...current,
        turnStatus: "failed",
        error,
        errorKind: "error",
      });
      announceAgentFailure(error);
      throw cause;
    }
  },
);

export const setLiveSessionConfigOptionAtom = atom(
  null,
  async (get, set, input: { configId: string; value: SessionConfigValue }) => {
    const live = get(liveChatAtom);
    if (!live) return;
    const session = get(activeSessionAtom);
    if (!session?.agent?.id || live.sessionConfigAgentId !== session.agent.id) {
      return;
    }
    const optimistic = live.sessionConfig.map((option) =>
      option.id === input.configId
        ? {
            ...option,
            current_value:
              input.value.type === "boolean"
                ? input.value.value
                : input.value.value,
          }
        : option,
    );
    set(liveChatAtom, { ...live, sessionConfig: optimistic });
    rememberSessionConfig(session.agent.id, optimistic);
    try {
      const result = await daemonApi.setSessionConfigOption(
        live.chatId,
        input.configId,
        input.value,
      );
      const current = get(liveChatAtom);
      if (!current || current.chatId !== live.chatId) return;
      set(liveChatAtom, {
        ...current,
        sessionConfig: result.options,
        sessionConfigAgentId: session.agent.id,
      });
      rememberSessionConfig(session.agent.id, result.options);
    } catch (cause) {
      console.info("Session config will apply when this chat starts:", cause);
    }
  },
);

export const stopLiveChatAtom = atom(null, async (get, set) => {
  const live = get(liveChatAtom);
  if (!live) return;
  const identity = get(liveChatIdentityAtom);
  const generation = get(daemonConnectionGenerationAtom);
  const stillCurrent = () =>
    get(liveChatIdentityAtom) === identity &&
    get(daemonConnectionGenerationAtom) === generation;
  try {
    await daemonApi.cancel(live.chatId);
    const current = get(liveChatAtom);
    if (!current || !stillCurrent()) return;
    if (!current.sessionState)
      set(liveChatAtom, {
        ...current,
        turnStatus:
          current.turnStatus === "started" ? "cancelled" : current.turnStatus,
        pendingRequest: null,
      });
    await set(loadLiveChatAtom, live.chatId);
  } catch (cause) {
    const current = get(liveChatAtom);
    if (!current || !stillCurrent()) return;
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
    const identity = get(liveChatIdentityAtom);
    const generation = get(daemonConnectionGenerationAtom);
    const stillCurrent = () =>
      get(liveChatIdentityAtom) === identity &&
      get(daemonConnectionGenerationAtom) === generation;
    try {
      if (live.pendingRequest.kind === "approval") {
        await daemonApi.respondPermission(live.pendingRequest.requestId, {
          result,
        });
      } else {
        await daemonApi.respondInput(live.pendingRequest.requestId, { result });
      }
      const current = get(liveChatAtom);
      if (!current || !stillCurrent()) return;
      if (
        !current.sessionState &&
        current.pendingRequest?.requestId === live.pendingRequest.requestId
      )
        set(liveChatAtom, { ...current, pendingRequest: null });
      await set(loadLiveChatAtom, { chatId: live.chatId, silent: true });
    } catch (cause) {
      const current = get(liveChatAtom);
      if (!current || !stillCurrent()) return;
      if (current.pendingRequest?.requestId === live.pendingRequest.requestId)
        set(liveChatAtom, {
          ...current,
          error:
            cause instanceof Error
              ? cause.message
              : "Unable to respond to the agent.",
        });
      await set(loadLiveChatAtom, { chatId: live.chatId, silent: true });
    }
  },
);
