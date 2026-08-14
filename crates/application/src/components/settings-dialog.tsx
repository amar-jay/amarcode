import { useEffect, useState } from "react";
import { useAtom } from "jotai";
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
  Trash2,
  TriangleAlert,
} from "lucide-react";
import { daemonApi, type ApplicationCleanupStatus } from "@/api";
import type { Palette as AppPalette, Theme } from "@/state";
import { verboseReasoningAtom } from "@/state";
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
  Field,
  FieldContent,
  FieldDescription,
  FieldLabel,
  FieldLegend,
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
import type { SessionMode } from "@/state";

type SettingsPage = "appearance" | "general" | "agent";

const navigation: { id: SettingsPage; label: string; icon: typeof Palette }[] =
  [
    { id: "appearance", label: "Appearance", icon: Palette },
    { id: "agent", label: "Agent defaults", icon: Bot },
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
  defaultSessionMode,
  onDefaultSessionModeChange,
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
  defaultSessionMode: SessionMode;
  onDefaultSessionModeChange: (mode: SessionMode) => void;
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

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="h-125! w-200! max-w-200! overflow-hidden p-0">
        <DialogTitle className="sr-only">Settings</DialogTitle>
        <DialogDescription className="sr-only">
          Customize your amarcode preferences.
        </DialogDescription>
        <SidebarProvider
          className="h-full min-h-0 items-start"
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
            <div className="flex flex-1 flex-col overflow-y-auto p-6">
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
                  defaultSessionMode={defaultSessionMode}
                  onDefaultSessionModeChange={onDefaultSessionModeChange}
                />
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

function AgentDefaultsPanel({
  agents,
  defaultAgentId,
  onDefaultAgentChange,
  defaultSessionMode,
  onDefaultSessionModeChange,
}: {
  agents: AgentInfo[];
  defaultAgentId: string;
  onDefaultAgentChange: (agentId: string) => void;
  defaultSessionMode: SessionMode;
  onDefaultSessionModeChange: (mode: SessionMode) => void;
}) {
  const [agentPickerOpen, setAgentPickerOpen] = useState(false);
  const selectedAgent = agents.find((agent) => agent.id === defaultAgentId);
  const availableAgents = agents.filter((agent) => agent.available);
  const unavailableAgents = agents.filter((agent) => !agent.available);
  const renderAgent = (agent: AgentInfo) => (
    <CommandItem
      className="cursor-pointer rounded-none w-full! space-x-auto data-disabled:cursor-not-allowed data-disabled:opacity-50"
      disabled={!agent.available}
      title={agent.unavailable_reason ?? undefined}
      key={agent.id}
      value={`${agent.name} ${agent.id}`}
      onSelect={() => {
        onDefaultAgentChange(agent.id);
        setAgentPickerOpen(false);
      }}
    >
      <span className="mr-auto">{agent.name.replace(/\s*\bACP\s*$/i, "")}</span>
      {!agent.available ? (
        <span className="ml-auto text-xs text-muted-foreground">
          Not installed
        </span>
      ) : (
        <Check
          className={`ml-auto size-4 ${agent.id === defaultAgentId ? "opacity-100" : "opacity-0"}`}
        />
      )}
    </CommandItem>
  );
  return (
    <div className="mx-auto w-full max-w-132">
      <h2 className="text-base font-medium">Agent defaults</h2>
      <p className="mt-1 text-xs leading-5 text-muted-foreground">
        These choices are used when you start a new chat. Existing conversations
        keep their current session settings.
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
                className="w-full justify-between font-normal"
              >
                {selectedAgent?.name.replace(/\s*\bACP\s*$/i, "") ??
                  "Choose an agent"}
                <ChevronsUpDown className="size-4 text-muted-foreground" />
              </Button>
            </PopoverTrigger>
            <PopoverContent
              align="start"
              className="w-(--radix-popover-trigger-width) p-0"
            >
              <Command>
                <CommandInput placeholder="Search ACP agents…" />
                <CommandList>
                  <CommandEmpty>No matching ACP agents.</CommandEmpty>
                  {availableAgents.length > 0 && (
                    <CommandGroup heading="Available">
                      {availableAgents.map(renderAgent)}
                    </CommandGroup>
                  )}
                  {unavailableAgents.length > 0 && (
                    <CommandGroup heading="Not installed">
                      {unavailableAgents.map(renderAgent)}
                    </CommandGroup>
                  )}
                </CommandList>
              </Command>
            </PopoverContent>
          </Popover>
        </Field>
        <FieldSet>
          <FieldLegend>Default session mode</FieldLegend>
          <FieldDescription>
            Controls how Codex starts new sessions.
          </FieldDescription>
          <RadioGroup
            value={defaultSessionMode}
            onValueChange={(value) =>
              onDefaultSessionModeChange(value as SessionMode)
            }
          >
            {(["plan", "build", "ask"] as const).map((mode) => (
              <Field key={mode} orientation="horizontal">
                <RadioGroupItem value={mode} id={`default-mode-${mode}`} />
                <FieldContent>
                  <FieldLabel
                    htmlFor={`default-mode-${mode}`}
                    className="capitalize"
                  >
                    {mode}
                  </FieldLabel>
                  <FieldDescription>
                    {mode === "plan"
                      ? "Plan before implementation"
                      : mode === "build"
                        ? "Work with agent access"
                        : "Review without edits"}
                  </FieldDescription>
                </FieldContent>
              </Field>
            ))}
          </RadioGroup>
        </FieldSet>
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
        className="mt-5 grid grid-cols-3 gap-3"
      >
        {themeChoices.map(({ value, label, description, Icon }) => (
          <label
            htmlFor={`theme-${value}`}
            key={value}
            data-active={theme === value || undefined}
            className="group relative cursor-pointer rounded-xl border border-border p-3 transition-all hover:-translate-y-0.5 hover:shadow-sm data-active:border-primary data-active:ring-2 data-active:ring-primary/20"
          >
            <ThemePreview theme={value} palette={palette} />
            <div className="mt-3 flex items-start gap-2">
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
      <div className="mt-8">
        <p className="text-sm font-medium">Palette</p>
        <p className="mt-1 text-xs leading-5 text-muted-foreground">
          Choose the accent and surface treatment used across the app.
        </p>
      </div>
      <RadioGroup
        value={palette}
        onValueChange={(value) => onPaletteChange(value as AppPalette)}
        className="mt-5 grid grid-cols-2 gap-3"
      >
        {paletteChoices.map(({ value, label, description }) => (
          <label
            htmlFor={`palette-${value}`}
            key={value}
            data-active={palette === value || undefined}
            className="group relative cursor-pointer rounded-xl border border-border p-3 transition-all hover:-translate-y-0.5 hover:shadow-sm data-active:border-primary data-active:ring-2 data-active:ring-primary/20"
          >
            <PalettePreview palette={value} />
            <div className="mt-3 flex items-start gap-2">
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
      <p className="mt-6 rounded-lg border border-border bg-muted/40 px-3 py-2.5 text-[11px] leading-4 text-muted-foreground">
        System uses your device preference and updates automatically when it
        changes.
      </p>
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
      <Separator className="my-6" />
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
      <div className="mt-6 flex items-center justify-between">
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
      <Separator className="my-8" />
      <section aria-labelledby="danger-zone-title">
        <div className="rounded-xl border border-destructive/35 bg-destructive/3 p-4">
          <div className="flex items-start gap-3">
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
              <Button
                className="mt-4"
                variant="destructive"
                size="sm"
                onClick={() => {
                  setConfirmation("");
                  setCleanupError(null);
                  setCleanupStatus(null);
                  setCleanupOpen(true);
                }}
              >
                Remove service and data
              </Button>
            </div>
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
      className={`h-28 overflow-hidden rounded-lg border border-black/10 p-2 ${canvas}`}
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
    <div className="flex h-16 overflow-hidden rounded-lg border border-black/10">
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
    <div className="flex items-center gap-4 px-4 py-4">
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
