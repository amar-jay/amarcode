import { isWeb } from "@/lib/platform";
import { browserInvoke, browserEvents } from "@/lib/browser-daemon";
import type {
  AmarcodeAcpConfigResult,
  SetAmarcodeAcpConfigParams,
} from "./generated/protocol";
import { Channel, invoke as nativeInvoke } from "@tauri-apps/api/core";
import type {
  AgentInfo,
  DaemonBootstrapStatus,
  DaemonUpdateCheck,
  DaemonUpdateStatus,
  WorkspaceInfo,
  WorkspaceSearchResult,
  WorkspaceChange,
  WorkspaceDiff,
  AcpEvent,
  AgentRun,
  AgentResponse,
  CancelResult,
  Chat,
  DeleteChatResult,
  DaemonVersion,
  DaemonConfigResult,
  VacuumDatabaseResult,
  EditorEvent,
  EventFilter,
  GetAttachmentResult,
  GetChatResult,
  MessagePart,
  Health,
  PromptResult,
  PromptAttachment,
  RespondAgentResult,
  TranscribeAudioResult,
  SessionCommands,
  SessionConfigAssignment,
  SessionConfigOption,
  SessionConfigValue,
} from "./types";

const invoke = <T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> =>
  isWeb ? browserInvoke<T>(command, args) : nativeInvoke<T>(command, args);

export type DaemonEventStreamStatus =
  { status: "connected" } | { status: "disconnected"; error: string };

export type {
  DaemonBootstrapStatus,
  DaemonUpdateCheck,
  DaemonUpdateStatus,
} from "./types";

export type AppUpdateCheck =
  | { status: "upToDate"; currentVersion: string }
  | {
      status: "available";
      currentVersion: string;
      version: string;
      notes: string | null;
    };

export type AppUpdateStatus =
  | { status: "downloading"; received: number; total: number }
  | { status: "installing" }
  | { status: "restarting" }
  | { status: "failed"; error: string };

export const appUpdateApi = {
  check: (): Promise<AppUpdateCheck> => invoke("app_check_update"),
  install: async (
    onStatus: (status: AppUpdateStatus) => void,
  ): Promise<void> => {
    const statusChannel = new Channel<AppUpdateStatus>();
    statusChannel.onmessage = onStatus;
    return invoke("app_install_update", { onStatus: statusChannel });
  },
};

export type DaemonRemovalStatus =
  | { status: "preparing" }
  | { status: "removingServiceAndData" }
  | { status: "removingReleaseCache" }
  | { status: "ready" }
  | { status: "failed"; error: string };

export interface DaemonRemovalResult {
  serviceAndDaemonDataRemoved: boolean;
  daemonReleaseCacheRemoved: boolean;
}

export type { WorkspaceInfo, WorkspaceChange, WorkspaceDiff } from "./types";

/**
 * Typed bindings for the daemon-backed Tauri commands.
 *
 * This is intentionally a direct mirror of `src-tauri/src/commands/`; no
 * session-era compatibility methods live here.
 */
export type SshConnectionConfig = {
  host: string;
  user: string | null;
  port: number | null;
  identityFile: string | null;
  daemonPort: number;
};

export type DaemonConnectionInfo = {
  remote: boolean;
  url: string;
  hasToken: boolean;
  mode: "local" | "websocket" | "ssh";
  ssh: SshConnectionConfig | null;
  scope: string;
};

export type DaemonConnectionStatus = {
  status: "idle" | "connecting" | "connected" | "reconnecting";
  error: string | null;
  reconnectAttempt: number;
  retryInMs: number | null;
  generation: number;
};

