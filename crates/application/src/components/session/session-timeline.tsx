import {
  Confirmation,
  ConfirmationAccepted,
  ConfirmationAction,
  ConfirmationActions,
  ConfirmationRejected,
  ConfirmationRequest,
  ConfirmationTitle,
} from "@/components/ai-elements/confirmation";
import {
  Conversation,
  ConversationContent,
  ConversationScrollButton,
} from "@/components/ai-elements/conversation";
import {
  Message,
  MessageContent,
  MessageResponse,
} from "@/components/ai-elements/message";
import {
  Reasoning,
  ReasoningContent,
  ReasoningTrigger,
} from "@/components/ai-elements/reasoning";
import {
  Task,
  TaskContent,
  TaskItem,
  TaskTrigger,
} from "@/components/ai-elements/task";
import {
  Tool,
  ToolContent,
  ToolHeader,
  ToolInput,
  ToolOutput,
} from "@/components/ai-elements/tool";
import type { AgentEvent } from "@/types";
import { Bot, Check, ChevronDown, CircleDot, Sparkles, X } from "lucide-react";
import { useState } from "react";
import {
  deriveSessionTimeline,
  type SessionTimelineEntry,
} from "./session-timeline-model";

export function SessionTimeline({
  events,
  isWorking,
  onRespond,
}: {
  events: AgentEvent[];
  isWorking: boolean;
  onRespond: (event: AgentEvent, result: unknown) => Promise<void>;
}) {
  const entries = deriveSessionTimeline(events);
  return (
    <Conversation className="h-full bg-background">
      <ConversationContent className="mx-auto w-full max-w-3xl space-y-6 px-8 py-8">
        {entries.length === 0 && !isWorking && <EmptyConversation />}
        {entries.map((entry) => (
          <TimelineEntry entry={entry} key={entry.id} onRespond={onRespond} />
        ))}
        {isWorking && <AgentWorking />}
      </ConversationContent>
      <ConversationScrollButton />
    </Conversation>
  );
}

function EmptyConversation() {
  return (
    <div className="flex min-h-72 flex-col items-center justify-center text-center">
      <div className="grid size-10 place-items-center rounded-xl border border-border bg-card shadow-sm">
        <Bot className="size-5 text-primary" />
      </div>
      <p className="mt-4 text-sm font-medium">Ready when you are</p>
      <p className="mt-1 max-w-sm text-sm leading-6 text-muted-foreground">
        Ask the agent to inspect the workspace, make a change, or explain a part
        of the project.
      </p>
    </div>
  );
}

function AgentWorking() {
  return (
    <Reasoning isStreaming className="max-w-md" defaultOpen>
      <ReasoningTrigger>
        <span className="flex items-center gap-2">
          <Sparkles className="size-4 text-primary" />
          Working on your request
        </span>
      </ReasoningTrigger>
      <ReasoningContent>
        The agent is inspecting the workspace and preparing its response.
      </ReasoningContent>
    </Reasoning>
  );
}

function TimelineEntry({
  entry,
  onRespond,
}: {
  entry: SessionTimelineEntry;
  onRespond: (event: AgentEvent, result: unknown) => Promise<void>;
}) {
  if (entry.kind === "message")
    return entry.role === "user" ? (
      <Message from="user" className="max-w-full">
        <MessageContent className="max-w-[80%]">
          <p>{entry.text}</p>
        </MessageContent>
      </Message>
    ) : (
      <Message from="assistant" className="max-w-full">
        <MessageContent className="max-w-full">
          <MessageResponse>{entry.text}</MessageResponse>
        </MessageContent>
      </Message>
    );
  if (entry.kind === "work-group") return <WorkGroup events={entry.events} />;
  if (entry.kind === "approval")
    return <ApprovalRequest event={entry.event} onRespond={onRespond} />;
  return (
    <div className="max-w-xl rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">
      {entry.message}
    </div>
  );
}

function WorkGroup({
  events,
}: {
  events: Extract<AgentEvent, { kind: "activity" }>[];
}) {
  const title =
    events.length === 1
      ? events[0].data.label
      : `${events.length} steps completed`;
  return (
    <Task defaultOpen className="max-w-2xl py-1">
      <TaskTrigger title={title}>
        <div className="flex w-full cursor-pointer items-center gap-2 text-sm text-muted-foreground transition-colors hover:text-foreground">
          <Sparkles className="size-3.5 text-primary" />
          <span>{title}</span>
          <ChevronDown className="size-3.5 transition-transform group-data-[state=open]:rotate-180" />
        </div>
      </TaskTrigger>
      <TaskContent className="ml-1">
        {events.map((event, index) => (
          <TaskItem key={index}>
            <Tool
              defaultOpen={false}
              className="mb-1 border-0 bg-transparent shadow-none"
            >
              <ToolHeader
                className="rounded-md px-2 py-1.5 hover:bg-muted/50"
                type="dynamic-tool"
                toolName={event.data.label}
                state="output-available"
              />
              <ToolContent className="mx-2 mb-2 rounded-md border border-border/70 bg-card/50">
                <ToolInput input={event.data.payload} />
                <ToolOutput errorText={undefined} output={event.data.payload} />
              </ToolContent>
            </Tool>
          </TaskItem>
        ))}
      </TaskContent>
    </Task>
  );
}

function ApprovalRequest({
  event,
  onRespond,
}: {
  event: Extract<AgentEvent, { kind: "request" }>;
  onRespond: (event: AgentEvent, result: unknown) => Promise<void>;
}) {
  const [decision, setDecision] = useState<boolean | undefined>();
  const [error, setError] = useState("");
  const [isSending, setIsSending] = useState(false);
  const approval =
    decision === undefined
      ? { id: String(event.data.requestId) }
      : { id: String(event.data.requestId), approved: decision };
  const state =
    decision === undefined ? "approval-requested" : "approval-responded";
  async function respond(approved: boolean) {
    setIsSending(true);
    setError("");
    try {
      await onRespond(event, { approved });
      setDecision(approved);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setIsSending(false);
    }
  }
  return (
    <Confirmation
      approval={approval}
      state={state}
      className="max-w-xl border-primary/30 bg-primary/5 shadow-sm"
    >
      <ConfirmationTitle>
        <span className="flex items-center gap-2 font-medium">
          <CircleDot className="size-4 text-primary" />
          Agent approval needed
        </span>
      </ConfirmationTitle>
      <ConfirmationRequest>
        <>
          <p className="text-xs text-muted-foreground">{event.data.method}</p>
          <pre className="max-h-44 overflow-auto rounded-md border bg-background/80 p-3 text-[11px] leading-5">
            {JSON.stringify(event.data.params, null, 2)}
          </pre>
          {error && <p className="text-xs text-destructive">{error}</p>}
        </>
      </ConfirmationRequest>
      <ConfirmationAccepted>
        <p className="flex items-center gap-2 text-sm text-emerald-700">
          <Check className="size-4" />
          Approved and sent to the agent
        </p>
      </ConfirmationAccepted>
      <ConfirmationRejected>
        <p className="flex items-center gap-2 text-sm text-muted-foreground">
          <X className="size-4" />
          Denied and sent to the agent
        </p>
      </ConfirmationRejected>
      <ConfirmationActions>
        <ConfirmationAction
          disabled={isSending}
          onClick={() => void respond(true)}
        >
          {isSending ? "Sending…" : "Approve"}
        </ConfirmationAction>
        <ConfirmationAction
          disabled={isSending}
          variant="outline"
          onClick={() => void respond(false)}
        >
          Deny
        </ConfirmationAction>
      </ConfirmationActions>
    </Confirmation>
  );
}
