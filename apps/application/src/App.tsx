import { useEffect, useMemo, useState } from "react"
import { open } from "@tauri-apps/plugin-dialog"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { Bot, ChevronRight, Cog, FolderOpen, Minus, Plus, Square, X } from "lucide-react"
import { api } from "./api"
import type { AgentDefinition, AgentEvent, SessionSummary } from "./types"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { SettingsDialog } from "@/components/settings/settings-dialog"
import { Toaster } from "@/components/ui/sonner"
import { useTheme } from "@/hooks/use-theme"
import { notify } from "@/lib/notify"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupContent, SidebarGroupLabel, SidebarHeader, SidebarInset, SidebarMenu, SidebarMenuButton, SidebarMenuItem, SidebarProvider, SidebarRail, SidebarTrigger } from "@/components/ui/sidebar"
import { WorkbenchPromptInput, type WorkMode } from "@/components/workbench/workbench-prompt-input"
import { SessionTimeline } from "@/components/workbench/session-timeline"

const eventText = (event: AgentEvent) => event.kind === "message" ? event.data.text : event.kind === "status" ? `${event.data.status}${event.data.detail ? ` · ${event.data.detail}` : ""}` : event.kind === "protocolError" ? event.data.message : event.kind === "request" ? `${event.data.method} needs your response` : event.data.label

