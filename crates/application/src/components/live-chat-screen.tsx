import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { AlertTriangle, CircleX, LoaderCircle, Wrench, type LucideIcon } from "lucide-react";
import { daemonApi } from "@/api";
import {
  Conversation,
  ConversationContent,
  ConversationEmptyState,
  ConversationScrollButton,
} from "@/components/ai-elements/conversation";
import {
  ChainOfThought,
  ChainOfThoughtContent,
  ChainOfThoughtHeader,
  ChainOfThoughtStep,
} from "@/components/ai-elements/chain-of-thought";
import { Message, MessageContent, MessageResponse } from "@/components/ai-elements/message";
import type { AgentDefinition, Chat, ChatDetail, EditorEvent, JsonValue, MessagePart, RunStatus } from "@/types";
import AppPromptInput, { type SessionMode } from "./main-prompt-input";
import { PendingAgentRequestCard, type PendingAgentRequest } from "./pending-agent-request";
type LiveChatScreenProps = {
  workspacePath: string;
  agent: AgentDefinition | undefined;
  initialChatId: string;
  initialRunId: string | null;
  initialSessionMode?: SessionMode;
  onChatsRefresh: () => Promise<void>;
  daemonEvent: EditorEvent | null;
  onAgentSelected: (agent: AgentDefinition) => void;
};

type TimelineStep = {
  key: string;
  label: string;
  description?: string;
  icon?: LucideIcon;
  status: "complete" | "active";
};

function isChatDetail(value: Chat | ChatDetail): value is ChatDetail {
  return "messages" in value;
}

function partText(part: MessagePart): string | null {
  try {
    const value: unknown = JSON.parse(part.content_json);
    return typeof value === "object" && value !== null && "text" in value && typeof value.text === "string" && Boolean(value.text.trim())
      ? value.text
      : null;
  } catch {
    return null;
  }
}

function toolSummary(part: MessagePart): { id: string; label: string } | null {
  try {
    const value: unknown = JSON.parse(part.content_json);
    if (typeof value !== "object" || value === null) return null;
    const record = value as Record<string, unknown>;
    // Titles/names are concise agent-facing summaries. Raw command payloads,
    // arguments, and internal lifecycle updates are intentionally omitted.
    const name = [record.title, record.name, record.toolName]
      .find((candidate): candidate is string => typeof candidate === "string" && Boolean(candidate.trim()));
    if (!name) return null;
    const id = typeof record.toolCallId === "string" ? record.toolCallId : name;
    return { id, label: name };
  } catch {
    return null;
  }
}

