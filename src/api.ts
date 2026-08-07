import { Channel, invoke } from "@tauri-apps/api/core";
import type { AgentDefinition, AgentEvent, SessionSummary } from "./types";

export const api = {
  agents: () => invoke<AgentDefinition[]>("list_agents"),
  sessions: () => invoke<SessionSummary[]>("list_sessions"),
  events: (sessionId: string) => invoke<AgentEvent[]>("session_events", { sessionId }),
  saveAgent: (agent: AgentDefinition) => invoke("save_agent", { agent }),
  start: async (workspacePath: string, agent: AgentDefinition, onEvent: (event: AgentEvent) => void) => {
    const channel = new Channel<AgentEvent>();
    channel.onmessage = onEvent;
    return invoke<SessionSummary>("start_session", { input: { workspacePath, agent }, onEvent: channel });
  },
  prompt: (sessionId: string, prompt: string) => invoke("send_prompt", { sessionId, prompt }),
  cancel: (sessionId: string) => invoke("cancel_session", { sessionId }),
  respond: (sessionId: string, requestId: string, result: unknown) => invoke("respond_to_request", { input: { sessionId, requestId, result } }),
};