export const daemonApi = {
  getWorkspaceLaunch: (): Promise<{ id: number; path: string } | null> =>
    invoke("get_workspace_launch"),
  acknowledgeWorkspaceLaunch: (id: number): Promise<void> =>
    invoke("acknowledge_workspace_launch", { id }),
  getConnectionStatus: (): Promise<DaemonConnectionStatus> =>
    invoke("get_daemon_connection_status"),
  getConnection: (): Promise<DaemonConnectionInfo> =>
    invoke("get_daemon_connection"),
  setConnection: (url: string, token: string): Promise<DaemonConnectionInfo> =>
    invoke("set_daemon_connection", { url, token }),
  bootstrap: async (
    onStatus: (status: DaemonBootstrapStatus) => void,
  ): Promise<Health | null> => {
    if (isWeb)
      return browserInvoke<Health>("daemon_bootstrap", {
        onStatus: { onmessage: onStatus },
      });
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
    version: string,
    onStatus: (status: DaemonUpdateStatus) => void,
  ): Promise<Health> => {
    const statusChannel = new Channel<DaemonUpdateStatus>();
    statusChannel.onmessage = onStatus;
    return invoke("daemon_update", { version, onStatus: statusChannel });
  },
  removeLocalDaemon: async (
    confirmation: string,
    onStatus: (status: DaemonRemovalStatus) => void,
  ): Promise<DaemonRemovalResult> => {
    const statusChannel = new Channel<DaemonRemovalStatus>();
    statusChannel.onmessage = onStatus;
    return invoke("remove_local_daemon", {
      confirmation,
      onStatus: statusChannel,
    });
  },
  exitApplication: (): Promise<void> => invoke("exit_application"),
  health: (): Promise<Health> => invoke("daemon_health"),
  version: (): Promise<DaemonVersion> => invoke("daemon_version"),
  searchWorkspaceContent: (
    workspacePath: string,
    query: string,
    caseSensitive = false,
    regex = false,
  ): Promise<WorkspaceSearchResult> =>
    invoke("search_workspace_content", {
      workspacePath,
      query,
      caseSensitive,
      regex,
    }),
  listWorkspaceFiles: (
    workspacePath: string,
    query?: string,
  ): Promise<string[]> =>
    invoke("list_workspace_files", { workspacePath, query }),
  getWorkspaceInfo: (workspacePath: string): Promise<WorkspaceInfo> =>
    invoke("get_workspace_info", { workspacePath }),
  listWorkspaceChanges: (workspacePath: string): Promise<WorkspaceChange[]> =>
    invoke("list_workspace_changes", { workspacePath }),
  stageWorkspaceFile: (
    workspacePath: string,
    relativePath: string,
  ): Promise<void> =>
    invoke("stage_workspace_file", { workspacePath, relativePath }),
  unstageWorkspaceFile: (
    workspacePath: string,
    relativePath: string,
  ): Promise<void> =>
    invoke("unstage_workspace_file", { workspacePath, relativePath }),
  stageAllWorkspaceFiles: (workspacePath: string): Promise<void> =>
    invoke("stage_all_workspace_files", { workspacePath }),
  commitWorkspaceChanges: (
    workspacePath: string,
    message: string,
  ): Promise<void> =>
    invoke("commit_workspace_changes", { workspacePath, message }),
  getWorkspaceFileDiff: (
    workspacePath: string,
    relativePath: string,
    comparison?: "staged" | "unstaged",
  ): Promise<WorkspaceDiff> =>
    invoke("get_workspace_file_diff", {
      workspacePath,
      relativePath,
      staged: comparison === undefined ? undefined : comparison === "staged",
    }),

  readWorkspaceFile: (
    workspacePath: string,
    relativePath: string,
  ): Promise<string> =>
    invoke("read_workspace_file", { workspacePath, relativePath }),
  saveWorkspaceFile: (
    workspacePath: string,
    relativePath: string,
    expectedContent: string,
    content: string,
  ): Promise<void> =>
    invoke("save_workspace_file", {
      workspacePath,
      relativePath,
      expectedContent,
      content,
    }),

  listAgents: (): Promise<AgentInfo[]> => invoke("list_agents"),
  installAgent: (
    agentId: string,
  ): Promise<{
    agent: AgentInfo;
    runtime_status: "ready" | "auth_required" | "unavailable" | "error";
    runtime_message?: string | null;
  }> => invoke("install_agent", { agentId }),
  authenticateAgent: (
    agentId: string,
    methodId?: string,
  ): Promise<{ ok: boolean }> =>
    invoke("authenticate_agent", { agentId, methodId }),
  getAmarcodeAcpConfig: (): Promise<AmarcodeAcpConfigResult> =>
    invoke("get_amarcode_acp_config"),
  updateAmarcodeAcpProfile: (
    params: SetAmarcodeAcpConfigParams,
  ): Promise<AmarcodeAcpConfigResult> =>
    invoke("set_amarcode_acp_config", { params }),

  discoverCommands: (
    chatId: string,
    agentId: string,
  ): Promise<SessionCommands> =>
    invoke("discover_commands", { chatId, agentId }),

  createChat: (workspacePath: string, title?: string): Promise<Chat> =>
    invoke("create_chat", { workspacePath, title }),
  listChats: (workspacePath?: string): Promise<Chat[]> =>
    invoke("list_chats", { workspacePath }),
  getChat: (
    chatId: string,
    includeMessages = true,
    includeToolContent = false,
    page?: { messageLimit: number; beforeMessageId?: string },
  ): Promise<GetChatResult> =>
    invoke("get_chat", {
      chatId,
      includeMessages,
      includeToolContent,
      ...page,
    }),
  getMessageParts: (messageIds: string[]): Promise<MessagePart[]> =>
    invoke("get_message_parts", { messageIds }),
  listAcpEventsForChat: (chatId: string): Promise<AcpEvent[]> =>
    invoke("list_acp_events_for_chat", { chatId }),
  listAgentRunsForChat: (chatId: string): Promise<AgentRun[]> =>
    invoke("list_agent_runs_for_chat", { chatId }),
  getDaemonConfig: (): Promise<DaemonConfigResult> =>
    invoke("get_daemon_config"),
  setDaemonConfig: (
    storeAcpEvents: boolean,
    acpEventRetentionDays: number,
  ): Promise<DaemonConfigResult> =>
    invoke("set_daemon_config", {
      params: {
        store_acp_events: storeAcpEvents,
        acp_event_retention_days: acpEventRetentionDays,
      },
    }),
  vacuumDatabase: (): Promise<VacuumDatabaseResult> =>
    invoke("vacuum_database"),
  getAttachment: (
    chatId: string,
    attachmentId: string,
  ): Promise<GetAttachmentResult> =>
    invoke("get_attachment", { chatId, attachmentId }),
  updateChat: (chatId: string, changes: { title: string }): Promise<Chat> =>
    invoke("update_chat", { params: { chat_id: chatId, ...changes } }),
  transcribeAudio: (
    audioBase64: string,
    mimeType: string,
    language?: string,
  ): Promise<TranscribeAudioResult> =>
    invoke("transcribe_audio", {
      params: {
        audio_base64: audioBase64,
        mime_type: mimeType,
        language: language ?? null,
      },
    }),
  deleteChat: (chatId: string): Promise<DeleteChatResult> =>
    invoke("delete_chat", { chatId }),

  enqueuePrompt: (
    requestId: string,
    chatId: string,
    agentId: string,
    text: string,
    attachments: PromptAttachment[],
    configValues: SessionConfigAssignment[] = [],
  ): Promise<import("@/types").PromptQueueState> =>
    invoke("enqueue_prompt", {
      params: {
        request_id: requestId,
        prompt: {
          chat_id: chatId,
          agent_id: agentId,
          text,
          attachments,
          config_values: configValues,
        },
      },
    }),
  updatePromptQueue: (
    chatId: string,
    action: "pause" | "resume" | "remove" | "edit",
    id?: string,
    text?: string,
  ): Promise<import("@/types").PromptQueueState> =>
    invoke("update_prompt_queue", {
      params: { chat_id: chatId, action, id, text },
    }),
  prompt: (
    chatId: string,
    agentId: string,
    text: string,
    attachments: PromptAttachment[],
    configValues?: SessionConfigAssignment[],
  ): Promise<PromptResult> =>
    invoke("prompt", { chatId, agentId, text, attachments, configValues }),
  setSessionConfigOption: (
    chatId: string,
    configId: string,
    value: SessionConfigValue,
  ): Promise<{ chat_id: string; options: SessionConfigOption[] }> =>
    invoke("set_session_config_option", { chatId, configId, value }),
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
    if (isWeb) return browserEvents(filter, onEvent, onStatus);
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
