import { useEffect, useState } from "react";
import {
  APP_SHORTCUTS,
  COMMANDS_CHANGED_EVENT,
  canRunAppCommand,
  runAppCommand,
  shortcutHint,
  type AppCommand,
} from "@/lib/app-shortcuts";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from "./ui/command";

import {
  MessageSquarePlus,
  TextCursorInput,
  Square,
  Settings2,
  PanelLeft,
  PanelRight,
  FileSearch,
  Search,
  GitBranch,
  Save,
  RefreshCw,
  Pin,
  Maximize2,
  X,
  type LucideIcon,
} from "lucide-react";

const commandIcons: Record<
  Exclude<AppCommand, "command-palette">,
  LucideIcon
> = {
  "new-chat": MessageSquarePlus,
  "focus-prompt": TextCursorInput,
  "stop-response": Square,
  settings: Settings2,
  sidebar: PanelLeft,
  workspace: PanelRight,
  "find-files": FileSearch,
  "search-content": Search,
  "git-changes": GitBranch,
  "save-file": Save,
  refresh: RefreshCw,
  "pin-workspace": Pin,
  "maximize-workspace": Maximize2,
  "close-workspace": X,
};

export function AppCommandPalette({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [available, setAvailable] = useState<Set<AppCommand>>(new Set());
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    let scheduled = false;
    // Wait for command owners to register their latest enabled states.
    const updateAvailability = () => {
      if (scheduled) return;
      scheduled = true;
      queueMicrotask(() => {
        scheduled = false;
        if (!cancelled)
          setAvailable(
            new Set(
              APP_SHORTCUTS.filter((item) => canRunAppCommand(item.id)).map(
                (item) => item.id,
              ),
            ),
          );
      });
    };
    updateAvailability();
    window.addEventListener(COMMANDS_CHANGED_EVENT, updateAvailability);
    return () => {
      cancelled = true;
      window.removeEventListener(COMMANDS_CHANGED_EVENT, updateAvailability);
    };
  }, [open]);

  const selectCommand = (id: AppCommand) => {
    onOpenChange(false);
    // Let the dialog release focus before a command opens another surface.
    requestAnimationFrame(() => runAppCommand(id));
  };

  return (
    <CommandDialog
      open={open}
      onOpenChange={onOpenChange}
      variant="palette"
      compact
      title="Command palette"
      description="Find an action, or learn its shortcut."
    >
      <Command key={String(open)} data-app-command-palette loop>
        <CommandInput
          placeholder="Search commands…"
          aria-label="Search commands"
        />
        <CommandList>
          <CommandEmpty>No matching commands. Try another search.</CommandEmpty>
          {APP_SHORTCUTS.filter((item) => item.id !== "command-palette").map(
            (item) => {
              const Icon = commandIcons[item.id];
              return (
                <CommandItem
                  key={item.id}
                  value={item.label}
                  keywords={[item.id, shortcutHint(item.id)]}
                  disabled={!available.has(item.id)}
                  onSelect={() => selectCommand(item.id)}
                >
                  <Icon className="size-4 text-muted-foreground" />
                  <span className="flex-1">{item.label}</span>
                  <CommandShortcut>
                    {shortcutHint(item.id)
                      .split("+")
                      .map((key) => (
                        <kbd key={key}>{key}</kbd>
                      ))}
                  </CommandShortcut>
                </CommandItem>
              );
            },
          )}
        </CommandList>
      </Command>
    </CommandDialog>
  );
}
