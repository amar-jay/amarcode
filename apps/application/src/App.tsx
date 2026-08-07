import { useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Bot,
  Cog,
  FolderOpen,
  Minus,
  Plus,
  Sparkles,
  Square,
  X,
} from "lucide-react";
import { api } from "./api";
import type { AgentDefinition, AgentEvent, SessionSummary } from "./types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { SettingsDialog } from "@/components/settings-dialog";
import { Toaster } from "@/components/ui/sonner";
import { useTheme } from "@/hooks/use-theme";
import { notify } from "@/lib/notify";
import { Input } from "@/components/ui/input";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarRail,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import { WorkbenchPromptInput, type WorkMode } from "@/components/prompt-input";
import { SessionTimeline } from "@/components/workbench/session-timeline";

const promptCacheKey = "amarcode:optimistic-prompts";
type CachedPrompt = {
  id: string;
  sessionId: string;
  text: string;
  afterAgentMessageCount: number;
};

function cachedPrompts() {
  try {
    return JSON.parse(
      localStorage.getItem(promptCacheKey) ?? "[]",
    ) as CachedPrompt[];
  } catch {
    return [] as CachedPrompt[];
  }
}

function saveCachedPrompts(prompts: CachedPrompt[]) {
  localStorage.setItem(promptCacheKey, JSON.stringify(prompts));
}

function cachePrompt(prompt: CachedPrompt) {
  saveCachedPrompts([...cachedPrompts(), prompt]);
}

function acknowledgeCachedPrompt(sessionId: string, text: string) {
  const prompts = cachedPrompts();
  const index = prompts.findIndex(
    (prompt) => prompt.sessionId === sessionId && prompt.text === text,
  );
  if (index < 0) return undefined;
  const [acknowledged] = prompts.splice(index, 1);
  saveCachedPrompts(prompts);
  return acknowledged;
}

function restoreCachedPrompts(
  sessionId: string,
  persistedEvents: AgentEvent[],
) {
  const missingPrompts = cachedPrompts().filter(
    (prompt) => prompt.sessionId === sessionId,
  );
  if (!missingPrompts.length) return persistedEvents;
  const serverUserTexts = new Set(
    persistedEvents.flatMap((event) =>
      event.kind === "message" && event.data.role === "user"
        ? [event.data.text]
        : [],
    ),
  );
  const pending = missingPrompts
    .filter((prompt) => !serverUserTexts.has(prompt.text))
    .sort(
      (left, right) =>
        left.afterAgentMessageCount - right.afterAgentMessageCount,
    );
  if (!pending.length) return persistedEvents;
  const restored: AgentEvent[] = [];
  let agentMessageCount = 0;
  let promptIndex = 0;
  for (const event of persistedEvents) {
    while (
      pending[promptIndex] &&
      pending[promptIndex].afterAgentMessageCount <= agentMessageCount
    ) {
      restored.push({
        kind: "message",
        data: { sessionId, role: "user", text: pending[promptIndex].text },
      });
      promptIndex += 1;
    }
    restored.push(event);
    if (event.kind === "message" && event.data.role !== "user")
      agentMessageCount += 1;
  }
  while (pending[promptIndex]) {
    restored.push({
      kind: "message",
      data: { sessionId, role: "user", text: pending[promptIndex].text },
    });
    promptIndex += 1;
  }
  return restored;
}

