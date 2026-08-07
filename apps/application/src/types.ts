/**
 * Persisted agent configuration used by the UI and daemon.
 * The daemon stores these definitions in the agents table and launches the
 * selected command with the configured arguments and environment.
 */
export type AgentDefinition = {
  /** Stable agent id used for persistence and lookups. */
  id: string;

  /** Human-readable label shown in the agent picker. */
  name: string;

  /** Executable the daemon starts for this agent. */
  command: string;

  /** Command-line arguments appended to the executable. */
  arguments: string[];

  /**
   * Environment variables passed to the process.
   * A variable can use a literal value or point at a secret reference that the
   * daemon resolves at launch time.
   */
  environment: {
    /** Environment variable name, for example API_KEY or RUST_LOG. */
    name: string;
    /** Optional secret store reference used instead of a literal value. */
    secretRef?: string;
    /** Literal value written directly into the child process environment. */
    value?: string;
  }[];

  /** True for built-in seeded agents; false for user-created agents. */
  isPreset: boolean;
};

/**
 * Lightweight session record returned by list and restore calls.
 * It mirrors the daemon's session summary table and intentionally omits the
 * full event history.
 */
export type SessionSummary = {
  /** Session id used to fetch events and send follow-up requests. */
  id: string;

  /** Workspace root the session is attached to. */
  workspacePath: string;

  /** Agent id that launched the session. */
  agentId: string;

  /** Current lifecycle state reported by the daemon. */
  status: string;

  /** RFC 3339 timestamp for when the session was created. */
  createdAt: string;

  /** RFC 3339 timestamp for the last status change. */
  updatedAt: string;
};

/**
 * Events streamed from the daemon while a session is running.
 * The union is deliberately discriminated by `kind` so the UI can render
 * messages, approvals, errors, and turn boundaries with separate code paths.
 */
export type AgentEvent =
  /** Session lifecycle changes such as starting, running, paused, or stopped. */
  | {
      kind: "status";
      data: {
        /** Session that produced the status update. */
        sessionId: string;
        /** New lifecycle state reported by the daemon. */
        status: string;
        /** Optional extra context shown in the UI. */
        detail?: string;
      };
    }
  /** Conversation messages from the user or assistant. */
  | {
      kind: "message";
      data: {
        /** Session that produced the message. */
        sessionId: string;
        /** Message author; the UI treats non-user messages as assistant output. */
        role: string;
        /** Text content for the transcript and timeline. */
        text: string;
        /**
         * Local optimistic prompt id used to reconcile a user message with the
         * eventual server-acknowledged copy.
         */
        clientId?: string;
      };
    }
  /** Non-conversational activity records that feed the timeline/work groups. */
  | {
      kind: "activity";
      data: {
        /** Session that emitted the activity. */
        sessionId: string;
        /** Short label used for grouping and display. */
        label: string;
        /** Opaque payload carried through for debugging or richer UI details. */
        payload: unknown;
      };
    }
  /** Tool or approval requests that wait for a response from the UI. */
  | {
      kind: "request";
      data: {
        /** Session that owns the request. */
        sessionId: string;
        /** Request id returned back to the daemon when responding. */
        requestId: string | number;
        /** Tool or RPC method being requested. */
        method: string;
        /** Opaque request payload passed through from the runtime. */
        params: unknown;
      };
    }
  /** Transport or protocol failures that should be surfaced as errors. */
  | {
      kind: "protocolError";
      data: {
        /** Session that encountered the failure. */
        sessionId: string;
        /** Human-readable error message. */
        message: string;
      };
    }
  /** Turn boundary marker emitted after the assistant finishes responding. */
  | {
      kind: "turnComplete";
      data: {
        /** Session that finished the turn. */
        sessionId: string;
      };
    };
