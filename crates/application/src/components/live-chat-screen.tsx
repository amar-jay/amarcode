import { useEffect, type ReactNode } from "react";
import { useAtomValue, useSetAtom } from "jotai";
import { AlertTriangle, CircleX, LoaderCircle, Wrench, type LucideIcon } from "lucide-react";
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
import { Shimmer } from "@/components/ai-elements/shimmer";
import type { MessagePart } from "@/types";
import AppPromptInput from "./main-prompt-input";
import { PendingAgentRequestCard } from "./pending-agent-request";
import { DiffArtifactCard, diffArtifacts } from "./diff-artifact-card";
import {
  activeSessionAtom,
  applyLiveChatEventAtom,
  bindSessionAgentAtom,
  liveChatAtom,
  liveChatIsWorkingAtom,
  loadLiveChatAtom,
  openLiveChatAtom,
  respondLiveRequestAtom,
  selectedAgentAtom,
  setLiveSessionModeAtom,
  stopLiveChatAtom,
  submitLivePromptAtom,
  subscribeDaemonEvents,
  type SessionMode,
} from "@/state";

type TimelineStep = {
  key: string;
  label: string;
  description?: string;
  icon?: LucideIcon;
  status: "complete" | "active";
};

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

function TurnLoadingIndicator({ label = "Thinking" }: { label?: string }) {
  return (
    <Message from="assistant" className="space-between">
      <MessageContent className="w-full">
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <LoaderCircle className="size-3.5 shrink-0 animate-spin" />
          <Shimmer className="text-sm" duration={1.4}>
            {`${label}…`}
          </Shimmer>
        </div>
      </MessageContent>
    </Message>
  );
}

function StreamingCaret() {
  return (
    <span
      aria-hidden
      className="ml-0.5 inline-block h-[1em] w-1.5 translate-y-[0.1em] animate-pulse rounded-sm bg-foreground/70 align-text-bottom"
    />
  );
}

/**
 * Live conversation surface. State lives in `liveChatAtom` (+ navigation /
 * agent atoms); this component is render + wire-up only.
 */
