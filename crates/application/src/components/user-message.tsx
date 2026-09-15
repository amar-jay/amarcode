import {
  splitLeadingWarning,
  assistantMessageTone,
  cleanThinking,
  ChatBlock,
  groupChatBlocks,
} from "@/lib/message-parsing";
import { useCallback, useMemo, useRef, useState } from "react";
import { daemonApi } from "@/api";
import { notify } from "@/lib/notify";
import { LoaderCircle, AlertTriangle, CircleX } from "lucide-react";
import {
  ChainOfThought,
  ChainOfThoughtHeader,
  ChainOfThoughtContent,
  ChainOfThoughtStep,
} from "./ai-elements/chain-of-thought";
import {
  Message,
  MessageContent,
  MessageResponse,
} from "./ai-elements/message";
import { Shimmer } from "./ai-elements/shimmer";
import { AttachedFiles, AttachedImages } from "./attached-image";
import { DiffArtifactCard } from "./diff-artifact-card";
import { StreamingCaret } from "./streaming-caret";

const messageTimeFormatter = new Intl.DateTimeFormat(undefined, {
  hour: "numeric",
  minute: "2-digit",
});

function formatDuration(start: string, end: string): string {
  const milliseconds = Math.max(
    0,
    new Date(end).getTime() - new Date(start).getTime(),
  );
  const seconds = Math.round(milliseconds / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  return remainder ? `${minutes}m ${remainder}s` : `${minutes}m`;
}

function reasoningLabel(
  text: string,
  kind: string,
  verbose: boolean,
): React.ReactNode {
  const display = verbose ? text : `${text.slice(0, 320)}…`;
  return (
    <div className="whitespace-pre-wrap space-x-2 space-y-2 mt-1">
      {kind && <span className="font-bold">{kind.replaceAll("_", " ")}</span>}
      {kind === "execute" && verbose && (
        <>
          <br />
          <code className="font-mono">{display}</code>
        </>
      )}
      {kind === "thinking" && (
        <>
          <br />
          <code className="ml-1 text-xs">{cleanThinking(display)}</code>
        </>
      )}
    </div>
  );
}
export function UserMessage({
  block,
  verboseReasoning,
  waitingLabel,
  agentNames,
}: {
  block: ChatBlock;
  verboseReasoning: boolean;
  waitingLabel: string;
  agentNames: Map<string, string>;
}) {
  const [detailedBlock, setDetailedBlock] = useState<ChatBlock | null>(null);
  const detailRequest = useRef<Promise<void> | null>(null);

  const displayedBlock = detailedBlock ?? block;
  const hasDeferredParts = useMemo(
    () =>
      block.kind === "assistant" &&
      block.items.some((item) =>
        item.parts.some(
          (part) =>
            part.kind === "tool_call" &&
            part.content_json.includes('"_deferred":true'),
        ),
      ),
    [block],
  );
  const loadDetails = useCallback((): Promise<void> => {
    if (block.kind !== "assistant" || !hasDeferredParts || detailedBlock) {
      return Promise.resolve();
    }
    if (detailRequest.current) return detailRequest.current;
    const request = daemonApi
      .getMessageParts(block.items.map((item) => item.message.id))
      .then((parts) => {
        const byMessage = new Map<string, typeof parts>();
        for (const part of parts) {
          const current = byMessage.get(part.message_id) ?? [];
          current.push(part);
          byMessage.set(part.message_id, current);
        }
        const items = block.items.map((item) => ({
          ...item,
          parts: byMessage.get(item.message.id) ?? [],
        }));
        const detailed = groupChatBlocks(
          items,
          block.streaming,
          verboseReasoning,
        ).find((candidate) => candidate.kind === "assistant");
        if (detailed) setDetailedBlock(detailed);
      })
      .catch((cause: unknown) => {
        notify(
          cause instanceof Error
            ? cause.message
            : "Could not load reasoning details.",
          "error",
        );
        throw cause;
      })
      .finally(() => {
        detailRequest.current = null;
      });
    detailRequest.current = request;
    return request;
  }, [block, detailedBlock, hasDeferredParts, verboseReasoning]);

  if (displayedBlock.kind === "user") {
    const { message } = displayedBlock.item;
    return (
      <Message from="user" key={block.key} className="space-between">
        <MessageContent className="w-full">
          {message.content && (
            <p className="whitespace-pre-wrap">
              <span className="mr-2 select-none text-muted-foreground">
                &gt;
              </span>
              {message.content}
            </p>
          )}
          <AttachedImages item={displayedBlock.item} />
          <AttachedFiles item={displayedBlock.item} />
        </MessageContent>
      </Message>
    );
  }

  const leadingWarning = splitLeadingWarning(displayedBlock.content);
  const responseContent = leadingWarning?.response ?? displayedBlock.content;
  const tone = assistantMessageTone(responseContent);
  const hasVisibleContent = Boolean(responseContent.trim());
  const interruption =
    displayedBlock.status === "failed"
      ? "Response failed before completion."
      : displayedBlock.status === "interrupted"
        ? "Response was interrupted before completion."
        : null;

  return (
    <Message from="assistant" key={block.key} className="space-between">
      <MessageContent className="w-full space-y-2">
        {displayedBlock.timeline.length > 0 && (
          <ChainOfThought
            // Controlled while streaming so the panel stays open for the whole turn.
            open={displayedBlock.streaming ? true : undefined}
            defaultOpen={false}
            onOpenChange={(open) => {
              if (open) void loadDetails().catch(() => {});
            }}
            className="space-y-0"
          >
            <ChainOfThoughtHeader className="py-1">
              {displayedBlock.streaming ? (
                <span className="inline-flex items-center gap-1.5">
                  <LoaderCircle className="size-3 animate-spin" />
                  Reasoning…
                </span>
              ) : (
                "Reasoning"
              )}
            </ChainOfThoughtHeader>
            <ChainOfThoughtContent className="mt-0 space-y-1">
              {displayedBlock.timeline.map((step) => (
                <ChainOfThoughtStep
                  key={step.key}
                  icon={step.icon}
                  kind={step.kind}
                  label={reasoningLabel(
                    step.label,
                    step.kind,
                    verboseReasoning,
                  )}
                  description={step.description}
                  status={step.status}
                />
              ))}
            </ChainOfThoughtContent>
          </ChainOfThought>
        )}
        {displayedBlock.diffs.map((artifact) => (
          <DiffArtifactCard
            key={artifact.key}
            artifact={artifact}
            onExpand={loadDetails}
          />
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
              {displayedBlock.streaming && <StreamingCaret />}
            </div>
          ))}
        {!hasVisibleContent && displayedBlock.streaming && (
          <div className="mt-1 flex items-center gap-2 text-sm text-muted-foreground">
            <LoaderCircle className="size-3.5 shrink-0 animate-spin" />
            <Shimmer
              className="text-sm"
              duration={1.4}
            >{`${waitingLabel}…`}</Shimmer>
          </div>
        )}
        {interruption && (
          <div className="flex items-center gap-2 text-xs text-destructive">
            <CircleX className="size-3.5 shrink-0" />
            <span>{interruption}</span>
          </div>
        )}
        {!displayedBlock.streaming && (
          <footer
            className="flex items-center gap-1.5 pt-1 font-mono text-[0.65rem] text-muted-foreground/70"
            title={new Date(displayedBlock.completedAt).toLocaleString()}
          >
            <time dateTime={displayedBlock.completedAt}>
              {messageTimeFormatter.format(
                new Date(displayedBlock.completedAt),
              )}
            </time>
            <span aria-hidden="true">·</span>
            <span>
              {formatDuration(
                displayedBlock.startedAt,
                displayedBlock.completedAt,
              )}
            </span>
            <span aria-hidden="true">·</span>
            <span>
              {(displayedBlock.agentId &&
                agentNames.get(displayedBlock.agentId)) ??
                displayedBlock.agentId ??
                "Unknown ACP"}
            </span>
          </footer>
        )}
      </MessageContent>
    </Message>
  );
}