export default function App() {
  const [agents, setAgents] = useState<AgentDefinition[]>([])
  const [sessions, setSessions] = useState<SessionSummary[]>([])
  const [selectedAgent, setSelectedAgent] = useState("")
  const [workspace, setWorkspace] = useState("")
  const [active, setActive] = useState<SessionSummary>()
  const [events, setEvents] = useState<AgentEvent[]>([])
  const [isPromptWorking, setIsPromptWorking] = useState(false)
  const [error, setError] = useState("")
  const [showAgentForm, setShowAgentForm] = useState(false)
  const [agentForm, setAgentForm] = useState({ name: "", command: "", arguments: "" })
  const [settingsOpen, setSettingsOpen] = useState(false)
  const { theme, setTheme } = useTheme()

  useEffect(() => {
    void Promise.all([api.agents(), api.sessions()])
      .then(([loadedAgents, loadedSessions]) => {
        setAgents(loadedAgents)
        setSessions(loadedSessions)
        setSelectedAgent(loadedAgents[0]?.id ?? "")
      })
      .catch((reason) => {
        setError(String(reason))
        notify("Could not load the workbench", "error")
      })
  }, [])

  const agent = useMemo(() => agents.find((candidate) => candidate.id === selectedAgent), [agents, selectedAgent])
  const activeAgent = useMemo(() => agents.find((candidate) => candidate.id === active?.agentId), [active?.agentId, agents])
  const onEvent = (event: AgentEvent) => {
    if (event.kind !== "status" || event.data.status === "stopped") setIsPromptWorking(false)
    setEvents((current) => [...current, event])
  }

  async function chooseWorkspace() {
    const path = await open({ directory: true, multiple: false, title: "Choose a project folder" })
    if (typeof path === "string") setWorkspace(path)
  }

  async function start() {
    if (!workspace || !agent) return
    setError("")
    setEvents([])
    try {
      const session = await api.start(workspace, agent, onEvent)
      setActive(session)
      setSessions((current) => [session, ...current])
      notify(`${agent.name} session started`, "success")
    } catch (reason) {
      setError(String(reason))
      notify("Unable to start the agent session", "error")
    }
  }

  async function restore(session: SessionSummary) {
    try {
      setActive(session)
      setEvents(await api.events(session.id))
    } catch (reason) {
      setError(String(reason))
      notify("Could not restore that session", "error")
    }
  }

  async function submitPrompt({ text, files, sources, mode }: { text: string; files: { filename?: string }[]; sources: { title?: string; filename?: string }[]; mode: WorkMode }) {
    if (!active || (!text && files.length === 0)) return
    const attachmentNames = files.map((file) => file.filename ?? "Attachment")
    const visibleText = text || `Review the attached context: ${attachmentNames.join(", ")}`
    const sourceNames = sources.map((source) => source.title ?? source.filename ?? "Workspace context")
    const acpPrompt = [
      `[Work mode: ${mode}]`,
      attachmentNames.length ? `[Local context attachments: ${attachmentNames.join(", ")}]` : "",
      sourceNames.length ? `[Referenced workspace context: ${sourceNames.join(", ")}]` : "",
      visibleText,
    ].filter(Boolean).join("\n\n")
    setEvents((current) => [...current, { kind: "message", data: { sessionId: active.id, role: "user", text: visibleText } }])
    setIsPromptWorking(true)
    try {
      await api.prompt(active.id, acpPrompt)
    } catch (reason) {
      setIsPromptWorking(false)
      setError(String(reason))
      notify("Prompt could not be sent", "error")
      throw reason
    }
  }

  async function stopPrompt() {
    if (!active) return
    try {
      await api.cancel(active.id)
      setIsPromptWorking(false)
      notify("Agent run cancelled", "success")
    } catch (reason) {
      setError(String(reason))
      notify("Could not cancel the agent", "error")
    }
  }

  async function addAgent(event: React.FormEvent) {
    event.preventDefault()
    const created: AgentDefinition = { id: crypto.randomUUID(), name: agentForm.name, command: agentForm.command, arguments: agentForm.arguments.split(" ").filter(Boolean), environment: [], isPreset: false }
    try {
      await api.saveAgent(created)
      setAgents((current) => [...current, created])
      setSelectedAgent(created.id)
      setShowAgentForm(false)
      setAgentForm({ name: "", command: "", arguments: "" })
      notify(`${created.name} was added`, "success")
    } catch (reason) {
      setError(String(reason))
      notify("Could not add that agent", "error")
    }
  }

  return <SidebarProvider>
    <TopBar active={active} />
    <WorkbenchSidebar active={active} sessions={sessions} onNewSession={() => setActive(undefined)} onRestore={restore} onSettings={() => setSettingsOpen(true)} />
    <SidebarInset className="min-w-0 pt-9">
      {!active ? <NewSession agent={agent} workspace={workspace} agents={agents} selectedAgent={selectedAgent} showAgentForm={showAgentForm} agentForm={agentForm} error={error} onChooseWorkspace={() => void chooseWorkspace()} onSelectAgent={setSelectedAgent} onShowAgentForm={() => setShowAgentForm((shown) => !shown)} onAgentFormChange={setAgentForm} onAddAgent={addAgent} onStart={() => void start()} /> : <>
        <SessionHeader active={active} agent={activeAgent} onCancel={() => void stopPrompt()} />
        <div className="min-h-0 flex-1"><SessionTimeline events={events} onRespond={(event, result) => { if (event.kind === "request") void api.respond(active.id, event.data.requestId, result).catch((reason) => setError(String(reason))) }} /></div>
        <div className="sticky bottom-0 z-20 shrink-0 border-t border-border bg-background/95 px-8 py-4 backdrop-blur-sm"><WorkbenchPromptInput agent={activeAgent} workspacePath={active.workspacePath} isWorking={isPromptWorking} onStop={() => void stopPrompt()} onSubmit={submitPrompt} />{error && <p className="mx-auto mt-2 max-w-3xl text-xs text-destructive">{error}</p>}</div>
      </>}
    </SidebarInset>
    <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} theme={theme} onThemeChange={setTheme} />
    <Toaster position="bottom-right" closeButton theme={theme} />
  </SidebarProvider>
}

function TopBar({ active }: { active?: SessionSummary }) {
  return <div className="fixed inset-x-0 top-0 z-[60] flex h-9 items-center border-b border-border bg-card">
    <div data-tauri-drag-region className="flex h-full min-w-0 flex-1 items-center gap-2 px-3 select-none"><img src="/acp-mark.svg" alt="" className="size-4" /><span className="text-xs font-medium">ACP Workbench</span><span className="border-l border-border pl-2 text-xs text-muted-foreground">Agent workspace</span></div>
    {active && <Badge variant="secondary" className="mr-3"><span className="size-1.5 rounded-full bg-primary" />{active.status}</Badge>}
    <div className="flex h-full"><button type="button" className="grid h-9 w-12 place-items-center text-muted-foreground transition-colors hover:bg-muted hover:text-foreground" onClick={() => void getCurrentWindow().minimize()} aria-label="Minimize"><Minus className="size-4" /></button><button type="button" className="grid h-9 w-12 place-items-center text-muted-foreground transition-colors hover:bg-muted hover:text-foreground" onClick={() => void getCurrentWindow().toggleMaximize()} aria-label="Maximize"><Square className="size-3.5" /></button><button type="button" className="grid h-9 w-12 place-items-center text-muted-foreground transition-colors hover:bg-destructive hover:text-background" onClick={() => void getCurrentWindow().close()} aria-label="Close"><X className="size-4" /></button></div>
  </div>
}