export default function App() {
  const [agents, setAgents] = useState<AgentDefinition[]>([]);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [selectedAgent, setSelectedAgent] = useState("");
  const [workspace, setWorkspace] = useState("");
  const [active, setActive] = useState<SessionSummary>();
  const [events, setEvents] = useState<AgentEvent[]>([]);
  const [isPromptWorking, setIsPromptWorking] = useState(false);
  const [isLaunching, setIsLaunching] = useState(false);
  const isLaunchingRef = useRef(false);
  const isPromptDispatchingRef = useRef(false);
  const [error, setError] = useState("");
  const [showAgentForm, setShowAgentForm] = useState(false);
  const [agentForm, setAgentForm] = useState({
    name: "",
    command: "",
    arguments: "",
  });
  const [settingsOpen, setSettingsOpen] = useState(false);
  const { theme, setTheme } = useTheme();

  useEffect(() => {
    void Promise.all([api.agents(), api.sessions()])
      .then(([loadedAgents, loadedSessions]) => {
        setAgents(loadedAgents);
        setSessions(loadedSessions);
        setSelectedAgent(loadedAgents[0]?.id ?? "");
      })
      .catch((reason) => {
        setError(String(reason));
        notify("Could not load the workbench", "error");
      });
  }, []);

  const agent = useMemo(
    () => agents.find((candidate) => candidate.id === selectedAgent),
    [agents, selectedAgent],
  );
  const activeAgent = useMemo(
    () => agents.find((candidate) => candidate.id === active?.agentId),
    [active?.agentId, agents],
  );
  const onEvent = (event: AgentEvent) => {
    if (
      event.kind === "turnComplete" ||
      event.kind === "protocolError" ||
      (event.kind === "status" &&
        ["failed", "stopped"].includes(event.data.status))
    ) {
      isPromptDispatchingRef.current = false;
      setIsPromptWorking(false);
    }
    if (event.kind === "protocolError") {
      setError(event.data.message);
      notify(event.data.message, "error");
    }
    if (event.kind === "status") {
      setActive((current) =>
        current?.id === event.data.sessionId
          ? { ...current, status: event.data.status }
          : current,
      );
      setSessions((current) =>
        current.map((session) =>
          session.id === event.data.sessionId
            ? { ...session, status: event.data.status }
            : session,
        ),
      );
    }
    const acknowledgedPrompt =
      event.kind === "message" && event.data.role === "user"
        ? acknowledgeCachedPrompt(event.data.sessionId, event.data.text)
        : undefined;
    setEvents((current) => {
      if (event.kind === "message" && event.data.role === "user") {
        const optimisticIndex = acknowledgedPrompt
          ? current.findIndex(
              (existing) =>
                existing.kind === "message" &&
                existing.data.clientId === acknowledgedPrompt.id,
            )
          : -1;
        if (optimisticIndex >= 0) {
          const next = [...current];
          next[optimisticIndex] = event;
          return next;
        }
        if (
          current.some(
            (existing) =>
              existing.kind === "message" &&
              existing.data.role === "user" &&
              existing.data.sessionId === event.data.sessionId &&
              existing.data.text === event.data.text,
          )
        )
          return current;
      }
      return [...current, event];
    });
  };

  async function chooseWorkspace() {
    const path = await open({
      directory: true,
      multiple: false,
      title: "Choose a project folder",
    });
    if (typeof path === "string") setWorkspace(path);
  }

  async function start() {
    if (!workspace || !agent || isLaunchingRef.current) return;
    isLaunchingRef.current = true;
    setIsLaunching(true);
    setError("");
    setEvents([]);
    try {
      const session = await api.start(workspace, agent, onEvent);
      setActive(session);
      setSessions((current) => [session, ...current]);
      notify(`${agent.name} session started`, "success");
    } catch (reason) {
      setError(String(reason));
      notify("Unable to start the agent session", "error");
    } finally {
      isLaunchingRef.current = false;
      setIsLaunching(false);
    }
  }

  async function restore(session: SessionSummary) {
    try {
      setActive(session);
      setEvents(restoreCachedPrompts(session.id, await api.events(session.id)));
    } catch (reason) {
      setError(String(reason));
      notify("Could not restore that session", "error");
    }
  }

  async function submitPrompt({
    text,
    files,
    sources,
    mode,
  }: {
    text: string;
    files: { filename?: string }[];
    sources: { title?: string; filename?: string }[];
    mode: WorkMode;
  }) {
    if (
      !active ||
      (!text && files.length === 0) ||
      isPromptDispatchingRef.current
    )
      return;
    if (active.status !== "running") {
      notify(
        "This saved session is no longer live. Start a new session to continue.",
        "error",
      );
      return;
    }
    const attachmentNames = files.map((file) => file.filename ?? "Attachment");
    const visibleText =
      text || `Review the attached context: ${attachmentNames.join(", ")}`;
    const sourceNames = sources.map(
      (source) => source.title ?? source.filename ?? "Workspace context",
    );
    const acpPrompt = [
      // `[Work mode: ${mode}]`,
      attachmentNames.length
        ? `[Local context attachments: ${attachmentNames.join(", ")}]`
        : "",
      sourceNames.length
        ? `[Referenced workspace context: ${sourceNames.join(", ")}]`
        : "",
      visibleText,
    ]
      .filter(Boolean)
      .join("\n\n");
    const promptId = crypto.randomUUID();
    isPromptDispatchingRef.current = true;
    cachePrompt({
      id: promptId,
      sessionId: active.id,
      text: visibleText,
      afterAgentMessageCount: events.filter(
        (event) => event.kind === "message" && event.data.role !== "user",
      ).length,
    });
    setEvents((current) => [
      ...current,
      {
        kind: "message",
        data: {
          sessionId: active.id,
          role: "user",
          text: visibleText,
          clientId: promptId,
        },
      },
    ]);
    setIsPromptWorking(true);
    try {
      await api.prompt(active.id, acpPrompt, visibleText);
    } catch (reason) {
      isPromptDispatchingRef.current = false;
      setIsPromptWorking(false);
      setError(String(reason));
      notify("Prompt could not be sent", "error");
      throw reason;
    }
  }

  async function stopPrompt() {
    if (!active || active.status !== "running") return;
    try {
      await api.cancel(active.id);
      isPromptDispatchingRef.current = false;
      setIsPromptWorking(false);
      notify("Agent run cancelled", "success");
    } catch (reason) {
      setError(String(reason));
      notify("Could not cancel the agent", "error");
    }
  }

  async function addAgent(event: React.FormEvent) {
    event.preventDefault();
    const created: AgentDefinition = {
      id: crypto.randomUUID(),
      name: agentForm.name,
      command: agentForm.command,
      arguments: agentForm.arguments.split(" ").filter(Boolean),
      environment: [],
      isPreset: false,
    };
    try {
      await api.saveAgent(created);
      setAgents((current) => [...current, created]);
      setSelectedAgent(created.id);
      setShowAgentForm(false);
      setAgentForm({ name: "", command: "", arguments: "" });
      notify(`${created.name} was added`, "success");
    } catch (reason) {
      setError(String(reason));
      notify("Could not add that agent", "error");
    }
  }

  return (
    <SidebarProvider>
      <TopBar />
      <WorkbenchSidebar
        active={active}
        sessions={sessions}
        onNewSession={() => setActive(undefined)}
        onRestore={restore}
        onSettings={() => setSettingsOpen(true)}
      />
      <SidebarInset className="min-w-0 pt-9">
        {!active ? (
          <NewSession
            agent={agent}
            workspace={workspace}
            agents={agents}
            selectedAgent={selectedAgent}
            showAgentForm={showAgentForm}
            agentForm={agentForm}
            error={error}
            isLaunching={isLaunching}
            onChooseWorkspace={() => void chooseWorkspace()}
            onSelectAgent={setSelectedAgent}
            onShowAgentForm={() => setShowAgentForm((shown) => !shown)}
            onAgentFormChange={setAgentForm}
            onAddAgent={addAgent}
            onStart={() => void start()}
          />
        ) : (
          <>
            <SessionHeader
              active={active}
              agent={activeAgent}
              isWorking={isPromptWorking}
              onCancel={() => void stopPrompt()}
            />
            <div className="min-h-0 flex-1">
              <SessionTimeline
                events={events}
                isWorking={isPromptWorking}
                onRespond={async (event, result) => {
                  if (event.kind === "request") {
                    const requestId =
                      event.data.requestId ??
                      (
                        event.data as typeof event.data & {
                          request_id?: string | number;
                        }
                      ).request_id;
                    if (requestId === undefined)
                      throw new Error(
                        "This agent request has no response ID. Restart the session and try again.",
                      );
                    await api.respond(active.id, requestId, result);
                  }
                }}
              />
            </div>
            <div className="sticky bottom-0 z-20 shrink-0 border-t border-border bg-background/95 px-8 py-4 backdrop-blur-sm">
              <WorkbenchPromptInput
                agent={activeAgent}
                workspacePath={active.workspacePath}
                isWorking={isPromptWorking}
                onStop={() => void stopPrompt()}
                onSubmit={submitPrompt}
              />
              {error && (
                <p className="mx-auto mt-2 max-w-3xl text-xs text-destructive">
                  {error}
                </p>
              )}
            </div>
          </>
        )}
      </SidebarInset>
      <SettingsDialog
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        theme={theme}
        onThemeChange={setTheme}
      />
      <Toaster position="bottom-right" closeButton theme={theme} />
    </SidebarProvider>
  );
}

