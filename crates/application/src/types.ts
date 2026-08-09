/**
 * The daemon wire contract exposed by the Tauri command layer.
 *
 * Field names deliberately follow Rust/serde output (`snake_case`).  Do not
 * reshape these into view models here; UI-specific adapters belong above this
 * boundary.
 */

export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

export type Health = {
  status: string;
  version: string;
  addr: string;
};

export type DaemonVersion = {
  version: string;
};

export type AgentDefinition = {
  id: string;
  name: string;
  command: string;
  arguments: string[];
  environment: [name: string, value: string][];
  is_preset: boolean;
  created_at: string;
  updated_at: string;
};

export type Chat = {
  id: string;
  workspace_path: string;
  title: string;
  created_at: string;
  updated_at: string;
  archived_at: string | null;
};

export type MessageRole = "system" | "user" | "assistant" | "tool";
export type MessageStatus = "streaming" | "complete" | "interrupted" | "failed";
export type MessagePartKind =
  | "text"
  | "tool_call"
  | "tool_result"
  | "thinking"
  | "file"
  | "image";
export type RunStatus = "starting" | "running" | "completed" | "stopped" | "failed";

export type Message = {
  id: string;
  chat_id: string;
  agent_run_id: string | null;
  role: MessageRole;
  content: string;
  status: MessageStatus;
  created_at: string;
  updated_at: string;
};

export type MessagePart = {
  message_id: string;
  ordinal: number;
  kind: MessagePartKind;
  /** JSON encoded by the daemon; parsing is a presentation concern. */
  content_json: string;
};

export type MessageDetail = {
  message: Message;
  parts: MessagePart[];
};

export type ChatDetail = {
  chat: Chat;
  messages: MessageDetail[];
};

/** `get_chat` returns a `Chat` when `include_messages` is false. */
export type GetChatResult = Chat | ChatDetail;

export type PromptResult = {
  run_id: string;
  chat_id: string;
  agent_id: string;
  user_message_id: string;
  acp_session_id: string | null;
};

export type CancelResult = {
  cancelled: boolean;
  chat_id: string;
};

export type AgentResponseError = {
  code: number;
  message: string;
  data?: JsonValue;
};

/** Exactly one of `result` or `error` should be supplied. */
export type AgentResponse =
  | { result: JsonValue; error?: never }
  | { result?: never; error: AgentResponseError };

export type RespondAgentResult = {
  ok: boolean;
  request_id: string;
};

export type EventFilter = {
  chat_id?: string;
  run_id?: string;
  /** Reserved by the daemon; current events do not carry a session id. */
  session_id?: string;
};

export type EditorEvent =
  | { type: "chatUpdated"; payload: { chat_id: string } }
  | {
      type: "runUpdated";
      payload: {
        run_id: string;
        status: RunStatus;
        error_message: string | null;
      };
    }
  | {
      type: "messageUpdated";
      payload: { message_id: string; status: MessageStatus };
    }
  | {
      type: "messagePartAdded";
      payload: { message_id: string; ordinal: number; kind: MessagePartKind };
    }
  | {
      type: "approvalRequired";
      payload: { run_id: string; request_id: string; details: JsonValue };
    }
  | {
      type: "questionRequired";
      payload: { run_id: string; request_id: string; details: JsonValue };
    }
  | {
      type: "workspaceFilesChanged";
      payload: { workspace_path: string; paths: string[] };
    }
  | {
      type: "agentConnectionChanged";
      payload: {
        agent_id: string;
        connected: boolean;
        error_message: string | null;
      };
    };
