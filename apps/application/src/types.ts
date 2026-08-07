export type AgentDefinition = {
  id: string;
  name: string;
  command: string;
  arguments: string[];
  environment: { name: string; secretRef?: string; value?: string }[];
  isPreset: boolean;
};

export type SessionSummary = { id: string; workspacePath: string; agentId: string; status: string; createdAt: string; updatedAt: string };

export type AgentEvent =
  | { kind: "status"; data: { sessionId: string; status: string; detail?: string } }
  | { kind: "message"; data: { sessionId: string; role: string; text: string } }
  | { kind: "activity"; data: { sessionId: string; label: string; payload: unknown } }
  | { kind: "request"; data: { sessionId: string; requestId: string | number; method: string; params: unknown } }
  | { kind: "protocolError"; data: { sessionId: string; message: string } }
  | { kind: "turnComplete"; data: { sessionId: string } };