function reasoningLabel(text: string): ReactNode {
  const fragments = text.split(/(```[\s\S]*?```|`[^`]+`)/g);
  return <div className="whitespace-pre-wrap">
    {fragments.map((fragment, index) => {
      if (fragment.startsWith("```") && fragment.endsWith("```")) {
        return <code key={index} className="my-1 block overflow-x-auto rounded bg-muted px-1.5 py-1 font-mono text-xs">{fragment.slice(3, -3).trim()}</code>;
      }
      if (fragment.startsWith("`") && fragment.endsWith("`")) {
        return <code key={index} className="rounded bg-muted px-1 py-0.5 font-mono text-[0.85em]">{fragment.slice(1, -1)}</code>;
      }
      return fragment;
    })}
  </div>;
}

function assistantMessageTone(content: string, status: string): "warning" | "error" | null {
  const text = content.trim();
  if (status === "failed" || /^(error|failed|failure|unable to|cannot |can't )/i.test(text)) return "error";
  if (/^(warning|caution|notice)/i.test(text)) return "warning";
  return null;
}

export function LiveChatScreen({ workspacePath, agent, initialChatId, initialRunId, initialSessionMode = "build", onChatsRefresh, daemonEvent, onAgentSelected }: LiveChatScreenProps) {
  const [activeChatId, setActiveChatId] = useState(initialChatId);
  const [activeChat, setActiveChat] = useState<ChatDetail | null>(null);
  const [runId, setRunId] = useState<string | null>(initialRunId);
  const [runStatus, setRunStatus] = useState<RunStatus | null>(initialRunId ? "starting" : null);
  const [pendingRequest, setPendingRequest] = useState<PendingAgentRequest | null>(null);
  const [sessionMode, setSessionMode] = useState<SessionMode>(initialSessionMode);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const activeChatIdRef = useRef(activeChatId);
  const runIdRef = useRef(runId);
  const isWorking = Boolean(runId);

  // Sidebar selection changes the parent session without remounting this
  // screen. Reset the conversation-owned state to the newly selected chat.
  useEffect(() => {
    setActiveChatId(initialChatId);
    setActiveChat(null);
    setRunId(initialRunId);
    setRunStatus(initialRunId ? "starting" : null);
    setPendingRequest(null);
    setSessionMode(initialSessionMode);
  }, [initialChatId, initialRunId, initialSessionMode]);

  useEffect(() => { activeChatIdRef.current = activeChatId; }, [activeChatId]);
  useEffect(() => { runIdRef.current = runId; }, [runId]);

  const loadChat = useCallback(async (chatId: string) => {
    setLoading(true);
    try {
      const result = await daemonApi.getChat(chatId, true);
      if (isChatDetail(result)) {
        setActiveChat(result);
        const discoveredRunId = result.messages
          .map(({ message }) => message.agent_run_id)
          .find((candidate): candidate is string => candidate !== null);
        if (discoveredRunId) {
          setRunId((current) => current ?? discoveredRunId);
          setRunStatus((current) => current ?? "running");
        }
      }
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to load this chat.");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { void loadChat(activeChatId); }, [activeChatId, loadChat]);

  const refreshTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const scheduleRefresh = useCallback(() => {
    if (refreshTimer.current) clearTimeout(refreshTimer.current);
    refreshTimer.current = setTimeout(() => void loadChat(activeChatIdRef.current), 80);
  }, [loadChat]);

  useEffect(() => () => { if (refreshTimer.current) clearTimeout(refreshTimer.current); }, []);

  useEffect(() => {
    if (!daemonEvent) return;
    if (daemonEvent.type === "chatUpdated" && daemonEvent.payload.chat_id === activeChatIdRef.current) {
      scheduleRefresh();
      void onChatsRefresh();
      return;
    }
    if (daemonEvent.type === "runUpdated" && daemonEvent.payload.run_id === runIdRef.current) {
      setRunStatus(daemonEvent.payload.status);
      scheduleRefresh();
      if (["completed", "stopped", "failed"].includes(daemonEvent.payload.status)) {
        setRunId(null);
        setPendingRequest(null);
      } else {
        setRunId(daemonEvent.payload.run_id);
      }
      return;
    }
    if ((daemonEvent.type === "approvalRequired" || daemonEvent.type === "questionRequired") && daemonEvent.payload.run_id === runIdRef.current) {
      setPendingRequest({
        kind: daemonEvent.type === "approvalRequired" ? "approval" : "input",
        requestId: daemonEvent.payload.request_id,
        details: daemonEvent.payload.details,
      });
      return;
    }
    // The daemon intentionally omits content from message events. They only
    // make the already-visible streaming state feel immediate; ChatDetail is
    // still the source of rendered message text.
    if (daemonEvent.type === "messageUpdated" || daemonEvent.type === "messagePartAdded") scheduleRefresh();
  }, [daemonEvent, onChatsRefresh, scheduleRefresh]);

  const submit = async (text: string, mode: SessionMode) => {
    if (!agent || !text.trim()) return;
    try {
      const result = await daemonApi.prompt(activeChatId, agent.id, text.trim(), mode);
      setRunId(result.run_id);
      setRunStatus("starting");
      await onChatsRefresh();
      await loadChat(activeChatId);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to send prompt.");
    }
  };

  const changeSessionMode = async (mode: SessionMode) => {
    setSessionMode(mode);
    try {
      await daemonApi.setSessionMode(activeChatId, mode);
    } catch (cause) {
      // A historical chat has no live ACP session yet. Keep the selection so
      // the next prompt applies it when that session starts.
      console.info("Session mode will apply when this chat starts:", cause);
    }
  };

  const stop = async () => {
    try {
      await daemonApi.cancel(activeChatId);
      setRunId(null);
      setRunStatus(null);
      await loadChat(activeChatId);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to cancel the run.");
    }
  };

  const respondToRequest = async (result: JsonValue) => {
    if (!pendingRequest) return;
    try {
      if (pendingRequest.kind === "approval") {
        await daemonApi.respondPermission(pendingRequest.requestId, { result });
      } else {
        await daemonApi.respondInput(pendingRequest.requestId, { result });
      }
      setPendingRequest(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to respond to the agent.");
    }
  };

  return (
    <main className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 items-center border-b px-6">
          <h1 className="truncate text-sm font-medium">{activeChat?.chat.title ?? "Loading chat"}</h1>
          {loading && <LoaderCircle className="ml-2 size-4 animate-spin text-muted-foreground" />}
          {runStatus && <span className="ml-3 text-xs text-muted-foreground">{runStatus === "running" ? "Working…" : runStatus}</span>}
        </header>
        <Conversation>
          <ConversationContent className="mx-auto w-full max-w-3xl py-8 gap-2">
            {(activeChat?.messages ?? []).map(({ message, parts }) => {
              const seenTools = new Set<string>();
              const timeline: TimelineStep[] = parts
                .filter((part) => part.kind === "thinking" || part.kind === "tool_call")
                .sort((left, right) => left.ordinal - right.ordinal)
                .flatMap<TimelineStep>((part) => {
                  if (part.kind === "thinking") {
                    const text = partText(part);
                    return text?.trim() ? [{ key: `thinking-${part.ordinal}`, label: text, icon: undefined, description: undefined, status: "complete" as const }] : [];
                  }
                  const tool = toolSummary(part);
                  if (!tool || seenTools.has(tool.id)) return [];
                  seenTools.add(tool.id);
                  return [{
                    key: `tool-${part.ordinal}`,
                    label: tool.label,
                    icon: Wrench,
                    status: message.status === "streaming" ? "active" as const : "complete" as const,
                  }];
                });
              const hasVisibleContent = Boolean(message.content.trim());
              const tone = message.role === "assistant" ? assistantMessageTone(message.content, message.status) : null;
              if (!hasVisibleContent && timeline.length === 0) return null;
              return (
              <Message from={message.role === "user" ? "user" : "assistant"} key={message.id} className="space-between">
                <MessageContent className="w-full">
                  {timeline.length > 0 && <ChainOfThought defaultOpen={false} className="space-y-0">
                    <ChainOfThoughtHeader className="py-1">Reasoning...</ChainOfThoughtHeader>
                    <ChainOfThoughtContent className="mt-0 space-y-1">
                      {timeline.map((step) => <ChainOfThoughtStep key={`${message.id}-${step.key}`} icon={step.icon} label={reasoningLabel(step.label)} status={step.status}/>)}
                    </ChainOfThoughtContent>
                  </ChainOfThought>}
                  {hasVisibleContent && (message.role === "assistant" ? (
                    tone === "error" ? <div className="flex gap-2 rounded-md text-xs px-3 py-2 text-destructive"><CircleX className="mt-0.5 size-4 shrink-0" /><MessageResponse>{message.content}</MessageResponse></div>
                    : tone === "warning" ? <div className="flex gap-2 rounded-md py-2 text-xs text-amber-700 dark:text-amber-300"><AlertTriangle className="mt-0.5 size-4 shrink-0" /><MessageResponse>{message.content}</MessageResponse></div>
                    : <MessageResponse>{message.content}</MessageResponse>
                  ) : <p className="whitespace-pre-wrap"><span className="mr-2 select-none text-muted-foreground">&gt;</span>{message.content}</p>)}
                </MessageContent>
              </Message>
              );
            })}
            {!loading && !activeChat?.messages.length && <ConversationEmptyState title="This chat is ready" description="Send a prompt to start working with your agent." />}
          </ConversationContent>
          <ConversationScrollButton />
        </Conversation>
        <div className="border-t p-4">
          {pendingRequest && <PendingAgentRequestCard request={pendingRequest} onRespond={respondToRequest} />}
          {error && <p className="mx-auto mb-2 max-w-3xl text-sm text-destructive">{error}</p>}
          <AppPromptInput
            workspacePath={workspacePath}
            selectedAgentId={agent?.id ?? ""}
            onAgentSelected={onAgentSelected}
            onSendPrompt={submit}
            sessionMode={sessionMode}
            onSessionModeChange={changeSessionMode}
            isWorking={isWorking}
            onStop={() => void stop()}
          />
        </div>
    </main>
  );
}
