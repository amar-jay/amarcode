import { useState } from "react"
import { Attachment, AttachmentPreview, AttachmentRemove, Attachments } from "@/components/ai-elements/attachments"
import {
  PromptInput,
  PromptInputActionAddAttachments,
  PromptInputActionAddScreenshot,
  PromptInputActionMenu,
  PromptInputActionMenuContent,
  PromptInputActionMenuTrigger,
  PromptInputBody,
  PromptInputFooter,
  PromptInputHeader,
  type PromptInputMessage,
  PromptInputSelect,
  PromptInputSelectContent,
  PromptInputSelectItem,
  PromptInputSelectTrigger,
  PromptInputSelectValue,
  PromptInputSubmit,
  PromptInputTextarea,
  PromptInputTools,
  usePromptInputAttachments,
} from "@/components/ai-elements/prompt-input"
import { Badge } from "@/components/ui/badge"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import type { AgentDefinition } from "@/types"
import { Bot, FolderOpen, ShieldCheck } from "lucide-react"

export type WorkMode = "plan" | "build" | "ask"
export type PermissionPolicy = "ask" | "safe" | "autonomous"

type WorkbenchPromptInputProps = {
  agent?: AgentDefinition
  workspacePath: string
  isWorking: boolean
  onSubmit: (input: { text: string; files: PromptInputMessage["files"]; mode: WorkMode; permission: PermissionPolicy }) => Promise<void>
  onStop: () => void
}

const modeLabels: Record<WorkMode, string> = { plan: "Plan", build: "Build", ask: "Ask" }
const permissionLabels: Record<PermissionPolicy, string> = { ask: "Ask always", safe: "Approve safe", autonomous: "Autonomous" }

function PromptAttachments() {
  const attachments = usePromptInputAttachments()
  if (attachments.files.length === 0) return null
  return <Attachments variant="inline">
    {attachments.files.map((file) => <Attachment data={file} key={file.id} onRemove={() => attachments.remove(file.id)}>
      <AttachmentPreview /><span className="max-w-40 truncate">{file.filename ?? "Attachment"}</span><AttachmentRemove />
    </Attachment>)}
  </Attachments>
}

export function WorkbenchPromptInput({ agent, workspacePath, isWorking, onSubmit, onStop }: WorkbenchPromptInputProps) {
  const [mode, setMode] = useState<WorkMode>("build")
  const [permission, setPermission] = useState<PermissionPolicy>("ask")
  return <div className="mx-auto w-full max-w-3xl">
    <div className="mb-2 flex min-w-0 items-center gap-2 px-1 text-[11px] text-muted-foreground">
      <Tooltip><TooltipTrigger asChild><span className="flex min-w-0 items-center gap-1.5"><FolderOpen className="size-3 shrink-0" /><span className="truncate">{workspacePath}</span></span></TooltipTrigger><TooltipContent>{workspacePath}</TooltipContent></Tooltip>
      <span className="text-border">/</span><span className="flex shrink-0 items-center gap-1.5"><Bot className="size-3" />{agent?.name ?? "ACP agent"}</span>
    </div>
    <PromptInput accept="image/*,.txt,.md,.json,.ts,.tsx,.js,.jsx,.rs,.py,.log" className="rounded-xl border border-border bg-card shadow-sm" globalDrop maxFiles={10} maxFileSize={10 * 1024 * 1024} multiple onError={({ message }) => console.warn(message)} onSubmit={async (message) => {
      if (!message.text.trim() && message.files.length === 0) return
      await onSubmit({ files: message.files, mode, permission, text: message.text.trim() })
    }}>
      <PromptInputHeader className="px-3 pt-3"><PromptAttachments /></PromptInputHeader>
      <PromptInputBody><PromptInputTextarea className="px-3 py-3 text-sm" placeholder="Describe what you want the agent to do…" /></PromptInputBody>
      <PromptInputFooter className="border-t border-border/70 px-2 py-2">
        <PromptInputTools>
          <PromptInputActionMenu><PromptInputActionMenuTrigger tooltip={{ content: "Add context", shortcut: "⌘⇧A" }} /><PromptInputActionMenuContent><PromptInputActionAddAttachments label="Attach files" /><PromptInputActionAddScreenshot /></PromptInputActionMenuContent></PromptInputActionMenu>
          <PromptInputSelect onValueChange={(value) => setMode(value as WorkMode)} value={mode}><PromptInputSelectTrigger aria-label="Working mode" className="h-7 gap-1 px-1.5 text-xs"><PromptInputSelectValue>{modeLabels[mode]}</PromptInputSelectValue></PromptInputSelectTrigger><PromptInputSelectContent><PromptInputSelectItem value="plan">Plan</PromptInputSelectItem><PromptInputSelectItem value="build">Build</PromptInputSelectItem><PromptInputSelectItem value="ask">Ask</PromptInputSelectItem></PromptInputSelectContent></PromptInputSelect>
          <PromptInputSelect onValueChange={(value) => setPermission(value as PermissionPolicy)} value={permission}><PromptInputSelectTrigger aria-label="Permission policy" className="h-7 gap-1 px-1.5 text-xs"><ShieldCheck className="size-3" /><PromptInputSelectValue>{permissionLabels[permission]}</PromptInputSelectValue></PromptInputSelectTrigger><PromptInputSelectContent><PromptInputSelectItem value="ask">Ask always</PromptInputSelectItem><PromptInputSelectItem value="safe">Approve safe actions</PromptInputSelectItem><PromptInputSelectItem value="autonomous">Autonomous</PromptInputSelectItem></PromptInputSelectContent></PromptInputSelect>
        </PromptInputTools>
        <div className="flex items-center gap-2"><Badge variant="secondary" className="hidden sm:inline-flex">⌘↵ Send</Badge><Tooltip><TooltipTrigger asChild><span><PromptInputSubmit onStop={onStop} status={isWorking ? "streaming" : "ready"} /></span></TooltipTrigger><TooltipContent>{isWorking ? "Stop agent" : "Send prompt"}</TooltipContent></Tooltip></div>
      </PromptInputFooter>
    </PromptInput>
    <p className="mt-2 px-1 text-[11px] text-muted-foreground">Attachments are staged as local context. ACP text prompts and approval requests remain visible in this session.</p>
  </div>
}
