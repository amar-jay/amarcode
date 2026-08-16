import {
  splitLeadingWarning,
  assistantMessageTone,
  cleanThinking,
  cleanToolTitle,
  ChatBlock,
} from "@/lib/message-parsing";
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
import { AttachedImages } from "./attached-image";
import { DiffArtifactCard } from "./diff-artifact-card";
import { StreamingCaret } from "./streaming-caret";

function reasoningLabel(
  text: string,
  kind: string,
  verbose: boolean,
): React.ReactNode {
  const display = verbose ? text : `${text.slice(0, 320)}…`;
  return (
    <div className="whitespace-pre-wrap space-x-2 space-y-2 mt-1">
      {kind && <span className="font-bold">{kind}</span>}
      {kind === "execute" && verbose && (
        <>
          <br /> <code className="font-mono">{display}</code>
        </>
      )}
      {kind === "thinking" && (
        <>
          <br />
          <code className="ml-1">{cleanThinking(display)}</code>
        </>
      )}
      {kind !== "execute" && kind !== "thinking" && kind !== "search" && (
        <code className="font-mono">{cleanToolTitle(display)}</code>
      )}
    </div>
  );
}
export function UserMessage({
  block,
  verboseReasoning,
  waitingLabel,
}: {
  block: ChatBlock;
  verboseReasoning: boolean;
  waitingLabel: string;
}) {
  if (block.kind === "user") {
    const { message } = block.item;
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
          <AttachedImages item={block.item} />
        </MessageContent>
      </Message>
    );
  }

  const leadingWarning = splitLeadingWarning(block.content);
  const responseContent = leadingWarning?.response ?? block.content;
  const tone = assistantMessageTone(responseContent);
  const hasVisibleContent = Boolean(responseContent.trim());
  const interruption =
    block.status === "failed"
      ? "Response failed before completion."
      : block.status === "interrupted"
        ? "Response was interrupted before completion."
        : null;

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
      </MessageContent>
    </Message>
  );
}