function WorkbenchSidebar({ active, sessions, onNewSession, onRestore, onSettings }: { active?: SessionSummary; sessions: SessionSummary[]; onNewSession: () => void; onRestore: (session: SessionSummary) => void; onSettings: () => void }) {
  return <Sidebar variant="floating" collapsible="icon" className="inset-y-auto! top-9! bottom-0! h-auto!">
    <SidebarHeader className="border-b border-sidebar-border"><div className="flex items-center gap-1 px-2 py-1"><SidebarTrigger /><span className="font-heading text-sm font-semibold tracking-tight group-data-[collapsible=icon]:hidden">ACP Workbench</span></div><SidebarMenu><SidebarMenuItem><SidebarMenuButton isActive={!active} tooltip="New session" onClick={onNewSession}><Plus /><span>New session</span></SidebarMenuButton></SidebarMenuItem></SidebarMenu></SidebarHeader>
    <SidebarContent className="px-2 py-2"><SidebarGroup className="p-0"><SidebarGroupLabel>Recent sessions</SidebarGroupLabel><SidebarGroupContent><SidebarMenu>{sessions.length === 0 ? <p className="px-2 py-3 text-xs text-muted-foreground group-data-[collapsible=icon]:hidden">No saved sessions yet.</p> : sessions.map((session) => <SidebarMenuItem key={session.id}><SidebarMenuButton isActive={active?.id === session.id} tooltip={session.workspacePath.split("/").pop() ?? session.workspacePath} onClick={() => onRestore(session)}><FolderOpen /><span>{session.workspacePath.split("/").pop()}</span><Badge variant="secondary" className="ml-auto text-[10px] group-data-[collapsible=icon]:hidden">{session.status}</Badge></SidebarMenuButton></SidebarMenuItem>)}</SidebarMenu></SidebarGroupContent></SidebarGroup></SidebarContent>
    <SidebarFooter className="border-t border-sidebar-border"><div className="px-2 py-1 text-[11px] leading-5 text-muted-foreground group-data-[collapsible=icon]:hidden">Local-only ACP client.<br />Secrets stay in the OS keychain.</div><SidebarMenu><SidebarMenuItem><SidebarMenuButton tooltip="Settings" onClick={onSettings}><Cog /><span>Settings</span></SidebarMenuButton></SidebarMenuItem></SidebarMenu></SidebarFooter><SidebarRail />
  </Sidebar>
}

