import { Conversation, ConversationContent, ConversationScrollButton } from "@/components/ai-elements/conversation"
import { Message, MessageContent } from "@/components/ai-elements/message"
import { Reasoning, ReasoningContent, ReasoningTrigger } from "@/components/ai-elements/reasoning"
import { Tool, ToolContent, ToolHeader, ToolInput, ToolOutput } from "@/components/ai-elements/tool"
import { Button } from "@/components/ui/button"
import type { AgentEvent } from "@/types"
import { Bot, Check, CircleDot, Terminal } from "lucide-react"

export function SessionTimeline({ events, onRespond }: { events: AgentEvent[]; onRespond: (event: AgentEvent, result: unknown) => void }) {
  return <Conversation className="h-full"><ConversationContent className="mx-auto w-full max-w-3xl space-y-4 px-8 py-7">
    {events.length === 0 && <div className="py-24 text-center text-sm text-muted-foreground"><Bot className="mx-auto mb-3 size-5" />Waiting for the agent to initialize…</div>}
    {events.map((event, index) => <SessionEvent event={event} key={index} onRespond={onRespond} />)}
  </ConversationContent><ConversationScrollButton /></Conversation>
}

function SessionEvent({ event, onRespond }: { event: AgentEvent; onRespond: (event: AgentEvent, result: unknown) => void }) {
  if (event.kind === "message") return <Message from={event.data.role === "user" ? "user" : "assistant"}><MessageContent>{event.data.text}</MessageContent></Message>
  if (event.kind === "activity") return <Tool defaultOpen={false}><ToolHeader type="dynamic-tool"><Terminal className="size-3.5" />{event.data.label}</ToolHeader><ToolContent><ToolInput input={event.data.payload} /><ToolOutput errorText={undefined} output={event.data.payload} /></ToolContent></Tool>
  if (event.kind === "request") return <div className="rounded-lg border border-primary/30 bg-primary/5 p-4"><div className="mb-2 flex items-center gap-2 text-sm font-medium"><CircleDot className="size-4 text-primary" />Agent decision required</div><p className="mb-3 text-xs text-muted-foreground">{event.data.method}</p><pre className="max-h-44 overflow-auto rounded bg-background/80 p-3 text-[11px]">{JSON.stringify(event.data.params, null, 2)}</pre><div className="mt-3 flex gap-2"><Button size="sm" onClick={() => onRespond(event, { approved: true })}><Check />Approve</Button><Button size="sm" variant="outline" onClick={() => onRespond(event, { approved: false })}>Deny</Button></div></div>
  if (event.kind === "status") return <Reasoning defaultOpen={false}><ReasoningTrigger>{event.data.status}</ReasoningTrigger><ReasoningContent>{event.data.detail ?? "Session state changed."}</ReasoningContent></Reasoning>
  return <div className="rounded-md border border-destructive/30 p-3 text-sm text-destructive">{event.data.message}</div>
}
