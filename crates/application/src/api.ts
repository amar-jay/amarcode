import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AgentDefinition,
  AgentResponse,
  CancelResult,
  Chat,
  DaemonVersion,
  EditorEvent,
  EventFilter,
  GetChatResult,
  Health,
  PromptResult,
  RespondAgentResult,
} from "./types";

export type DaemonEventStreamStatus =
  { status: "connected" } | { status: "disconnected"; error: string };

export type DaemonBootstrapStatus =
  | { status: "checking" }
  | { status: "downloading"; received: number; total: number }
  | { status: "verifying" }
  | { status: "installing" }
  | { status: "starting" }
  | { status: "ready"; version: string }
  | { status: "failed"; error: string };

/**
 * Typed bindings for the daemon-backed Tauri commands.
 *
 * This is intentionally a direct mirror of `src-tauri/src/lib.rs`; no
 * session-era compatibility methods live here.
 */
export const daemonApi = {
  bootstrap: async (
    onStatus: (status: DaemonBootstrapStatus) => void,
  ): Promise<Health> => {
    const statusChannel = new Channel<DaemonBootstrapStatus>();
    statusChannel.onmessage = onStatus;
    return invoke("daemon_bootstrap", { onStatus: statusChannel });
  },
  health: (): Promise<Health> => invoke("daemon_health"),
  version: (): Promise<DaemonVersion> => invoke("daemon_version"),

  listAgents: (): Promise<AgentDefinition[]> => invoke("list_agents"),

  createChat: (workspacePath: string, title?: string): Promise<Chat> =>
    invoke("create_chat", { workspacePath, title }),
  listChats: (workspacePath?: string): Promise<Chat[]> =>
    invoke("list_chats", { workspacePath }),
  getChat: (chatId: string, includeMessages = true): Promise<GetChatResult> =>
    invoke("get_chat", { chatId, includeMessages }),

  prompt: (
    chatId: string,
    agentId: string,
    text: string,
    sessionMode?: "plan" | "build" | "ask",
  ): Promise<PromptResult> =>
    invoke("prompt", { chatId, agentId, text, sessionMode }),
  setSessionMode: (
    chatId: string,
    mode: "plan" | "build" | "ask",
  ): Promise<void> => invoke("set_session_mode", { chatId, mode }),
  cancel: (chatId: string): Promise<CancelResult> =>
    invoke("cancel", { chatId }),

  respondPermission: (
    requestId: string,
    response: AgentResponse,
  ): Promise<RespondAgentResult> =>
    invoke("respond_permission", {
      params: {
        request_id: requestId,
        result: response.result ?? null,
        error: response.error ?? null,
      },
    }),
  respondInput: (
    requestId: string,
    response: AgentResponse,
  ): Promise<RespondAgentResult> =>
    invoke("respond_input", {
      params: {
        request_id: requestId,
        result: response.result ?? null,
        error: response.error ?? null,
      },
    }),

  subscribeEvents: async (
    filter: EventFilter,
    onEvent: (event: EditorEvent) => void,
    onStatus: (status: DaemonEventStreamStatus) => void,
  ): Promise<void> => {
    const eventChannel = new Channel<EditorEvent>();
    eventChannel.onmessage = onEvent;
    const statusChannel = new Channel<DaemonEventStreamStatus>();
    statusChannel.onmessage = onStatus;
    // This promise intentionally remains pending while the stream is healthy.
    // Tauri rejects it when the daemon connection ends.
    await invoke("subscribe_events", {
      filter,
      onEvent: eventChannel,
      onStatus: statusChannel,
    });
  },
} as const;
