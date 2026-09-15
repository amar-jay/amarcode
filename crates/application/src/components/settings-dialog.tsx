import { useEffect, useState } from "react";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Check,
  Bot,
  ChevronsUpDown,
  Cog,
  Monitor,
  Moon,
  Palette,
  RotateCcw,
  SlidersHorizontal,
  Sun,
  LoaderCircle,
  Download,
  Database,
  DatabaseZap,
  FolderOpen,
  Trash2,
  TriangleAlert,
} from "lucide-react";
import { daemonApi, type ApplicationCleanupStatus } from "@/api";
import type { Palette as AppPalette, Theme } from "@/state";
import {
  latestTurnByChatAtom,
  liveChatIsWorkingAtom,
  verboseReasoningAtom,
} from "@/state";
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from "@/components/ui/breadcrumb";
import { Button } from "@/components/ui/button";
import {
  AlertDialog,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Separator } from "@/components/ui/separator";
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
} from "@/components/ui/sidebar";
import { Switch } from "@/components/ui/switch";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldLabel,
  FieldSet,
} from "@/components/ui/field";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import type { AgentInfo } from "@/types";
import { AgentLogo } from "@/components/agent-logo";
import { installAgentAtom } from "@/state/agents";
import { notify } from "@/lib/notify";

type SettingsPage = "appearance" | "general" | "agent" | "daemon";

const navigation: { id: SettingsPage; label: string; icon: typeof Palette }[] =
  [
    { id: "appearance", label: "Appearance", icon: Palette },
    { id: "agent", label: "Agent defaults", icon: Bot },
    { id: "daemon", label: "Daemon data", icon: Database },
    { id: "general", label: "General", icon: SlidersHorizontal },
  ];

const themeChoices: {
  value: Theme;
  label: string;
  description: string;
  Icon: typeof Sun;
}[] = [
  { value: "light", label: "Light", description: "Warm and clear", Icon: Sun },
  { value: "dark", label: "Dark", description: "Soft low-light", Icon: Moon },
  {
    value: "system",
    label: "System",
    description: "Match your device",
    Icon: Monitor,
  },
];

const paletteChoices: {
  value: AppPalette;
  label: string;
  description: string;
}[] = [
  {
    value: "monochrome",
    label: "Monochrome",
    description: "Neutral grayscale surfaces",
  },
  {
    value: "ember",
    label: "Ember",
    description: "Warm amber accents",
  },
];

function usePreference(key: string, defaultValue: boolean) {
  const [value, setValue] = useState(() =>
    localStorage.getItem(key) === null
      ? defaultValue
      : localStorage.getItem(key) === "true",
  );
  useEffect(() => localStorage.setItem(key, String(value)), [key, value]);
  return [value, setValue] as const;
}

