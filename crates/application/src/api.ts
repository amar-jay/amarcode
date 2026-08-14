import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AgentInfo,
  AgentResponse,
  CancelResult,
  Chat,
  DeleteChatResult,
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
  | { status: "installRequired"; reason: string }
  | { status: "downloading"; received: number; total: number }
  | { status: "verifying" }
  | { status: "installing" }
  | { status: "starting" }
  | { status: "ready"; version: string }
  | { status: "failed"; error: string };

export type DaemonUpdateCheck =
  | { status: "upToDate"; currentVersion: string }
  | { status: "available"; currentVersion: string; version: string }
  | { status: "unavailable"; reason: string };

export type DaemonUpdateStatus =
  | { status: "downloading"; received: number; total: number }
  | { status: "verifying" }
  | { status: "installing" }
  | { status: "restarting" }
  | { status: "rollingBack" }
  | { status: "ready"; version: string }
  | { status: "failed"; error: string };

export type ApplicationCleanupStatus =
  | { status: "preparing" }
  | { status: "removingServiceAndData" }
  | { status: "removingReleaseCache" }
  | { status: "ready" }
  | { status: "failed"; error: string };

export interface ApplicationCleanupResult {
  serviceAndDaemonDataRemoved: boolean;
  daemonReleaseCacheRemoved: boolean;
}

export interface WorkspaceInfo {
  displayName: string;
  isGitRepository: boolean;
}

export interface WorkspaceChange {
  path: string;
  status: string;
  staged: boolean;
  unstaged: boolean;
}

export interface WorkspaceDiff {
  path: string;
  status: string;
  before: string;
  after: string;
  comparison: string;
}

/**
 * Typed bindings for the daemon-backed Tauri commands.
 *
 * This is intentionally a direct mirror of `src-tauri/src/lib.rs`; no
 * session-era compatibility methods live here.
 */
export const daemonApi = {
  bootstrap: async (
    onStatus: (status: DaemonBootstrapStatus) => void,
  ): Promise<Health | null> => {
    const statusChannel = new Channel<DaemonBootstrapStatus>();
    statusChannel.onmessage = onStatus;
    return invoke("daemon_bootstrap", { onStatus: statusChannel });
  },
  install: async (
    onStatus: (status: DaemonBootstrapStatus) => void,
  ): Promise<Health> => {
    const statusChannel = new Channel<DaemonBootstrapStatus>();
    statusChannel.onmessage = onStatus;
    return invoke("daemon_install", { onStatus: statusChannel });
  },
  checkUpdate: (): Promise<DaemonUpdateCheck> => invoke("daemon_check_update"),
  update: async (
    onStatus: (status: DaemonUpdateStatus) => void,
  ): Promise<Health> => {
    const statusChannel = new Channel<DaemonUpdateStatus>();
    statusChannel.onmessage = onStatus;
    return invoke("daemon_update", { onStatus: statusChannel });
  },
  prepareApplicationUninstall: async (
    confirmed: boolean,
    onStatus: (status: ApplicationCleanupStatus) => void,
  ): Promise<ApplicationCleanupResult> => {
    const statusChannel = new Channel<ApplicationCleanupStatus>();
    statusChannel.onmessage = onStatus;
    return invoke("prepare_application_uninstall", {
      confirmed,
      onStatus: statusChannel,
    });
  },
  exitApplication: (): Promise<void> => invoke("exit_application"),
  health: (): Promise<Health> => invoke("daemon_health"),
  version: (): Promise<DaemonVersion> => invoke("daemon_version"),
  listWorkspaceFiles: (workspacePath: string): Promise<string[]> =>
    invoke("list_workspace_files", { workspacePath }),
  getWorkspaceInfo: (workspacePath: string): Promise<WorkspaceInfo> =>
    invoke("get_workspace_info", { workspacePath }),
  listWorkspaceChanges: (workspacePath: string): Promise<WorkspaceChange[]> =>
    invoke("list_workspace_changes", { workspacePath }),
  getWorkspaceFileDiff: (
    workspacePath: string,
    relativePath: string,
  ): Promise<WorkspaceDiff> =>
    invoke("get_workspace_file_diff", { workspacePath, relativePath }),

  listAgents: (): Promise<AgentInfo[]> => invoke("list_agents"),

  createChat: (workspacePath: string, title?: string): Promise<Chat> =>
    invoke("create_chat", { workspacePath, title }),
  listChats: (workspacePath?: string): Promise<Chat[]> =>
    invoke("list_chats", { workspacePath }),
  getChat: (chatId: string, includeMessages = true): Promise<GetChatResult> =>
    invoke("get_chat", { chatId, includeMessages }),
  deleteChat: (chatId: string): Promise<DeleteChatResult> =>
    invoke("delete_chat", { chatId }),

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
