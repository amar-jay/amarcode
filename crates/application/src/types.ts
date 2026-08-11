/**
 * Generated daemon wire contract plus the small convenience input types used
 * only by the React API wrapper. Regenerate with `bun run protocol:generate`.
 */

export { PROTOCOL_VERSION } from "./generated/protocol";
export type * from "./generated/protocol";

import type {
  HealthResult,
  JsonValue,
  PromptResultDto,
  VersionResult,
} from "./generated/protocol";

export type Health = HealthResult;
export type DaemonVersion = VersionResult;
export type PromptResult = PromptResultDto;

export type AgentResponseError = {
  code: number;
  message: string;
  data?: JsonValue;
};

/** Exactly one of `result` or `error` should be supplied. */
export type AgentResponse =
  | { result: JsonValue; error?: never }
  | { result?: never; error: AgentResponseError };

/** Ergonomic optional filter; Tauri/serde supplies omitted fields as null. */
export type EventFilter = {
  chat_id?: string;
  run_id?: string;
  session_id?: string;
};
