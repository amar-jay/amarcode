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

/**
 * Typed bindings for the daemon-backed Tauri commands.
 *
 * This is intentionally a direct mirror of `src-tauri/src/lib.rs`; no
 * session-era compatibility methods live here.
 */
export const daemonApi = {
  health: (): Promise<Health> => invoke("daemon_health"),
  version: (): Promise<DaemonVersion> => invoke("daemon_version"),

  listAgents: (): Promise<AgentDefinition[]> => invoke("list_agents"),

  createChat: (workspacePath: string, title?: string): Promise<Chat> =>
    invoke("create_chat", { workspacePath, title }),
  listChats: (workspacePath?: string): Promise<Chat[]> =>
    invoke("list_chats", { workspacePath }),
  getChat: (chatId: string, includeMessages = true): Promise<GetChatResult> =>
    invoke("get_chat", { chatId, includeMessages }),

  prompt: (chatId: string, agentId: string, text: string): Promise<PromptResult> =>
    invoke("prompt", { chatId, agentId, text }),
  cancel: (chatId: string): Promise<CancelResult> => invoke("cancel", { chatId }),

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
  respondInput: (requestId: string, response: AgentResponse): Promise<RespondAgentResult> =>
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
  ): Promise<void> => {
    const channel = new Channel<EditorEvent>();
    channel.onmessage = onEvent;
    await invoke("subscribe_events", { filter, onEvent: channel });
  },
} as const;
