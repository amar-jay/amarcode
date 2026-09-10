import type { AcpEvent } from "@/types";

/** A persisted row from the daemon's `acp_events` table. */
export type RawActivityEvent = AcpEvent;

export type ActivityCategory =
  | "message"
  | "thinking"
  | "tool"
  | "permission"
  | "plan"
  | "usage"
  | "session"
  | "mcp"
  | "configuration"
  | "error";

export type ActivityStatus =
  "running" | "completed" | "failed" | "cancelled" | "waiting";

export type ActivityPlanEntry = {
  content: string;
  priority?: string;
  status: "pending" | "in_progress" | "completed" | string;
};

export type ActivityEventDetails = {
  command?: string;
  cwd?: string;
  output?: string;
  exitCode?: number;
  text?: string;
  reason?: string;
  decision?: string;
  paths?: string[];
  plan?: ActivityPlanEntry[];
  usage?: Record<string, number>;
  payload?: Record<string, unknown>;
};

/** A readable event derived from one or more raw ACP events. */
export type ActivityEvent = {
  id: string;
  runId: string;
  category: ActivityCategory;
  kind: string;
  title: string;
  summary?: string;
  startedAt: string;
  completedAt?: string;
  status?: ActivityStatus;
  direction?: RawActivityEvent["direction"];
  /** Source row IDs are retained so Timeline mode can always link to Protocol mode. */
  rawEventIds: number[];
  details?: ActivityEventDetails;
};