function TopBar() {
  return (
    <div className="fixed inset-x-0 top-0 z-[60] flex h-9 items-center border-b border-border bg-card">
      <div
        data-tauri-drag-region
        className="flex h-full min-w-0 flex-1 items-center gap-2 px-3 select-none"
      >
        <img src="/acp-mark.svg" alt="" className="size-4" />
        <span className="text-xs font-medium">AMARCODE</span>
        <span className="border-l border-border pl-2 text-xs text-muted-foreground">
          Agent workspace
        </span>
      </div>
      <div className="flex h-full">
        <button
          type="button"
          className="grid h-9 w-12 place-items-center text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          onClick={() => void getCurrentWindow().minimize()}
          aria-label="Minimize"
        >
          <Minus className="size-4" />
        </button>
        <button
          type="button"
          className="grid h-9 w-12 place-items-center text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          onClick={() => void getCurrentWindow().toggleMaximize()}
          aria-label="Maximize"
        >
          <Square className="size-3.5" />
        </button>
        <button
          type="button"
          className="grid h-9 w-12 place-items-center text-muted-foreground transition-colors hover:bg-destructive hover:text-background"
          onClick={() => void getCurrentWindow().close()}
          aria-label="Close"
        >
          <X className="size-4" />
        </button>
      </div>
    </div>
  );
}

