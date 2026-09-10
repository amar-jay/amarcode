import type { RawActivityEvent } from "@/lib/activity-types";

const RUN_ID = "run-demo-auth-review";
const SESSION_ID = "session-demo-001";
const TOOL_ID = "tool-demo-tests";
const PERMISSION_ID = "permission-demo-tests";
const MESSAGE_ID = "message-demo-response";
const THOUGHT_ID = "message-demo-thought";
const BASE_TIME_MS = Date.parse("2026-09-10T11:32:08.000Z");

type EventInput = Omit<RawActivityEvent, "id" | "agentRunId" | "createdAt"> & {
  offsetMs: number;
};

function event(id: number, input: EventInput): RawActivityEvent {
  const { offsetMs, ...rest } = input;
  return {
    id,
    agentRunId: RUN_ID,
    createdAt: new Date(BASE_TIME_MS + offsetMs).toISOString(),
    ...rest,
  };
}

function sessionUpdate(
  id: number,
  offsetMs: number,
  update: Record<string, unknown>,
): RawActivityEvent {
  return event(id, {
    offsetMs,
    direction: "received",
    method: "session/update",
    payload: { sessionId: SESSION_ID, update },
  });
}

const thoughtChunks = [
  "I’ll inspect the authentication flow ",
  "and locate where refresh tokens are read. ",
  "Then I’ll run the focused tests before changing anything.",
];

const responseChunks = [
  "Updated the authentication flow. ",
  "Expired sessions now refresh once and retry the original request. ",
  "The focused test suite passes.",
];

/**
 * Sanitized ACP traffic for UI development. Payload shapes mirror real rows,
 * but identifiers, paths, prompts, outputs, and account data are fictional.
 */
