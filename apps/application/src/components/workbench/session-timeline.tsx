import { Conversation, ConversationContent, ConversationScrollButton } from "@/components/ai-elements/conversation"
import { Message, MessageContent, MessageResponse } from "@/components/ai-elements/message"
import { Reasoning, ReasoningContent, ReasoningTrigger } from "@/components/ai-elements/reasoning"
import { Tool, ToolContent, ToolHeader, ToolInput, ToolOutput } from "@/components/ai-elements/tool"
import { Button } from "@/components/ui/button"
import type { AgentEvent } from "@/types"
import { Bot, Check, CircleDot, Sparkles, X } from "lucide-react"
import { useState } from "react"

export function SessionTimeline({ events, isWorking, onRespond }: { events: AgentEvent[]; isWorking: boolean; onRespond: (event: AgentEvent, result: unknown) => Promise<void> }) {
  // Coalesce before hiding protocol events: those events are turn boundaries in
  // older saved sessions that do not yet have persisted user messages.
  const displayEvents = coalesceMessageChunks(events).filter(isTimelineEvent)

  return <Conversation className="h-full bg-background"><ConversationContent className="mx-auto w-full max-w-3xl space-y-6 px-8 py-8">
    {displayEvents.length === 0 && !isWorking && <EmptyConversation />}
    {displayEvents.map((event, index) => <SessionEvent event={event} key={index} onRespond={onRespond} />)}
    {isWorking && <AgentWorking />}
  </ConversationContent><ConversationScrollButton /></Conversation>
}

function isTimelineEvent(event: AgentEvent) {
  if (event.kind === "turnComplete" || event.kind === "status") return false
  return event.kind !== "activity" || !["response", "session update"].includes(event.data.label)
}

function EmptyConversation() {
  return <div className="flex min-h-72 flex-col items-center justify-center text-center"><div className="grid size-10 place-items-center rounded-xl border border-border bg-card shadow-sm"><Bot className="size-5 text-primary" /></div><p className="mt-4 text-sm font-medium">Ready when you are</p><p className="mt-1 max-w-sm text-sm leading-6 text-muted-foreground">Ask the agent to inspect the workspace, make a change, or explain a part of the project.</p></div>
}

function AgentWorking() {
  return <Reasoning isStreaming className="max-w-md" defaultOpen><ReasoningTrigger><span className="flex items-center gap-2"><Sparkles className="size-4 text-primary" />Working on your request</span></ReasoningTrigger><ReasoningContent>The agent is inspecting the workspace and preparing its response.</ReasoningContent></Reasoning>
}

// ACP emits a streamed response in many message chunks. Other protocol events
// mark a new boundary, so separate responses never collapse into one message.
function coalesceMessageChunks(events: AgentEvent[]): AgentEvent[] {
  return events.reduce<AgentEvent[]>((coalesced, event) => {
    const previous = coalesced.at(-1)
    if (event.kind === "message" && event.data.role !== "user" && previous?.kind === "message" && previous.data.sessionId === event.data.sessionId && previous.data.role === event.data.role) {
      coalesced[coalesced.length - 1] = { kind: "message", data: { ...previous.data, text: previous.data.text + event.data.text } }
    } else {
      coalesced.push(event)
    }
    return coalesced
  }, [])
}

function SessionEvent({ event, onRespond }: { event: AgentEvent; onRespond: (event: AgentEvent, result: unknown) => Promise<void> }) {
  if (event.kind === "message") return <Message from={event.data.role === "user" ? "user" : "assistant"} className="max-w-full"><MessageContent className={event.data.role === "user" ? "max-w-[80%]" : "max-w-full"}>{event.data.role === "user" ? event.data.text : <MessageResponse>{event.data.text}</MessageResponse>}</MessageContent></Message>
  if (event.kind === "activity") return <Tool defaultOpen={false} className="max-w-full rounded-lg border-border/70 bg-card/50"><ToolHeader type="dynamic-tool" toolName={event.data.label} state="output-available" /><ToolContent><ToolInput input={event.data.payload} /><ToolOutput errorText={undefined} output={event.data.payload} /></ToolContent></Tool>
  if (event.kind === "request") return <ApprovalRequest event={event} onRespond={onRespond} />
  if (event.kind === "protocolError") return <div className="max-w-xl rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">{event.data.message}</div>
  return null
}

function ApprovalRequest({ event, onRespond }: { event: Extract<AgentEvent, { kind: "request" }>; onRespond: (event: AgentEvent, result: unknown) => Promise<void> }) {
  const [decision, setDecision] = useState<boolean | undefined>()
  const [error, setError] = useState("")
  const [isSending, setIsSending] = useState(false)
  const approval = decision === undefined ? { id: String(event.data.requestId) } : { id: String(event.data.requestId), approved: decision }
  const state = decision === undefined ? "approval-requested" : "approval-responded"

  async function respond(approved: boolean) {
    setIsSending(true)
    setError("")
    try {
      await onRespond(event, { approved })
      setDecision(approved)
    } catch (reason) {
      setError(String(reason))
    } finally {
      setIsSending(false)
    }
  }

  return <Confirmation approval={approval} state={state} className="max-w-xl border-primary/30 bg-primary/5 shadow-sm"><ConfirmationTitle><span className="flex items-center gap-2 font-medium"><CircleDot className="size-4 text-primary" />Agent approval needed</span></ConfirmationTitle><ConfirmationRequest><><p className="text-xs text-muted-foreground">{event.data.method}</p><pre className="max-h-44 overflow-auto rounded-md border bg-background/80 p-3 text-[11px] leading-5">{JSON.stringify(event.data.params, null, 2)}</pre>{error && <p className="text-xs text-destructive">{error}</p>}</></ConfirmationRequest><ConfirmationAccepted><p className="flex items-center gap-2 text-sm text-emerald-700"><Check className="size-4" />Approved and sent to the agent</p></ConfirmationAccepted><ConfirmationRejected><p className="flex items-center gap-2 text-sm text-muted-foreground"><X className="size-4" />Denied and sent to the agent</p></ConfirmationRejected><ConfirmationActions><ConfirmationAction disabled={isSending} onClick={() => void respond(true)}>{isSending ? "Sending…" : "Approve"}</ConfirmationAction><ConfirmationAction disabled={isSending} variant="outline" onClick={() => void respond(false)}>Deny</ConfirmationAction></ConfirmationActions></Confirmation>
}
import { Confirmation, ConfirmationAccepted, ConfirmationAction, ConfirmationActions, ConfirmationRejected, ConfirmationRequest, ConfirmationTitle } from "@/components/ai-elements/confirmation"