function WorkbenchSidebar({
  active,
  sessions,
  onNewSession,
  onRestore,
  onSettings,
}: {
  active?: SessionSummary;
  sessions: SessionSummary[];
  onNewSession: () => void;
  onRestore: (session: SessionSummary) => void;
  onSettings: () => void;
}) {
  return (
    <Sidebar
      variant="floating"
      collapsible="icon"
      className="inset-y-auto! top-9! bottom-0! h-auto!"
    >
      <SidebarHeader className="border-b border-sidebar-border">
        <div className="flex items-center gap-1 px-2 py-1">
          <SidebarTrigger />
          <span className="font-heading text-sm font-semibold tracking-tight group-data-[collapsible=icon]:hidden">
            AMARCODE
          </span>
        </div>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              isActive={!active}
              tooltip="New session"
              onClick={onNewSession}
            >
              <Plus />
              <span>New session</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>
      <SidebarContent className="px-2 py-2">
        <SidebarGroup className="p-0">
          <SidebarGroupLabel>Recent sessions</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {sessions.length === 0 ? (
                <p className="px-2 py-3 text-xs text-muted-foreground group-data-[collapsible=icon]:hidden">
                  No saved sessions yet.
                </p>
              ) : (
                sessions.map((session) => (
                  <SidebarMenuItem key={session.id}>
                    <SidebarMenuButton
                      isActive={active?.id === session.id}
                      tooltip={
                        session.workspacePath.split("/").pop() ??
                        session.workspacePath
                      }
                      onClick={() => onRestore(session)}
                    >
                      <FolderOpen />
                      <span>{session.workspacePath.split("/").pop()}</span>
                      <Badge
                        variant="secondary"
                        className="ml-auto text-[10px] group-data-[collapsible=icon]:hidden"
                      >
                        {session.status}
                      </Badge>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                ))
              )}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter className="border-t border-sidebar-border">
        <div className="px-2 py-1 text-[11px] leading-5 text-muted-foreground group-data-[collapsible=icon]:hidden">
          Local-only ACP client.
          <br />
          Secrets stay in the OS keychain.
        </div>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton tooltip="Settings" onClick={onSettings}>
              <Cog />
              <span>Settings</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  );
}

