import type { AgentEvent } from "@/types";

export type SessionTimelineEntry =
  | { id: string; kind: "message"; role: string; text: string }
  | {
      id: string;
      kind: "work-group";
      events: Extract<AgentEvent, { kind: "activity" }>[];
    }
  | {
      id: string;
      kind: "approval";
      event: Extract<AgentEvent, { kind: "request" }>;
    }
  | { id: string; kind: "error"; message: string };

const isTimelineActivity = (
  event: AgentEvent,
): event is Extract<AgentEvent, { kind: "activity" }> =>
  event.kind === "activity" &&
  !["response", "session update"].includes(event.data.label);

export function deriveSessionTimeline(
  events: AgentEvent[],
): SessionTimelineEntry[] {
  const entries: SessionTimelineEntry[] = [];
  let assistant: {
    id: string;
    sessionId: string;
    role: string;
    text: string;
  } | null = null;
  let work: Extract<AgentEvent, { kind: "activity" }>[] = [];
  const flushAssistant = () => {
    const pending = assistant;
    if (!pending) return;
    entries.push({
      id: pending.id,
      kind: "message",
      role: pending.role,
      text: pending.text,
    });
    assistant = null;
  };
  const flushWork = () => {
    if (work.length) {
      entries.push({
        id: `work-${entries.length}`,
        kind: "work-group",
        events: work,
      });
      work = [];
    }
  };
  events.forEach((event, index) => {
    if (event.kind === "message" && event.data.role !== "user") {
      if (
        assistant !== null &&
        assistant.sessionId === event.data.sessionId &&
        assistant.role === event.data.role
      )
        assistant.text += event.data.text;
      else {
        flushAssistant();
        flushWork();
        assistant = {
          id: `assistant-${index}`,
          sessionId: event.data.sessionId,
          role: event.data.role,
          text: event.data.text,
        };
      }
      return;
    }
    flushAssistant();
    if (isTimelineActivity(event)) {
      work.push(event);
      return;
    }
    flushWork();
    if (event.kind === "message") {
      const previous = entries.at(-1);
      if (
        event.data.role === "user" &&
        previous?.kind === "message" &&
        previous.role === "user" &&
        previous.text === event.data.text
      )
        return;
      entries.push({
        id: `user-${index}`,
        kind: "message",
        role: event.data.role,
        text: event.data.text,
      });
    }
    if (event.kind === "request")
      entries.push({ id: `approval-${index}`, kind: "approval", event });
    if (event.kind === "protocolError")
      entries.push({
        id: `error-${index}`,
        kind: "error",
        message: event.data.message,
      });
  });
  flushAssistant();
  flushWork();
  return entries;
}