export function LiveChatScreen() {
  const session = useAtomValue(activeSessionAtom);
  const live = useAtomValue(liveChatAtom);
  const isWorking = useAtomValue(liveChatIsWorkingAtom);
  const agent = useAtomValue(selectedAgentAtom) ?? session?.agent;
  const workspacePath = session?.chat.workspace_path ?? "";

  const openLiveChat = useSetAtom(openLiveChatAtom);
  const loadLiveChat = useSetAtom(loadLiveChatAtom);
  const applyEvent = useSetAtom(applyLiveChatEventAtom);
  const submitPrompt = useSetAtom(submitLivePromptAtom);
  const changeMode = useSetAtom(setLiveSessionModeAtom);
  const stop = useSetAtom(stopLiveChatAtom);
  const respond = useSetAtom(respondLiveRequestAtom);
  const bindAgent = useSetAtom(bindSessionAgentAtom);

  // Open only when chat identity changes. Seed (turn-active, mode) is read
  // from activeSessionAtom inside the write atom — don't re-open on those.
  const chatId = session?.chat.id;

  useEffect(() => {
    if (!chatId) return;
    openLiveChat(chatId);
  }, [chatId, openLiveChat]);

  // Initial + id-change load.
  useEffect(() => {
    if (!live?.chatId) return;
    void loadLiveChat(live.chatId);
  }, [live?.chatId, loadLiveChat]);

  // Every daemon event → live chat reducer (not just the latest atom value).
  useEffect(() => {
    return subscribeDaemonEvents((event) => {
      applyEvent(event);
    });
  }, [applyEvent]);

  if (!session || !live || live.chatId !== session.chat.id) {
    return (
      <main className="flex min-w-0 flex-1 items-center justify-center">
        <LoaderCircle className="size-5 animate-spin text-muted-foreground" />
      </main>
    );
  }

  const messages = live.detail?.messages ?? [];
  const lastMessage = messages[messages.length - 1]?.message;
  const showTurnPlaceholder =
    isWorking && (!lastMessage || lastMessage.role === "user");
  const waitingLabel = live.pendingRequest
    ? live.pendingRequest.kind === "approval"
      ? "Waiting for approval"
      : "Waiting for input"
    : "Thinking";

  const submit = async (text: string, mode: SessionMode) => {
    if (!agent) return;
    await submitPrompt({ text, mode, agentId: agent.id });
  };

  return (
    <main className="flex min-w-0 flex-1 flex-col">
      <header className="mr-2 flex items-center border-b px-6 py-1">
        <h1 className="truncate text-sm font-medium">
          {live.detail?.chat.title ?? session.chat.title ?? "Loading chat"}
        </h1>
        {live.loading && (
          <LoaderCircle className="ml-2 size-4 animate-spin text-muted-foreground" />
        )}
        {isWorking && <span className="ml-3 text-xs text-muted-foreground">Working…</span>}
        {!isWorking && live.runStatus && live.runStatus !== "running" && (
          <span className="ml-3 text-xs text-muted-foreground">{live.runStatus}</span>
        )}
      </header>
      <Conversation>
        <ConversationContent className="mx-auto w-full max-w-3xl gap-2 py-8">
          {messages.map(({ message, parts }) => {
            const seenTools = new Set<string>();
            const timeline: TimelineStep[] = parts
              .filter((part) => part.kind === "thinking" || part.kind === "tool_call")
              .sort((left, right) => left.ordinal - right.ordinal)
              .flatMap<TimelineStep>((part) => {
                if (part.kind === "thinking") {
                  const text = partText(part);
                  return text?.trim()
                    ? [{
                        key: `thinking-${part.ordinal}`,
                        label: text,
                        icon: undefined,
                        description: undefined,
                        status: "complete" as const,
                      }]
                    : [];
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
            const diffs = diffArtifacts(parts);
            const isStreamingAssistant =
              message.role === "assistant" &&
              (message.status === "streaming" || (isWorking && message.id === lastMessage?.id));
            const tone =
              message.role === "assistant"
                ? assistantMessageTone(message.content, message.status)
                : null;

            if (!hasVisibleContent && timeline.length === 0 && diffs.length === 0) {
              if (message.role === "assistant" && isStreamingAssistant) {
                return <TurnLoadingIndicator key={message.id} label={waitingLabel} />;
              }
              return null;
            }

            return (
              <Message
                from={message.role === "user" ? "user" : "assistant"}
                key={message.id}
                className="space-between"
              >
                <MessageContent className="w-full">
                  {timeline.length > 0 && (
                    <ChainOfThought defaultOpen={message.status === "streaming"} className="space-y-0">
                      <ChainOfThoughtHeader className="py-1">
                        {message.status === "streaming" ? (
                          <span className="inline-flex items-center gap-1.5">
                            <LoaderCircle className="size-3 animate-spin" />
                            Reasoning…
                          </span>
                        ) : (
                          "Reasoning..."
                        )}
                      </ChainOfThoughtHeader>
                      <ChainOfThoughtContent className="mt-0 space-y-1">
                        {timeline.map((step) => (
                          <ChainOfThoughtStep
                            key={`${message.id}-${step.key}`}
                            icon={step.icon}
                            label={reasoningLabel(step.label)}
                            status={step.status}
                          />
                        ))}
                      </ChainOfThoughtContent>
                    </ChainOfThought>
                  )}
                  {diffs.map((artifact) => (
                    <DiffArtifactCard key={artifact.key} artifact={artifact} />
                  ))}
                  {hasVisibleContent &&
                    (message.role === "assistant" ? (
                      tone === "error" ? (
                        <div className="flex gap-2 rounded-md px-3 py-2 text-xs text-destructive">
                          <CircleX className="mt-0.5 size-4 shrink-0" />
                          <MessageResponse>{message.content}</MessageResponse>
                        </div>
                      ) : tone === "warning" ? (
                        <div className="flex gap-2 rounded-md py-2 text-xs text-amber-700 dark:text-amber-300">
                          <AlertTriangle className="mt-0.5 size-4 shrink-0" />
                          <MessageResponse>{message.content}</MessageResponse>
                        </div>
                      ) : (
                        <div>
                          <MessageResponse>{message.content}</MessageResponse>
                          {isStreamingAssistant && <StreamingCaret />}
                        </div>
                      )
                    ) : (
                      <p className="whitespace-pre-wrap">
                        <span className="mr-2 select-none text-muted-foreground">&gt;</span>
                        {message.content}
                      </p>
                    ))}
                  {!hasVisibleContent && timeline.length > 0 && isStreamingAssistant && (
                    <div className="mt-1 flex items-center gap-2 text-sm text-muted-foreground">
                      <LoaderCircle className="size-3.5 shrink-0 animate-spin" />
                      <Shimmer className="text-sm" duration={1.4}>{`${waitingLabel}…`}</Shimmer>
                    </div>
                  )}
                </MessageContent>
              </Message>
            );
          })}
          {showTurnPlaceholder && <TurnLoadingIndicator label={waitingLabel} />}
          {!live.loading && !messages.length && !isWorking && (
            <ConversationEmptyState
              title="This chat is ready"
              description="Send a prompt to start working with your agent."
            />
          )}
        </ConversationContent>
        <ConversationScrollButton />
      </Conversation>
      <div className="border-t p-4">
        {live.pendingRequest && (
          <PendingAgentRequestCard
            request={live.pendingRequest}
            onRespond={async (result) => {
              await respond(result);
            }}
          />
        )}
        {live.error && (
          <p className="mx-auto mb-2 max-w-3xl text-sm text-destructive">{live.error}</p>
        )}
        <AppPromptInput
          workspacePath={workspacePath}
          selectedAgentId={agent?.id ?? ""}
          onAgentSelected={(next) => bindAgent(next)}
          onSendPrompt={submit}
          sessionMode={live.sessionMode}
          onSessionModeChange={(mode) => void changeMode(mode)}
          isWorking={isWorking}
          onStop={() => void stop()}
        />
      </div>
    </main>
  );
}