function NewSession({
  agent,
  workspace,
  agents,
  selectedAgent,
  showAgentForm,
  agentForm,
  error,
  isLaunching,
  onChooseWorkspace,
  onSelectAgent,
  onShowAgentForm,
  onAgentFormChange,
  onAddAgent,
  onStart,
}: {
  agent?: AgentDefinition;
  workspace: string;
  agents: AgentDefinition[];
  selectedAgent: string;
  showAgentForm: boolean;
  agentForm: { name: string; command: string; arguments: string };
  error: string;
  isLaunching: boolean;
  onChooseWorkspace: () => void;
  onSelectAgent: (agentId: string) => void;
  onShowAgentForm: () => void;
  onAgentFormChange: (form: {
    name: string;
    command: string;
    arguments: string;
  }) => void;
  onAddAgent: (event: React.FormEvent) => void;
  onStart: () => void;
}) {
  return (
    <section className="m-auto w-full max-w-2xl px-8 py-12">
      <Card className="shadow-sm">
        <CardHeader>
          <p className="text-[11px] font-medium uppercase tracking-[.12em] text-primary">
            New agent session
          </p>
          <CardTitle className="text-3xl font-medium tracking-tight">
            A quiet place for capable agents.
          </CardTitle>
          <CardDescription className="max-w-xl text-sm leading-6">
            Choose a local project, start any ACP-compatible coding agent, and
            review its work in a focused desktop workspace.
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-5">
          <label className="grid gap-2 text-xs font-medium">
            Project folder
            <div className="flex gap-2">
              <Input
                value={workspace}
                placeholder="Choose a local project"
                readOnly
              />
              <Button
                type="button"
                variant="outline"
                onClick={onChooseWorkspace}
              >
                <FolderOpen data-icon="inline-start" />
                Browse
              </Button>
            </div>
          </label>
          <label className="grid gap-2 text-xs font-medium">
            ACP agent
            <select
              className="h-8 rounded-md border border-input bg-input/20 px-2 text-xs outline-none focus:ring-2 focus:ring-ring/30"
              value={selectedAgent}
              onChange={(event) => onSelectAgent(event.target.value)}
            >
              {agents.map((candidate) => (
                <option key={candidate.id} value={candidate.id}>
                  {candidate.name}
                </option>
              ))}
            </select>
          </label>
          <Button
            type="button"
            variant="link"
            className="w-fit px-0"
            onClick={onShowAgentForm}
          >
            <Plus data-icon="inline-start" />
            Add custom ACP command
          </Button>
          {showAgentForm && (
            <form
              className="grid gap-2 rounded-md border border-border bg-muted/30 p-3"
              onSubmit={onAddAgent}
            >
              <Input
                required
                placeholder="Display name"
                value={agentForm.name}
                onChange={(event) =>
                  onAgentFormChange({ ...agentForm, name: event.target.value })
                }
              />
              <Input
                required
                placeholder="Executable, e.g. my-agent"
                value={agentForm.command}
                onChange={(event) =>
                  onAgentFormChange({
                    ...agentForm,
                    command: event.target.value,
                  })
                }
              />
              <Input
                placeholder="Arguments, space-separated"
                value={agentForm.arguments}
                onChange={(event) =>
                  onAgentFormChange({
                    ...agentForm,
                    arguments: event.target.value,
                  })
                }
              />
              <Button className="w-fit" type="submit">
                Add agent
              </Button>
            </form>
          )}
          <Button
            size="lg"
            className="w-fit"
            disabled={!workspace || !agent || isLaunching}
            onClick={onStart}
          >
            <Bot data-icon="inline-start" />
            {isLaunching
              ? "Starting session…"
              : `Launch ${agent?.name ?? "agent"}`}
          </Button>
          {error && <p className="text-xs text-destructive">{error}</p>}
        </CardContent>
      </Card>
    </section>
  );
}

function SessionHeader({
  active,
  agent,
  isWorking,
  onCancel,
}: {
  active: SessionSummary;
  agent?: AgentDefinition;
  isWorking: boolean;
  onCancel: () => void;
}) {
  const agentName = agent?.name ?? active.agentId;
  return (
    <header className="flex min-h-20 items-center justify-between gap-4 border-b border-border bg-background/90 px-8 py-3 backdrop-blur-sm">
      <div className="flex min-w-0 items-center gap-3">
        <div className="grid size-9 shrink-0 place-items-center rounded-lg border border-border bg-card shadow-sm">
          <Bot className="size-4 text-primary" />
        </div>
        <div className="min-w-0">
          <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <FolderOpen className="size-3 shrink-0" />
            <span className="truncate">{active.workspacePath}</span>
          </div>
          <h1 className="mt-0.5 truncate text-sm font-semibold tracking-tight">
            {agentName}
          </h1>
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-3">
        <Badge variant="secondary" className="gap-1.5 rounded-full px-2.5 py-1">
          <span
            className={`size-1.5 rounded-full ${isWorking ? "animate-pulse bg-primary" : "bg-emerald-500"}`}
          />
          {isWorking ? "Working" : "Ready"}
        </Badge>
        {isWorking && (
          <Button size="sm" variant="outline" onClick={onCancel}>
            <Square data-icon="inline-start" />
            Stop
          </Button>
        )}
        <Sparkles className="hidden size-4 text-muted-foreground sm:block" />
      </div>
    </header>
  );
}