export const mockRawActivityEvents: RawActivityEvent[] = [
  event(1000, {
    offsetMs: 0,
    direction: "sent",
    method: "initialize",
    payload: {
      protocolVersion: 1,
      clientInfo: { name: "amarcode-daemon", title: "Amarcode Daemon" },
      clientCapabilities: { elicitation: { form: {} } },
    },
  }),
  event(1001, {
    offsetMs: 42,
    direction: "received",
    method: "rpc.result",
    payload: {
      agentInfo: { name: "demo-agent", title: "Demo Agent", version: "1.0.0" },
      agentCapabilities: { loadSession: true },
    },
  }),
  event(1002, {
    offsetMs: 90,
    direction: "sent",
    method: "session/resume",
    payload: {
      sessionId: SESSION_ID,
      cwd: "/workspace/demo-project",
      mcpServers: [],
    },
  }),
  event(1003, {
    offsetMs: 180,
    direction: "received",
    method: "_x.ai/mcp/init_progress",
    payload: { sessionId: SESSION_ID, connected: 0, total: 2 },
  }),
  event(1004, {
    offsetMs: 340,
    direction: "received",
    method: "_x.ai/mcp/server_status",
    payload: {
      sessionId: SESSION_ID,
      name: "documentation",
      source: "local",
      status: "ready",
      reason: "initialized",
    },
  }),
  event(1005, {
    offsetMs: 510,
    direction: "received",
    method: "_x.ai/mcp/server_status",
    payload: {
      sessionId: SESSION_ID,
      name: "workspace-tools",
      source: "local",
      status: "ready",
      reason: "initialized",
    },
  }),
  event(1006, {
    offsetMs: 525,
    direction: "received",
    method: "_x.ai/mcp_initialized",
    payload: { sessionId: SESSION_ID, elapsedMs: 345, mcpToolCount: 12 },
  }),
  event(1007, {
    offsetMs: 700,
    direction: "sent",
    method: "session/set_config_option",
    payload: {
      sessionId: SESSION_ID,
      configId: "mode",
      type: "id",
      value: "agent",
    },
  }),
  sessionUpdate(1008, 730, {
    sessionUpdate: "config_option_update",
    configOptions: [{ id: "mode", currentValue: "agent" }],
  }),
  event(1009, {
    offsetMs: 900,
    direction: "sent",
    method: "session/prompt",
    payload: {
      sessionId: SESSION_ID,
      prompt: [{ type: "text", text: "Review and update the authentication flow." }],
    },
  }),
  event(1010, {
    offsetMs: 930,
    direction: "received",
    method: "_x.ai/queue/changed",
    payload: {
      sessionId: SESSION_ID,
      entries: [
        {
          id: "prompt-demo-001",
          kind: "prompt",
          position: 0,
          text: "Review and update the authentication flow.",
          version: 0,
        },
      ],
    },
  }),
  sessionUpdate(1011, 1100, {
    sessionUpdate: "plan",
    entries: [
      { content: "Inspect the authentication flow", priority: "high", status: "in_progress" },
      { content: "Run focused tests", priority: "high", status: "pending" },
      { content: "Update the implementation", priority: "medium", status: "pending" },
    ],
  }),
  ...thoughtChunks.map((text, index) =>
    sessionUpdate(1012 + index, 1300 + index * 115, {
      sessionUpdate: "agent_thought_chunk",
      messageId: THOUGHT_ID,
      content: { type: "text", text },
    }),
  ),
  sessionUpdate(1015, 1900, {
    sessionUpdate: "tool_call",
    toolCallId: "tool-demo-read",
    title: "Read src/auth/session.ts",
    kind: "read",
    status: "in_progress",
    rawInput: { path: "src/auth/session.ts" },
  }),
  sessionUpdate(1016, 2050, {
    sessionUpdate: "tool_call_update",
    toolCallId: "tool-demo-read",
    status: "completed",
    content: [{ type: "content", content: { type: "text", text: "148 lines read" } }],
  }),
  sessionUpdate(1017, 2300, {
    sessionUpdate: "tool_call",
    toolCallId: TOOL_ID,
    title: "npm test -- auth",
    kind: "execute",
    status: "in_progress",
    rawInput: { command: "npm test -- auth", cwd: "/workspace/demo-project" },
    content: [{ type: "terminal", terminalId: TOOL_ID }],
    _meta: { terminal_info: { cwd: "/workspace/demo-project", terminal_id: TOOL_ID } },
  }),
  ...["\n> test\n", "Running auth tests…\n", "3 tests passed\n"].map((data, index) =>
    sessionUpdate(1018 + index, 2500 + index * 280, {
      sessionUpdate: "tool_call_update",
      toolCallId: TOOL_ID,
      status: "in_progress",
      _meta: { terminal_output_delta: { data, terminal_id: TOOL_ID } },
    }),
  ),
  sessionUpdate(1021, 3450, {
    sessionUpdate: "tool_call_update",
    toolCallId: TOOL_ID,
    status: "completed",
    rawOutput: { exit_code: 0, formatted_output: "> test\n3 tests passed\n" },
    _meta: {
      terminal_exit: { exit_code: 0, signal: null, terminal_id: TOOL_ID },
    },
  }),
  event(1022, {
    offsetMs: 3800,
    direction: "received",
    method: "session/request_permission",
    payload: {
      sessionId: SESSION_ID,
      requestId: PERMISSION_ID,
      toolCall: {
        toolCallId: "tool-demo-edit",
        title: "Edit src/auth/session.ts",
        kind: "edit",
        rawInput: { path: "src/auth/session.ts" },
      },
      options: [
        { optionId: "allow_once", name: "Allow once", kind: "allow_once" },
        { optionId: "reject_once", name: "Reject", kind: "reject_once" },
      ],
    },
  }),
  event(1023, {
    offsetMs: 5100,
    direction: "sent",
    method: "response:session/request_permission",
    payload: { requestId: PERMISSION_ID, outcome: { outcome: "selected", optionId: "allow_once" } },
  }),
  sessionUpdate(1024, 5200, {
    sessionUpdate: "tool_call",
    toolCallId: "tool-demo-edit",
    title: "Edit src/auth/session.ts",
    kind: "edit",
    status: "in_progress",
    rawInput: { path: "src/auth/session.ts" },
  }),
  sessionUpdate(1025, 5480, {
    sessionUpdate: "tool_call_update",
    toolCallId: "tool-demo-edit",
    title: "Updated src/auth/session.ts",
    kind: "edit",
    status: "completed",
    content: [{ type: "diff", path: "src/auth/session.ts", oldText: "", newText: "" }],
  }),
  sessionUpdate(1026, 5700, {
    sessionUpdate: "plan",
    entries: [
      { content: "Inspect the authentication flow", priority: "high", status: "completed" },
      { content: "Run focused tests", priority: "high", status: "completed" },
      { content: "Update the implementation", priority: "medium", status: "completed" },
    ],
  }),
  sessionUpdate(1027, 5850, {
    sessionUpdate: "usage_update",
    used: 4821,
    size: 32000,
    cost: { amount: 0.021, currency: "USD" },
  }),
  ...responseChunks.map((text, index) =>
    sessionUpdate(1028 + index, 6100 + index * 170, {
      sessionUpdate: "agent_message_chunk",
      messageId: MESSAGE_ID,
      content: { type: "text", text },
    }),
  ),
  event(1031, {
    offsetMs: 6800,
    direction: "received",
    method: "_x.ai/session/prompt_complete",
    payload: {
      sessionId: SESSION_ID,
      promptId: "prompt-demo-001",
      stopReason: "end_turn",
      agentResult: null,
    },
  }),
  event(1032, {
    offsetMs: 7000,
    direction: "received",
    method: "_x.demo/unknown_event",
    payload: { sessionId: SESSION_ID, feature: "future-capability", enabled: true },
  }),
  sessionUpdate(1033, 7250, {
    sessionUpdate: "tool_call",
    toolCallId: "tool-demo-failed",
    title: "Read missing.config.json",
    kind: "read",
    status: "in_progress",
    rawInput: { path: "missing.config.json" },
  }),
  sessionUpdate(1034, 7420, {
    sessionUpdate: "tool_call_update",
    toolCallId: "tool-demo-failed",
    title: "Could not read missing.config.json",
    kind: "read",
    status: "failed",
    content: [{ type: "content", content: { type: "text", text: "File not found" } }],
  }),
];