export function SettingsDialog({
  open,
  onOpenChange,
  theme,
  onThemeChange,
  palette,
  onPaletteChange,
  agents,
  defaultAgentId,
  onDefaultAgentChange,
  defaultWorkspacePath,
  onDefaultWorkspacePathChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
  palette: AppPalette;
  onPaletteChange: (palette: AppPalette) => void;
  agents: AgentInfo[];
  defaultAgentId: string;
  onDefaultAgentChange: (agentId: string) => void;
  defaultWorkspacePath: string;
  onDefaultWorkspacePathChange: (path: string) => void;
}) {
  const [page, setPage] = useState<SettingsPage>("appearance");
  const [restoreWorkspace, setRestoreWorkspace] = usePreference(
    "amarcode-restore-workspace",
    true,
  );
  const [timestamps, setTimestamps] = usePreference(
    "amarcode-show-timestamps",
    false,
  );
  const [verboseReasoning, setVerboseReasoning] = useAtom(verboseReasoningAtom);
  const [dialogContentElement, setDialogContentElement] =
    useState<HTMLDivElement | null>(null);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        ref={setDialogContentElement}
        className="h-125! w-200! max-w-200! overflow-visible p-0"
      >
        <DialogTitle className="sr-only">Settings</DialogTitle>
        <DialogDescription className="sr-only">
          Customize your amarcode preferences.
        </DialogDescription>
        <SidebarProvider
          className="h-full min-h-0 items-start overflow-hidden rounded-xl"
          style={{ "--sidebar-width": "12.5rem" } as React.CSSProperties}
        >
          <Sidebar
            collapsible="none"
            className="hidden border-r border-border bg-muted/25 md:flex"
          >
            <div className="flex h-16 items-center gap-2 px-5">
              <Cog className="size-4 text-muted-foreground" />
              <span className="text-sm font-medium">Settings</span>
            </div>
            <SidebarContent className="px-3">
              <SidebarGroup className="p-0">
                <SidebarGroupContent>
                  <SidebarMenu>
                    {navigation.map(({ id, label, icon: Icon }) => (
                      <SidebarMenuItem key={id}>
                        <SidebarMenuButton
                          isActive={page === id}
                          onClick={() => setPage(id)}
                        >
                          <Icon />
                          <span>{label}</span>
                        </SidebarMenuButton>
                      </SidebarMenuItem>
                    ))}
                  </SidebarMenu>
                </SidebarGroupContent>
              </SidebarGroup>
            </SidebarContent>
          </Sidebar>
          <main className="flex h-full min-w-0 flex-1 flex-col overflow-hidden bg-popover">
            <header className="flex h-16 shrink-0 items-center px-6">
              <Breadcrumb>
                <BreadcrumbList>
                  <BreadcrumbItem className="hidden md:block">
                    <span className="text-muted-foreground">Settings</span>
                  </BreadcrumbItem>
                  <BreadcrumbSeparator className="hidden md:block" />
                  <BreadcrumbItem>
                    <BreadcrumbPage>
                      {navigation.find((item) => item.id === page)?.label}
                    </BreadcrumbPage>
                  </BreadcrumbItem>
                </BreadcrumbList>
              </Breadcrumb>
            </header>
            <div className="flex flex-1 flex-col overflow-y-hidden p-6">
              {page === "appearance" ? (
                <AppearancePanel
                  theme={theme}
                  onThemeChange={onThemeChange}
                  palette={palette}
                  onPaletteChange={onPaletteChange}
                />
              ) : page === "agent" ? (
                <AgentDefaultsPanel
                  agents={agents}
                  defaultAgentId={defaultAgentId}
                  onDefaultAgentChange={onDefaultAgentChange}
                  defaultWorkspacePath={defaultWorkspacePath}
                  onDefaultWorkspacePathChange={onDefaultWorkspacePathChange}
                  portalContainer={dialogContentElement}
                />
              ) : page === "daemon" ? (
                <DaemonDataPanel />
              ) : (
                <GeneralPanel
                  restoreWorkspace={restoreWorkspace}
                  setRestoreWorkspace={setRestoreWorkspace}
                  timestamps={timestamps}
                  setTimestamps={setTimestamps}
                  verboseReasoning={verboseReasoning}
                  setVerboseReasoning={setVerboseReasoning}
                />
              )}
            </div>
          </main>
        </SidebarProvider>
      </DialogContent>
    </Dialog>
  );
}

const retentionChoices = [
  { days: 1, label: "1 day" },
  { days: 7, label: "7 days" },
  { days: 30, label: "30 days" },
  { days: 90, label: "90 days" },
];