function NewSession({ agent, workspace, agents, selectedAgent, showAgentForm, agentForm, error, onChooseWorkspace, onSelectAgent, onShowAgentForm, onAgentFormChange, onAddAgent, onStart }: { agent?: AgentDefinition; workspace: string; agents: AgentDefinition[]; selectedAgent: string; showAgentForm: boolean; agentForm: { name: string; command: string; arguments: string }; error: string; onChooseWorkspace: () => void; onSelectAgent: (agentId: string) => void; onShowAgentForm: () => void; onAgentFormChange: (form: { name: string; command: string; arguments: string }) => void; onAddAgent: (event: React.FormEvent) => void; onStart: () => void }) {
  return <section className="m-auto w-full max-w-2xl px-8 py-12"><Card className="shadow-sm"><CardHeader><p className="text-[11px] font-medium uppercase tracking-[.12em] text-primary">New agent session</p><CardTitle className="text-3xl font-medium tracking-tight">A quiet place for capable agents.</CardTitle><CardDescription className="max-w-xl text-sm leading-6">Choose a local project, start any ACP-compatible coding agent, and review its work in a focused desktop workspace.</CardDescription></CardHeader><CardContent className="grid gap-5"><label className="grid gap-2 text-xs font-medium">Project folder<div className="flex gap-2"><Input value={workspace} placeholder="Choose a local project" readOnly /><Button type="button" variant="outline" onClick={onChooseWorkspace}><FolderOpen data-icon="inline-start" />Browse</Button></div></label><label className="grid gap-2 text-xs font-medium">ACP agent<select className="h-8 rounded-md border border-input bg-input/20 px-2 text-xs outline-none focus:ring-2 focus:ring-ring/30" value={selectedAgent} onChange={(event) => onSelectAgent(event.target.value)}>{agents.map((candidate) => <option key={candidate.id} value={candidate.id}>{candidate.name}</option>)}</select></label><Button type="button" variant="link" className="w-fit px-0" onClick={onShowAgentForm}><Plus data-icon="inline-start" />Add custom ACP command</Button>{showAgentForm && <form className="grid gap-2 rounded-md border border-border bg-muted/30 p-3" onSubmit={onAddAgent}><Input required placeholder="Display name" value={agentForm.name} onChange={(event) => onAgentFormChange({ ...agentForm, name: event.target.value })} /><Input required placeholder="Executable, e.g. my-agent" value={agentForm.command} onChange={(event) => onAgentFormChange({ ...agentForm, command: event.target.value })} /><Input placeholder="Arguments, space-separated" value={agentForm.arguments} onChange={(event) => onAgentFormChange({ ...agentForm, arguments: event.target.value })} /><Button className="w-fit" type="submit">Add agent</Button></form>}<Button size="lg" className="w-fit" disabled={!workspace || !agent} onClick={onStart}><Bot data-icon="inline-start" />Launch {agent?.name ?? "agent"}</Button>{error && <p className="text-xs text-destructive">{error}</p>}</CardContent></Card></section>
}

function SessionHeader({ active, agent, onCancel }: { active: SessionSummary; agent?: AgentDefinition; onCancel: () => void }) {
  return <header className="flex items-center justify-between border-b border-border px-8 py-4"><div className="min-w-0"><div className="flex items-center gap-2 text-xs text-muted-foreground"><FolderOpen className="size-3" /><span className="truncate">{active.workspacePath}</span></div><h1 className="mt-1 font-heading text-lg font-medium">{agent?.name ?? active.agentId}</h1></div><Button variant="outline" onClick={onCancel}><Square data-icon="inline-start" />Cancel</Button></header>
}

function EventCard({ event, onRespond }: { event: AgentEvent; onRespond: (result: unknown) => void }) {
  if (event.kind === "request") return <Card className="border-primary/40 bg-primary/5"><CardHeader><CardTitle className="text-sm">Agent request · {event.data.method}</CardTitle><CardDescription>Review the request before responding.</CardDescription></CardHeader><CardContent><pre className="max-h-52 overflow-auto rounded-md bg-background p-3 text-[11px] leading-5">{JSON.stringify(event.data.params, null, 2)}</pre><div className="mt-3 flex gap-2"><Button size="sm" onClick={() => onRespond({ approved: true })}>Approve</Button><Button size="sm" variant="outline" onClick={() => onRespond({ approved: false })}>Deny</Button></div></CardContent></Card>
  return <Card className={event.kind === "message" ? "ml-auto max-w-[90%] bg-secondary" : "max-w-[90%]"}><CardContent className="pt-0"><div className="mb-2 flex items-center gap-2 text-[10px] font-medium uppercase tracking-[.1em] text-muted-foreground"><ChevronRight className="size-3" />{event.kind === "message" ? event.data.role : event.kind}</div>{event.kind === "activity" ? <pre className="max-h-52 overflow-auto rounded-md bg-background/70 p-3 text-[11px] leading-5">{JSON.stringify(event.data.payload, null, 2)}</pre> : <p className={event.kind === "protocolError" ? "text-destructive" : "whitespace-pre-wrap text-sm leading-6"}>{eventText(event)}</p>}</CardContent></Card>
}
