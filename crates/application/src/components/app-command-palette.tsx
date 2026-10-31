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
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from "./ui/command";

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
      title="Command palette"
      description="Search actions and keyboard shortcuts. Use arrow keys to choose and Enter to run."
      className="sm:max-w-lg"
    >
      <Command key={String(open)} data-app-command-palette loop>
        <CommandInput
          placeholder="Search commands…"
          aria-label="Search commands"
        />
        <CommandList>
          <CommandEmpty>No matching commands.</CommandEmpty>
          {["Chat", "Workspace", "App"].map((group) => (
            <CommandGroup key={group} heading={group}>
              {APP_SHORTCUTS.filter(
                (item) => item.group === group && item.id !== "command-palette",
              ).map((item) => (
                <CommandItem
                  key={item.id}
                  value={item.label}
                  keywords={[item.id, shortcutHint(item.id)]}
                  disabled={!available.has(item.id)}
                  onSelect={() => selectCommand(item.id)}
                >
                  <span>{item.label}</span>
                  <CommandShortcut>{shortcutHint(item.id)}</CommandShortcut>
                </CommandItem>
              ))}
            </CommandGroup>
          ))}
        </CommandList>
        <div className="flex items-center justify-between border-t px-3 py-2 text-[10px] text-muted-foreground">
          <span>↑↓ Navigate · Enter Run · Esc Close</span>
          <kbd>{shortcutHint("command-palette")}</kbd>
        </div>
      </Command>
    </CommandDialog>
  );
}
