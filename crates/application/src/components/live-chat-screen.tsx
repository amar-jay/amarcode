import { useEffect, useMemo, type ReactNode } from "react";
import { useAtomValue, useSetAtom } from "jotai";
import { AlertTriangle, CircleX, LoaderCircle, Wrench, type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
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
import type { MessageDetail, MessagePart } from "@/types";
import AppPromptInput from "./main-prompt-input";
import { PendingAgentRequestCard } from "./pending-agent-request";
import { DiffArtifactCard, diffArtifacts, type DiffArtifact } from "./diff-artifact-card";
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
  verboseReasoningAtom,
  type SessionMode,
} from "@/state";

type TimelineStep = {
  key: string;
  label: string;
  description?: string;
  icon?: LucideIcon;
  status: "complete" | "active";
};

/** One user bubble, or one assistant turn (many ACP messages collapsed). */
type ChatBlock =
  | { kind: "user"; key: string; item: MessageDetail }
  | {
      kind: "assistant";
      key: string;
      items: MessageDetail[];
      content: string;
      streaming: boolean;
      status: string;
      timeline: TimelineStep[];
      diffs: DiffArtifact[];
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

/** Prefer short tool titles; raw shell one-liners become "Run command" unless verbose. */
function toolSummary(
  part: MessagePart,
  verbose: boolean,
): { id: string; label: string } | null {
  try {
    const value: unknown = JSON.parse(part.content_json);
    if (typeof value !== "object" || value === null) return null;
    const record = value as Record<string, unknown>;
    const raw = [record.title, record.name, record.toolName]
      .find((candidate): candidate is string => typeof candidate === "string" && Boolean(candidate.trim()));
    if (!raw) return null;
    const id = typeof record.toolCallId === "string" ? record.toolCallId : raw;
    const kind = typeof record.kind === "string" ? record.kind : "";
    let label = raw.trim();

    // In verbose mode, surface rawInput command when title is generic.
    if (verbose) {
      const rawInput =
        typeof record.rawInput === "object" && record.rawInput !== null && !Array.isArray(record.rawInput)
          ? (record.rawInput as Record<string, unknown>)
          : null;
      const command =
        (rawInput && typeof rawInput.command === "string" && rawInput.command) ||
        (typeof record.command === "string" && record.command) ||
        null;
      if (command && command.trim() && (label.length < 8 || /^(run|execute|bash|shell|command)/i.test(label))) {
        label = command.trim();
      }
      return { id, label };
    }

    // Compact: long shell / multi-command titles are noise in the chain.
    if (label.length > 72 || /\n/.test(label) || /&&|\|\||;/.test(label)) {
      label =
        kind === "execute" || /^(bash|shell|run|command|terminal)/i.test(raw)
          ? "Run command"
          : kind
            ? kind.replace(/[_-]+/g, " ")
            : "Tool call";
    }
    return { id, label };
  } catch {
    return null;
  }
}

function reasoningLabel(text: string, verbose: boolean): ReactNode {
  const display = verbose || text.length <= 320 ? text : `${text.slice(0, 320)}…`;
  const fragments = display.split(/(```[\s\S]*?```|`[^`]+`)/g);
  return (
    <div className="whitespace-pre-wrap">
      {fragments.map((fragment, index) => {
        if (fragment.startsWith("```") && fragment.endsWith("```")) {
          return (
            <code
              key={index}
              className={cn(
                "my-1 block overflow-auto rounded bg-muted px-1.5 py-1 font-mono text-xs",
                verbose ? "max-h-96" : "max-h-40",
              )}
            >
              {fragment.slice(3, -3).trim()}
            </code>
          );
        }
        if (fragment.startsWith("`") && fragment.endsWith("`")) {
          return (
            <code key={index} className="rounded bg-muted px-1 py-0.5 font-mono text-[0.85em]">
              {fragment.slice(1, -1)}
            </code>
          );
        }
        return fragment;
      })}
    </div>
  );
}

function assistantMessageTone(content: string, status: string): "warning" | "error" | null {
  const text = content.trim();
  if (status === "failed" || /^(error|failed|failure|unable to|cannot |can't )/i.test(text)) return "error";
  if (/^(warning|caution|notice)/i.test(text)) return "warning";
  return null;
}

/** Hide the exact prompt echo produced by older daemon versions. */
function removeLeadingPromptEcho(content: string, prompt: string): string {
  const echoedPrompt = prompt.trim();
  return echoedPrompt && content.startsWith(echoedPrompt)
    ? content.slice(echoedPrompt.length).trimStart()
    : content;
}

/** Keep an initial notice distinct from the assistant's actual response. */
function splitLeadingWarning(content: string): { warning: string; response: string } | null {
  const paragraphs = content.trim().split(/\n\s*\n/);
  const [warning, ...response] = paragraphs;

  if (!warning || !response.length || !/^(warning|caution|notice)/i.test(warning)) {
    return null;
  }

  return { warning, response: response.join("\n\n") };
}

function buildTimeline(
  items: MessageDetail[],
  streaming: boolean,
  verbose: boolean,
): TimelineStep[] {
  const seenTools = new Set<string>();
  const steps: TimelineStep[] = [];

  for (const { message, parts } of items) {
    const ordered = parts
      .filter((part) => part.kind === "thinking" || part.kind === "tool_call")
      .sort((left, right) => left.ordinal - right.ordinal);

    for (const part of ordered) {
      if (part.kind === "thinking") {
        const text = partText(part);
        if (!text?.trim()) continue;
        // Compact mode: skip very short status-style thoughts that only add noise.
        if (!verbose && text.trim().length < 8) continue;
        steps.push({
          key: `${message.id}-thinking-${part.ordinal}`,
          label: text,
          status: "complete",
        });
        continue;
      }
      const tool = toolSummary(part, verbose);
      if (!tool || seenTools.has(tool.id)) continue;
      seenTools.add(tool.id);
      steps.push({
        key: `${message.id}-tool-${part.ordinal}`,
        label: tool.label,
        icon: Wrench,
        status: streaming && message.status === "streaming" ? "active" : "complete",
      });
    }
  }
  return steps;
}

/**
 * ACP may emit several assistant messages per turn (commentary vs final, or
 * per-messageId streams). Render them as one bubble so the UI doesn't look
 * like interleaved "Reasoning…" islands.
 */
function groupChatBlocks(
  messages: MessageDetail[],
  turnWorking: boolean,
  verbose: boolean,
): ChatBlock[] {
  const blocks: ChatBlock[] = [];

  for (const item of messages) {
    if (item.message.role === "user") {
      blocks.push({ kind: "user", key: item.message.id, item });
      continue;
    }
    if (item.message.role !== "assistant") continue;

    // Skip empty assistant shells — they only produce orphan "Thinking…" rows.
    const hasText = Boolean(item.message.content.trim());
    const hasParts = item.parts.some(
      (part) => part.kind === "thinking" || part.kind === "tool_call",
    );
    const hasDiff = diffArtifacts(item.parts).length > 0;
    if (!hasText && !hasParts && !hasDiff) continue;

    const runId = item.message.agent_run_id;
    const prev = blocks[blocks.length - 1];
    const canMerge =
      prev?.kind === "assistant" &&
      runId != null &&
      prev.items[0]?.message.agent_run_id === runId;

    if (canMerge && prev.kind === "assistant") {
      prev.items.push(item);
      continue;
    }

    blocks.push({
      kind: "assistant",
      key: item.message.id,
      items: [item],
      content: "",
      streaming: false,
      status: item.message.status,
      timeline: [],
      diffs: [],
    });
  }

  // Materialize merged fields.
  for (const [index, block] of blocks.entries()) {
    if (block.kind !== "assistant") continue;
    block.content = block.items
      .map((item) => item.message.content)
      .filter((text) => text.trim())
      .join("\n\n");
    const previous = blocks[index - 1];
    if (
      previous?.kind === "user" &&
      previous.item.message.agent_run_id === block.items[0]?.message.agent_run_id
    ) {
      block.content = removeLeadingPromptEcho(
        block.content,
        previous.item.message.content,
      );
    }
    block.streaming = block.items.some(
      (item) =>
        item.message.status === "streaming" ||
        (turnWorking && item.message.id === messages[messages.length - 1]?.message.id),
    );
    block.status = block.items.some((item) => item.message.status === "failed")
      ? "failed"
      : block.streaming
        ? "streaming"
        : "complete";
    block.timeline = buildTimeline(block.items, block.streaming, verbose);
    block.diffs = block.items.flatMap((item) => diffArtifacts(item.parts));
  }

  return blocks;
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
  const verboseReasoning = useAtomValue(verboseReasoningAtom);
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

  const messages = live?.detail?.messages ?? [];
  const blocks = useMemo(
    () => groupChatBlocks(messages, isWorking, verboseReasoning),
    [messages, isWorking, verboseReasoning],
  );

  if (!session || !live || live.chatId !== session.chat.id) {
    return (
      <main className="flex min-w-0 flex-1 items-center justify-center">
        <LoaderCircle className="size-5 animate-spin text-muted-foreground" />
      </main>
    );
  }

  const lastMessage = messages[messages.length - 1]?.message;
  const lastAssistantBlock = [...blocks].reverse().find((block) => block.kind === "assistant");
  // Only show a bottom placeholder when nothing assistant-visible exists yet.
  const showTurnPlaceholder =
    isWorking &&
    (!lastMessage || lastMessage.role === "user" || !lastAssistantBlock);
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
        {live.contextRestoration && (
          <span className="ml-3 text-xs text-muted-foreground">
            {live.contextRestoration}…
          </span>
        )}
        {!isWorking && live.runStatus && live.runStatus !== "running" && (
          <span className="ml-3 text-xs text-muted-foreground">{live.runStatus}</span>
        )}
      </header>
      <Conversation>
        <ConversationContent className="mx-auto w-full max-w-3xl gap-2 py-8">
          {blocks.map((block) => {
            if (block.kind === "user") {
              const { message } = block.item;
              return (
                <Message from="user" key={block.key} className="space-between">
                  <MessageContent className="w-full">
                    <p className="whitespace-pre-wrap">
                      <span className="mr-2 select-none text-muted-foreground">&gt;</span>
                      {message.content}
                    </p>
                  </MessageContent>
                </Message>
              );
            }

            const leadingWarning = splitLeadingWarning(block.content);
            const responseContent = leadingWarning?.response ?? block.content;
            const tone = assistantMessageTone(responseContent, block.status);
            const hasVisibleContent = Boolean(responseContent.trim());

            return (
              <Message from="assistant" key={block.key} className="space-between">
                <MessageContent className="w-full space-y-2">
                  {block.timeline.length > 0 && (
                    <ChainOfThought
                      // Controlled while streaming so the panel stays open for the whole turn.
                      open={block.streaming ? true : undefined}
                      defaultOpen={false}
                      className="space-y-0"
                    >
                      <ChainOfThoughtHeader className="py-1">
                        {block.streaming ? (
                          <span className="inline-flex items-center gap-1.5">
                            <LoaderCircle className="size-3 animate-spin" />
                            Reasoning…
                          </span>
                        ) : (
                          "Reasoning"
                        )}
                      </ChainOfThoughtHeader>
                      <ChainOfThoughtContent className="mt-0 space-y-1">
                        {block.timeline.map((step) => (
                          <ChainOfThoughtStep
                            key={step.key}
                            icon={step.icon}
                            label={reasoningLabel(step.label, verboseReasoning)}
                            status={step.status}
                          />
                        ))}
                      </ChainOfThoughtContent>
                    </ChainOfThought>
                  )}
                  {block.diffs.map((artifact) => (
                    <DiffArtifactCard key={artifact.key} artifact={artifact} />
                  ))}
                  {leadingWarning && (
                    <div className="flex gap-2 rounded-md py-2 text-xs text-amber-700 dark:text-amber-300">
                      <AlertTriangle className="mt-0.5 size-4 shrink-0" />
                      <MessageResponse>{leadingWarning.warning}</MessageResponse>
                    </div>
                  )}
                  {hasVisibleContent &&
                    (tone === "error" ? (
                      <div className="flex gap-2 rounded-md px-3 py-2 text-xs text-destructive">
                        <CircleX className="mt-0.5 size-4 shrink-0" />
                        <MessageResponse>{responseContent}</MessageResponse>
                      </div>
                    ) : tone === "warning" ? (
                      <div className="flex gap-2 rounded-md py-2 text-xs text-amber-700 dark:text-amber-300">
                        <AlertTriangle className="mt-0.5 size-4 shrink-0" />
                        <MessageResponse>{responseContent}</MessageResponse>
                      </div>
                    ) : (
                      <div>
                        <MessageResponse>{responseContent}</MessageResponse>
                        {block.streaming && <StreamingCaret />}
                      </div>
                    ))}
                  {!hasVisibleContent && block.streaming && (
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
