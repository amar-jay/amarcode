import { useCallback, useMemo, useRef, useState } from "react";
import { daemonApi } from "@/api";
import type { AgentDefinition, AgentEvent, SessionSummary } from "@/types";
import type { WorkMode } from "@/components/prompt-input";
import { notify } from "@/lib/notify";
import {
  acknowledgeOptimisticPrompt,
  cacheOptimisticPrompt,
  restoreOptimisticPrompts,
} from "@/lib/optimistic-prompts";

type PromptInput = {
  text: string;
  files: { filename?: string }[];
  sources: { title?: string; filename?: string }[];
  mode: WorkMode;
};

export function useSessionController(agents: AgentDefinition[]) {
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [activeSession, setActiveSession] = useState<SessionSummary>();
  const [events, setEvents] = useState<AgentEvent[]>([]);
  const [isLaunching, setIsLaunching] = useState(false);
  const [isPromptWorking, setIsPromptWorking] = useState(false);
  const [error, setError] = useState("");
  const isLaunchingRef = useRef(false);
  const isPromptDispatchingRef = useRef(false);

  const activeAgent = useMemo(
    () => agents.find((agent) => agent.id === activeSession?.agentId),
    [activeSession?.agentId, agents],
  );

  const onEvent = useCallback((event: AgentEvent) => {
    if (
      event.kind === "turnComplete" ||
      event.kind === "protocolError" ||
      (event.kind === "status" &&
        ["failed", "stopped"].includes(event.data.status))
    ) {
      isPromptDispatchingRef.current = false;
      setIsPromptWorking(false);
    }
    if (event.kind === "protocolError") {
      setError(event.data.message);
      notify(event.data.message, "error");
    }
    if (event.kind === "status") {
      setActiveSession((current) =>
        current?.id === event.data.sessionId
          ? { ...current, status: event.data.status }
          : current,
      );
      setSessions((current) =>
        current.map((session) =>
          session.id === event.data.sessionId
            ? { ...session, status: event.data.status }
            : session,
        ),
      );
    }

    const acknowledged =
      event.kind === "message" && event.data.role === "user"
        ? acknowledgeOptimisticPrompt(event.data.sessionId, event.data.text)
        : undefined;
    setEvents((current) => reconcileEvent(current, event, acknowledged?.id));
  }, []);

  const startSession = useCallback(
    async (workspacePath: string, agent?: AgentDefinition) => {
      if (!workspacePath || !agent || isLaunchingRef.current) return;
      isLaunchingRef.current = true;
      setIsLaunching(true);
      setError("");
      setEvents([]);
      try {
        const session = await daemonApi.createChat(workspacePath);
        // setActiveSession(session);
        // setSessions((current) => [session, ...current]);
        notify(`${agent.name} session started`, "success");
      } catch (reason) {
        setError(String(reason));
        notify("Unable to start the agent session", "error");
      } finally {
        isLaunchingRef.current = false;
        setIsLaunching(false);
      }
    },
    [onEvent],
  );

  const restoreSession = useCallback(async (session: SessionSummary) => {
    try {
      setActiveSession(session);
      // setEvents(
        // restoreOptimisticPrompts(session.id, await api.events(session.id)),
      // );
      setError("");
    } catch (reason) {
      setError(String(reason));
      notify("Could not restore that session", "error");
    }
  }, []);

  const submitPrompt = useCallback(
    async ({ text, files, sources }: PromptInput) => {
      if (
        !activeSession ||
        (!text && files.length === 0) ||
        isPromptDispatchingRef.current
      )
        return;
      if (activeSession.status !== "running") {
        notify(
          "This saved session is no longer live. Start a new session to continue.",
          "error",
        );
        return;
      }
      const attachmentNames = files.map(
        (file) => file.filename ?? "Attachment",
      );
      const visibleText =
        text || `Review the attached context: ${attachmentNames.join(", ")}`;
      const sourceNames = sources.map(
        (source) => source.title ?? source.filename ?? "Workspace context",
      );
      const prompt = [
        attachmentNames.length
          ? `[Local context attachments: ${attachmentNames.join(", ")}]`
          : "",
        sourceNames.length
          ? `[Referenced workspace context: ${sourceNames.join(", ")}]`
          : "",
        visibleText,
      ]
        .filter(Boolean)
        .join("\n\n");
      const promptId = crypto.randomUUID();
      isPromptDispatchingRef.current = true;
      cacheOptimisticPrompt({
        id: promptId,
        sessionId: activeSession.id,
        text: visibleText,
        afterAgentMessageCount: events.filter(
          (event) => event.kind === "message" && event.data.role !== "user",
        ).length,
      });
      setEvents((current) => [
        ...current,
        {
          kind: "message",
          data: {
            sessionId: activeSession.id,
            role: "user",
            text: visibleText,
            clientId: promptId,
          },
        },
      ]);
      setIsPromptWorking(true);
      try {
        // await api.prompt(activeSession.id, prompt, visibleText);
      } catch (reason) {
        isPromptDispatchingRef.current = false;
        setIsPromptWorking(false);
        setError(String(reason));
        notify("Prompt could not be sent", "error");
        throw reason;
      }
    },
    [activeSession, events],
  );

  const stopPrompt = useCallback(async () => {
    if (!activeSession || activeSession.status !== "running") return;
    try {
      // await api.cancel(activeSession.id);
      isPromptDispatchingRef.current = false;
      setIsPromptWorking(false);
      notify("Agent run cancelled", "success");
    } catch (reason) {
      setError(String(reason));
      notify("Could not cancel the agent", "error");
    }
  }, [activeSession]);

  async function respondToRequest(event: AgentEvent, result: unknown) {
    if (!activeSession || event.kind !== "request") return;
    // await api.respond(activeSession.id, event.data.requestId, result);
  }

  return {
    activeAgent,
    activeSession,
    error,
    events,
    isLaunching,
    isPromptWorking,
    restoreSession,
    respondToRequest,
    sessions,
    setActiveSession,
    setSessions,
    startSession,
    stopPrompt,
    submitPrompt,
  };
}

function reconcileEvent(
  events: AgentEvent[],
  event: AgentEvent,
  acknowledgedPromptId?: string,
) {
  if (event.kind !== "message" || event.data.role !== "user") {
    return [...events, event];
  }
  const optimisticIndex = acknowledgedPromptId
    ? events.findIndex(
        (existing) =>
          existing.kind === "message" &&
          existing.data.clientId === acknowledgedPromptId,
      )
    : -1;
  if (optimisticIndex >= 0) {
    const next = [...events];
    next[optimisticIndex] = event;
    return next;
  }
  const isDuplicate = events.some(
    (existing) =>
      existing.kind === "message" &&
      existing.data.role === "user" &&
      existing.data.sessionId === event.data.sessionId &&
      existing.data.text === event.data.text,
  );
  return isDuplicate ? events : [...events, event];
}