function DaemonDataPanel() {
  const latestTurns = useAtomValue(latestTurnByChatAtom);
  const activeChatWorking = useAtomValue(liveChatIsWorkingAtom);
  const [config, setConfig] = useState<{
    store_acp_events: boolean;
    acp_event_retention_days: number;
  } | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [vacuuming, setVacuuming] = useState(false);
  const [vacuumResult, setVacuumResult] = useState<{
    after_bytes: number;
    reclaimed_bytes: number;
  } | null>(null);

  useEffect(() => {
    let cancelled = false;
    void daemonApi
      .getDaemonConfig()
      .then((value) => {
        if (!cancelled) setConfig(value);
      })
      .catch((cause: unknown) => {
        if (!cancelled) {
          setError(
            cause instanceof Error
              ? cause.message
              : "Could not load daemon settings.",
          );
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const updateConfig = async (
    storeAcpEvents: boolean,
    retentionDays: number,
  ) => {
    if (saving) return;
    setSaving(true);
    setError(null);
    try {
      const value = await daemonApi.setDaemonConfig(
        storeAcpEvents,
        retentionDays,
      );
      setConfig(value);
    } catch (cause) {
      setError(
        cause instanceof Error
          ? cause.message
          : "Could not save daemon settings.",
      );
    } finally {
      setSaving(false);
    }
  };

  const enabled = config?.store_acp_events ?? false;
  const retentionDays = config?.acp_event_retention_days ?? 7;
  const agentWorking =
    activeChatWorking ||
    Object.values(latestTurns).some((turn) => turn.status === "started");
  const formatBytes = (bytes: number) => {
    if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  const vacuumDatabase = async () => {
    if (vacuuming || agentWorking) return;
    setVacuuming(true);
    setVacuumResult(null);
    setError(null);
    try {
      setVacuumResult(await daemonApi.vacuumDatabase());
    } catch (cause) {
      setError(
        cause instanceof Error
          ? cause.message
          : "Could not compact the database.",
      );
    } finally {
      setVacuuming(false);
    }
  };

  return (
    <div className="mx-auto w-full max-w-132">
      <h2 className="text-base font-medium">Daemon data</h2>
      <p className="mt-1 text-xs leading-5 text-muted-foreground">
        Control diagnostic data recorded by the background service.
      </p>
      <Separator className="my-5" />
      <div className="rounded-xl border border-border">
        <div className="flex items-center gap-4 px-4 py-3">
          <div className="min-w-0 flex-1">
            <p className="text-xs font-medium">Store raw ACP events</p>
            <p className="mt-1 text-[11px] leading-4 text-muted-foreground">
              Save protocol traffic for diagnostics. It may include prompts,
              tool output, and file content.
            </p>
          </div>
          {config === null ? (
            <LoaderCircle className="size-4 animate-spin text-muted-foreground" />
          ) : (
            <Switch
              checked={enabled}
              disabled={saving}
              aria-label="Store raw ACP events"
              onCheckedChange={(checked) =>
                void updateConfig(checked, retentionDays)
              }
            />
          )}
        </div>
        <Separator />
        <div className="flex items-center gap-4 px-4 py-3">
          <div className="min-w-0 flex-1">
            <p className="text-xs font-medium">Retention</p>
            <p className="mt-1 text-[11px] leading-4 text-muted-foreground">
              Delete older raw events. Changes prune them immediately.
            </p>
          </div>
          <Select
            value={String(retentionDays)}
            disabled={!config || !enabled || saving}
            onValueChange={(value) => void updateConfig(enabled, Number(value))}
          >
            <SelectTrigger className="w-28" aria-label="ACP event retention">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {retentionChoices.map((choice) => (
                <SelectItem key={choice.days} value={String(choice.days)}>
                  {choice.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <Separator />
        <section
          className="flex items-center gap-4 px-4 py-3"
          aria-labelledby="database-maintenance-title"
        >
          <div className="min-w-0 flex-1">
            <h3 id="database-maintenance-title" className="text-xs font-medium">
              Compact database
            </h3>
            <p className="mt-1 text-[11px] leading-4 text-muted-foreground">
              Return unused SQLite pages to disk while no agent is working.
            </p>
            {vacuumResult && (
              <p className="mt-2 text-[11px] text-emerald-600 dark:text-emerald-400">
                Reclaimed {formatBytes(vacuumResult.reclaimed_bytes)} · Database
                is now {formatBytes(vacuumResult.after_bytes)}
              </p>
            )}
          </div>
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  variant="outline"
                  size="icon-sm"
                  disabled={vacuuming || agentWorking}
                  aria-label={
                    vacuuming ? "Compacting database" : "Compact database"
                  }
                  onClick={() => void vacuumDatabase()}
                >
                  {vacuuming ? (
                    <LoaderCircle className="animate-spin" />
                  ) : (
                    <DatabaseZap />
                  )}
                </Button>
              </TooltipTrigger>
              <TooltipContent side="left">
                {agentWorking
                  ? "Available when agents are idle"
                  : "Compact database"}
              </TooltipContent>
            </Tooltip>
          </TooltipProvider>
        </section>
      </div>
      <p className="mt-3 text-[11px] leading-4 text-muted-foreground">
        Raw event recording is off by default. Chat messages and reasoning
        summaries are stored separately.
      </p>
      {error && (
        <Alert className="mt-4" variant="destructive" aria-live="assertive">
          <TriangleAlert />
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}
    </div>
  );
}

function AgentDefaultsPanel({
  agents,
  defaultAgentId,
  onDefaultAgentChange,
  defaultWorkspacePath,
  onDefaultWorkspacePathChange,
  portalContainer,
}: {
  agents: AgentInfo[];
  defaultAgentId: string;
  onDefaultAgentChange: (agentId: string) => void;
  defaultWorkspacePath: string;
  onDefaultWorkspacePathChange: (path: string) => void;
  portalContainer: HTMLElement | null;
}) {
  const [agentPickerOpen, setAgentPickerOpen] = useState(false);
  const [installingId, setInstallingId] = useState<string | null>(null);
  const installAgent = useSetAtom(installAgentAtom);
  const selectedAgent = agents.find((agent) => agent.id === defaultAgentId);
  const availableAgents = agents.filter((agent) => agent.available);
  const installableAgents = agents.filter((agent) => !agent.available);

  const chooseDefaultWorkspace = async () => {
    try {
      const path = await open({
        directory: true,
        multiple: false,
        title: "Choose the default workspace",
        defaultPath: defaultWorkspacePath || undefined,
      });
      if (typeof path === "string") onDefaultWorkspacePathChange(path);
    } catch (error) {
      console.error("Error choosing default workspace:", error);
      notify("Unable to open the directory picker.", "error");
    }
  };

  const handleInstall = async (agent: AgentInfo) => {
    if (installingId) return;
    setInstallingId(agent.id);
    try {
      const result = await installAgent(agent.id);
      onDefaultAgentChange(result.agent.id);
      setAgentPickerOpen(false);
      const name = result.agent.name.replace(/\s*\bACP\s*$/i, "");
      if (result.runtime_status === "ready") {
        notify(`${name} installed`, "success");
      } else if (result.runtime_status === "auth_required") {
        notify(`${name} installed — sign in required`, "info");
      } else {
        notify(
          result.runtime_message ?? `${name} installed but is not ready`,
          "error",
        );
      }
    } catch (error) {
      console.error("Failed to install agent:", error);
      notify(
        error instanceof Error
          ? error.message
          : `Failed to install ${agent.name.replace(/\s*\bACP\s*$/i, "")}.`,
        "error",
      );
    } finally {
      setInstallingId(null);
    }
  };

  const renderAgent = (agent: AgentInfo) => {
    const installing = installingId === agent.id;
    return (
      <CommandItem
        className="grid w-full! cursor-pointer grid-cols-[1rem_minmax(0,1fr)_2rem] items-center gap-2 rounded-none [&>svg:last-child]:hidden data-[disabled=true]:cursor-not-allowed data-[disabled=true]:opacity-50"
        disabled={installingId !== null && !installing}
        title={agent.unavailable_reason ?? undefined}
        key={agent.id}
        value={`${agent.name} ${agent.id}`}
        onSelect={() => {
          if (!agent.available) return;
          onDefaultAgentChange(agent.id);
          setAgentPickerOpen(false);
        }}
      >
        <AgentLogo icon={agent.icon} />
        <span className="min-w-0 truncate">
          {agent.name.replace(/\s*\bACP\s*$/i, "")}
        </span>
        {!agent.available ? (
          <button
            type="button"
            className="inline-flex size-6 items-center justify-center justify-self-end rounded-md text-primary hover:bg-foreground/10 disabled:opacity-50"
            aria-label={`Install ${agent.name.replace(/\s*\bACP\s*$/i, "")}`}
            title={`Install ${agent.name.replace(/\s*\bACP\s*$/i, "")}`}
            disabled={installingId !== null}
            onClick={(event) => {
              event.preventDefault();
              event.stopPropagation();
              void handleInstall(agent);
            }}
          >
            {installing ? (
              <LoaderCircle className="size-3.5 animate-spin" />
            ) : (
              <Download className="size-3.5" />
            )}
          </button>
        ) : (
          <Check
            className={`size-4 justify-self-end ${agent.id === defaultAgentId ? "opacity-100" : "opacity-0"}`}
          />
        )}
      </CommandItem>
    );
  };
  return (
    <div className="mx-auto w-full max-w-132">
      <h2 className="text-base font-medium">Agent defaults</h2>
      <p className="mt-1 text-xs leading-5 text-muted-foreground">
        Choose the agent used for new chats. Session configuration is provided
        by each agent and its last choices are remembered locally.
      </p>
      <Separator className="my-6" />
      <FieldSet className="gap-7">
        <Field>
          <FieldContent>
            <FieldLabel htmlFor="default-acp-agent">
              Default ACP agent
            </FieldLabel>
            <FieldDescription>
              Choose the agent preselected in the new-chat composer.
            </FieldDescription>
          </FieldContent>
          <Popover open={agentPickerOpen} onOpenChange={setAgentPickerOpen}>
            <PopoverTrigger asChild>
              <Button
                id="default-acp-agent"
                variant="outline"
                role="combobox"
                aria-expanded={agentPickerOpen}
                className="w-full cursor-pointer justify-between font-normal"
              >
                <span className="flex min-w-0 items-center gap-2">
                  <AgentLogo icon={selectedAgent?.icon} />
                  {selectedAgent?.name.replace(/\s*\bACP\s*$/i, "") ??
                    "Choose an agent"}
                </span>
                <ChevronsUpDown className="size-4 text-muted-foreground" />
              </Button>
            </PopoverTrigger>
            <PopoverContent
              align="start"
              className="w-(--radix-popover-trigger-width) p-0"
              portalContainer={portalContainer}
            >
              <Command>
                <CommandInput placeholder="Search ACP agents…" />
                <CommandList className="overscroll-contain">
                  <CommandEmpty>No matching ACP agents.</CommandEmpty>
                  {availableAgents.length > 0 && (
                    <CommandGroup heading="Available">
                      {availableAgents.map(renderAgent)}
                    </CommandGroup>
                  )}
                  {installableAgents.length > 0 && (
                    <CommandGroup heading="Install">
                      {installableAgents.map(renderAgent)}
                    </CommandGroup>
                  )}
                </CommandList>
              </Command>
            </PopoverContent>
          </Popover>
        </Field>
        <Field>
          <FieldContent>
            <FieldLabel htmlFor="default-workspace">
              Default workspace
            </FieldLabel>
            <FieldDescription>
              Project folder preselected when starting a new chat.
            </FieldDescription>
          </FieldContent>
          <div className="flex min-w-0 gap-2">
            <Button
              id="default-workspace"
              type="button"
              variant="outline"
              className="min-w-0 flex-1 justify-start font-normal"
              title={defaultWorkspacePath || "Choose a workspace"}
              onClick={() => void chooseDefaultWorkspace()}
            >
              <FolderOpen className="size-4 shrink-0" />
              <span className="truncate">
                {defaultWorkspacePath || "Choose a workspace"}
              </span>
            </Button>
            {defaultWorkspacePath && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => onDefaultWorkspacePathChange("")}
              >
                Clear
              </Button>
            )}
          </div>
        </Field>
      </FieldSet>
    </div>
  );
}

function AppearancePanel({
  theme,
  onThemeChange,
  palette,
  onPaletteChange,
}: {
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
  palette: AppPalette;
  onPaletteChange: (palette: AppPalette) => void;
}) {
  return (
    <div className="mx-auto w-full max-w-132">
      <div>
        <p className="text-sm font-medium">Theme</p>
        <p className="mt-1 text-xs leading-5 text-muted-foreground">
          Choose a theme or follow the operating system preference.
        </p>
      </div>
      <RadioGroup
        value={theme}
        onValueChange={(value) => onThemeChange(value as Theme)}
        className="mt-3 grid grid-cols-3 gap-3"
      >
        {themeChoices.map(({ value, label, description, Icon }) => (
          <label
            htmlFor={`theme-${value}`}
            key={value}
            data-active={theme === value || undefined}
            className="group relative cursor-pointer rounded-xl border border-border p-2.5 transition-all hover:-translate-y-0.5 hover:shadow-sm data-active:border-primary data-active:ring-2 data-active:ring-primary/20"
          >
            <ThemePreview theme={value} palette={palette} />
            <div className="mt-2 flex items-start gap-2">
              <RadioGroupItem
                id={`theme-${value}`}
                value={value}
                className="mt-0.5"
              />
              <span>
                <span className="flex items-center gap-1 text-xs font-medium">
                  <Icon className="size-3" />
                  {label}
                </span>
                <span className="mt-0.5 block text-[10px] leading-4 text-muted-foreground">
                  {description}
                </span>
              </span>
            </div>
            {theme === value && (
              <span className="absolute right-3 top-3 grid size-4 place-items-center rounded-full bg-primary text-primary-foreground">
                <Check className="size-2.5" />
              </span>
            )}
          </label>
        ))}
      </RadioGroup>
      <div className="mt-5">
        <p className="text-sm font-medium">Palette</p>
        <p className="mt-1 text-xs leading-5 text-muted-foreground">
          Choose the accent and surface treatment used across the app.
        </p>
      </div>
      <RadioGroup
        value={palette}
        onValueChange={(value) => onPaletteChange(value as AppPalette)}
        className="mt-3 grid grid-cols-2 gap-3"
      >
        {paletteChoices.map(({ value, label, description }) => (
          <label
            htmlFor={`palette-${value}`}
            key={value}
            data-active={palette === value || undefined}
            className="group relative cursor-pointer rounded-xl border border-border p-2.5 transition-all hover:-translate-y-0.5 hover:shadow-sm data-active:border-primary data-active:ring-2 data-active:ring-primary/20"
          >
            <PalettePreview palette={value} />
            <div className="mt-2 flex items-start gap-2">
              <RadioGroupItem
                id={`palette-${value}`}
                value={value}
                className="mt-0.5"
              />
              <span>
                <span className="block text-xs font-medium">{label}</span>
                <span className="mt-0.5 block text-[10px] leading-4 text-muted-foreground">
                  {description}
                </span>
              </span>
            </div>
          </label>
        ))}
      </RadioGroup>
    </div>
  );
}

function GeneralPanel({
  restoreWorkspace,
  setRestoreWorkspace,
  timestamps,
  setTimestamps,
  verboseReasoning,
  setVerboseReasoning,
}: {
  restoreWorkspace: boolean;
  setRestoreWorkspace: (value: boolean) => void;
  timestamps: boolean;
  setTimestamps: (value: boolean) => void;
  verboseReasoning: boolean;
  setVerboseReasoning: (value: boolean) => void;
}) {
  const [cleanupOpen, setCleanupOpen] = useState(false);
  const [confirmation, setConfirmation] = useState("");
  const [cleanupStatus, setCleanupStatus] =
    useState<ApplicationCleanupStatus | null>(null);
  const [cleanupError, setCleanupError] = useState<string | null>(null);
  const cleaning =
    cleanupStatus !== null &&
    cleanupStatus.status !== "ready" &&
    cleanupStatus.status !== "failed";
  const cleanupComplete = cleanupStatus?.status === "ready";

  const beginCleanup = async () => {
    if (confirmation !== "DELETE AMARCODE DATA" || cleaning) return;
    setCleanupError(null);
    setCleanupStatus({ status: "preparing" });
    try {
      await daemonApi.prepareApplicationUninstall(true, setCleanupStatus);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setCleanupError(message);
      setCleanupStatus({ status: "failed", error: message });
      return;
    }

    try {
      localStorage.clear();
      sessionStorage.clear();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setCleanupError(
        `The service and native data were removed, but UI preferences could not be cleared: ${message}`,
      );
      setCleanupStatus({ status: "ready" });
      return;
    }

    setCleanupStatus({ status: "ready" });
    try {
      await daemonApi.exitApplication();
    } catch {
      setCleanupError(
        "Cleanup is complete, but Amarcode could not exit automatically. Exit the application manually.",
      );
    }
  };

  const cleanupLabel =
    cleanupStatus?.status === "removingServiceAndData"
      ? "Removing service and local data…"
      : cleanupStatus?.status === "removingReleaseCache"
        ? "Removing downloaded daemon files…"
        : cleanupStatus?.status === "ready"
          ? "Cleanup complete"
          : "Preparing cleanup…";

  return (
    <div className="mx-auto w-full max-w-132">
      <h2 className="text-base font-medium">General</h2>
      <p className="mt-1 text-xs leading-5 text-muted-foreground">
        Set the defaults for starting and reviewing agent sessions.
      </p>
      <Separator className="my-5" />
      <div className="divide-y divide-border rounded-xl border border-border">
        <PreferenceRow
          title="Restore recent workspace"
          description="Preselect the most recently used project when creating a session."
          checked={restoreWorkspace}
          onCheckedChange={setRestoreWorkspace}
        />
        <PreferenceRow
          title="Show activity timestamps"
          description="Display local timestamps beside streamed agent activity."
          checked={timestamps}
          onCheckedChange={setTimestamps}
        />
        <PreferenceRow
          title="Verbose reasoning"
          description="Show full tool commands and untruncated thought steps in the chain of thought."
          checked={verboseReasoning}
          onCheckedChange={setVerboseReasoning}
        />
      </div>
      <div className="mt-4 flex items-center justify-between">
        <p className="text-[11px] text-muted-foreground">
          Reset these workspace preferences.
        </p>
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            setRestoreWorkspace(true);
            setTimestamps(false);
            setVerboseReasoning(false);
          }}
        >
          <RotateCcw data-icon="inline-start" />
          Restore defaults
        </Button>
      </div>
      <Separator className="my-5" />
      <section aria-labelledby="danger-zone-title">
        <div className="rounded-xl border border-destructive/35 bg-destructive/3 p-3">
          <div className="flex items-center gap-3">
            <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-destructive/10 text-destructive">
              <Trash2 className="size-4" />
            </span>
            <div className="min-w-0 flex-1">
              <h3
                id="danger-zone-title"
                className="text-sm font-medium text-destructive"
              >
                Remove Amarcode data
              </h3>
              <p className="mt-1 text-[11px] leading-4 text-muted-foreground">
                Stop and remove the background service, then permanently delete
                local chats, logs, settings, and downloaded daemon files.
              </p>
            </div>
            <Button
              className="shrink-0"
              variant="destructive"
              size="sm"
              onClick={() => {
                setConfirmation("");
                setCleanupError(null);
                setCleanupStatus(null);
                setCleanupOpen(true);
              }}
            >
              Remove data
            </Button>
          </div>
        </div>
      </section>

      <AlertDialog
        open={cleanupOpen}
        onOpenChange={(nextOpen) => {
          if (!cleaning) setCleanupOpen(nextOpen);
        }}
      >
        <AlertDialogContent
          onEscapeKeyDown={(event) => {
            if (cleaning) event.preventDefault();
          }}
        >
          <AlertDialogHeader>
            <AlertDialogMedia className="bg-destructive/10 text-destructive">
              <TriangleAlert />
            </AlertDialogMedia>
            <AlertDialogTitle>Delete all local Amarcode data?</AlertDialogTitle>
            <AlertDialogDescription>
              This permanently removes local chats, daemon logs, settings, the
              background service, and downloaded daemon versions. Project files
              in your workspaces are not deleted.
            </AlertDialogDescription>
          </AlertDialogHeader>

          {cleaning || cleanupStatus?.status === "ready" ? (
            <div
              className="flex items-center gap-2 rounded-lg border border-border bg-muted/40 px-3 py-2.5 text-xs"
              aria-live="polite"
            >
              {cleaning ? (
                <LoaderCircle className="size-4 animate-spin text-muted-foreground" />
              ) : (
                <Check className="size-4 text-emerald-600" />
              )}
              <span>{cleanupLabel}</span>
            </div>
          ) : (
            <div className="space-y-2">
              <label
                htmlFor="cleanup-confirmation"
                className="text-xs font-medium"
              >
                Type <span className="font-mono">DELETE AMARCODE DATA</span> to
                continue
              </label>
              <Input
                id="cleanup-confirmation"
                autoComplete="off"
                spellCheck={false}
                value={confirmation}
                onChange={(event) => setConfirmation(event.target.value)}
                aria-invalid={Boolean(cleanupError)}
              />
            </div>
          )}

          {cleanupError && (
            <Alert variant="destructive" aria-live="assertive">
              <TriangleAlert />
              <AlertDescription>{cleanupError}</AlertDescription>
            </Alert>
          )}

          <AlertDialogFooter>
            <Button
              variant="outline"
              disabled={cleaning}
              onClick={() => {
                if (cleanupComplete) {
                  void daemonApi.exitApplication();
                } else {
                  setCleanupOpen(false);
                }
              }}
            >
              {cleanupComplete ? "Exit Amarcode" : "Cancel"}
            </Button>
            <Button
              variant="destructive"
              disabled={
                confirmation !== "DELETE AMARCODE DATA" ||
                cleaning ||
                cleanupComplete
              }
              onClick={() => void beginCleanup()}
            >
              {cleaning ? "Removing…" : "Permanently remove data"}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function ThemePreview({
  theme,
  palette,
}: {
  theme: Theme;
  palette: AppPalette;
}) {
  const lightCanvas =
    palette === "monochrome" ? "bg-[#fafafa]" : "bg-[#f8f3ea]";
  const darkCanvas = palette === "monochrome" ? "bg-[#383838]" : "bg-[#3d342e]";
  const canvas =
    theme === "light"
      ? lightCanvas
      : theme === "dark"
        ? darkCanvas
        : "bg-transparent";
  return (
    <div
      className={`h-20 overflow-hidden rounded-lg border border-black/10 p-2 ${canvas}`}
    >
      {theme === "system" ? (
        <div className="grid h-full grid-cols-2 overflow-hidden rounded-md">
          <div className={lightCanvas} />
          <div className={darkCanvas} />
        </div>
      ) : (
        <div className="grid h-full grid-cols-[1.5rem_1fr] gap-1.5">
          <div
            className={
              theme === "light"
                ? palette === "monochrome"
                  ? "rounded-sm bg-[#e8e8e8]"
                  : "rounded-sm bg-[#e9dcc8]"
                : "rounded-sm bg-[#58493f]"
            }
          />
          <div className="grid gap-1.5">
            <div
              className={
                theme === "light"
                  ? palette === "monochrome"
                    ? "h-3 rounded-sm bg-[#d6d6d6]"
                    : "h-3 rounded-sm bg-[#e0c9aa]"
                  : "h-3 rounded-sm bg-[#5a4a40]"
              }
            />
            <div
              className={
                theme === "light"
                  ? "rounded-sm bg-white/90"
                  : "rounded-sm bg-[#493d35]"
              }
            />
          </div>
        </div>
      )}
    </div>
  );
}

function PalettePreview({ palette }: { palette: AppPalette }) {
  const colors =
    palette === "monochrome"
      ? ["bg-[#fafafa]", "bg-[#e8e8e8]", "bg-[#303030]"]
      : ["bg-[#f8f3ea]", "bg-[#e0c9aa]", "bg-[#d7862f]"];
  return (
    <div className="flex h-10 overflow-hidden rounded-lg border border-black/10">
      {colors.map((color) => (
        <div className={`flex-1 ${color}`} key={color} />
      ))}
    </div>
  );
}

function PreferenceRow({
  title,
  description,
  checked,
  onCheckedChange,
}: {
  title: string;
  description: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <div className="flex items-center gap-4 px-4 py-3">
      <div className="min-w-0 flex-1">
        <p className="text-xs font-medium">{title}</p>
        <p className="mt-1 text-[11px] leading-4 text-muted-foreground">
          {description}
        </p>
      </div>
      <Switch checked={checked} onCheckedChange={onCheckedChange} />
    </div>
  );
}
