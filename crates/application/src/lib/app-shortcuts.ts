export const APP_SHORTCUTS = [
  { id: "new-chat", label: "New chat", key: "n", group: "Chat" },
  {
    id: "focus-prompt",
    label: "Focus prompt / return to chat",
    key: "l",
    shift: true,
    group: "Chat",
  },
  {
    id: "stop-response",
    label: "Stop active response",
    key: ".",
    group: "Chat",
  },
  { id: "settings", label: "Open settings", key: ",", group: "App" },
  { id: "sidebar", label: "Toggle chat sidebar", key: "b", group: "App" },
  {
    id: "command-palette",
    label: "Command palette",
    key: "k",
    group: "App",
  },
  {
    id: "workspace",
    label: "Toggle workspace panel",
    key: "e",
    shift: true,
    group: "Workspace",
  },
  {
    id: "find-files",
    label: "Find workspace files",
    key: "p",
    group: "Workspace",
  },
  {
    id: "search-content",
    label: "Search workspace contents",
    key: "f",
    shift: true,
    group: "Workspace",
  },
  {
    id: "git-changes",
    label: "Show Git changes",
    key: "g",
    shift: true,
    group: "Workspace",
  },
  { id: "save-file", label: "Save open file", key: "s", group: "Workspace" },
  {
    id: "refresh",
    label: "Refresh workspace, search, and file",
    key: "r",
    shift: true,
    group: "Workspace",
  },
  {
    id: "pin-workspace",
    label: "Pin / unpin workspace",
    key: "d",
    shift: true,
    group: "Workspace",
  },
  {
    id: "maximize-workspace",
    label: "Maximize / restore workspace",
    key: "m",
    shift: true,
    group: "Workspace",
  },
  {
    id: "close-workspace",
    label: "Close workspace panel",
    key: "w",
    group: "Workspace",
  },
] as const;

export type AppCommand = (typeof APP_SHORTCUTS)[number]["id"];
type Shortcut = { key: string; mod?: boolean; shift?: boolean };
type KeyEvent = Pick<
  KeyboardEvent,
  | "key"
  | "ctrlKey"
  | "metaKey"
  | "altKey"
  | "shiftKey"
  | "isComposing"
  | "repeat"
  | "defaultPrevented"
>;

export function matchesShortcut(event: KeyEvent, shortcut: Shortcut) {
  if (
    event.defaultPrevented ||
    event.isComposing ||
    event.repeat ||
    event.altKey
  )
    return false;
  const modifier = shortcut.mod !== false;
  return (
    event.key.toLowerCase() === shortcut.key.toLowerCase() &&
    Boolean(event.ctrlKey || event.metaKey) === modifier &&
    !(event.ctrlKey && event.metaKey) &&
    event.shiftKey === Boolean(shortcut.shift)
  );
}

export function shortcutHint(id: AppCommand) {
  const shortcut: Shortcut = APP_SHORTCUTS.find((item) => item.id === id)!;
  const mac =
    typeof navigator !== "undefined" &&
    /Mac|iPhone|iPad/.test(navigator.platform);
  return [
    shortcut.mod === false ? "" : mac ? "Cmd" : "Ctrl",
    shortcut.shift ? "Shift" : "",
    shortcut.key.toUpperCase(),
  ]
    .filter(Boolean)
    .join("+");
}

/** Modal UI owns its keyboard interaction; the nonmodal workspace sheet does not. */
export function shortcutOverlayOpen() {
  return [
    ...document.querySelectorAll<HTMLElement>(
      '[role="dialog"]:not([data-workspace-panel]), [role="alertdialog"], [role="menu"], [role="listbox"]',
    ),
  ].some(
    (element) =>
      element.getAttribute("data-state") !== "closed" &&
      element.getClientRects().length > 0,
  );
}

export const COMMAND_EVENT = "amarcode:command";
export const COMMANDS_CHANGED_EVENT = "amarcode:commands-changed";
export type CommandRequest = {
  id: AppCommand;
  handled: boolean;
  check?: boolean;
};

export function canRunAppCommand(id: AppCommand) {
  const detail: CommandRequest = { id, handled: false, check: true };
  window.dispatchEvent(
    new CustomEvent<CommandRequest>(COMMAND_EVENT, { detail }),
  );
  return detail.handled;
}

export function runAppCommand(id: AppCommand) {
  const detail: CommandRequest = { id, handled: false };
  window.dispatchEvent(
    new CustomEvent<CommandRequest>(COMMAND_EVENT, { detail }),
  );
  return detail.handled;
}

export function routeAppShortcut(
  event: KeyEvent & Pick<KeyboardEvent, "preventDefault" | "stopPropagation">,
  overlayOpen = shortcutOverlayOpen,
  dispatch = runAppCommand,
  paletteOpen = () => {
    const palette = document.querySelector("[data-app-command-palette]");
    return Boolean(
      palette?.getClientRects().length &&
      palette.closest('[role="dialog"]')?.getAttribute("data-state") === "open",
    );
  },
) {
  const shortcut = APP_SHORTCUTS.find((item) => matchesShortcut(event, item));
  if (!shortcut) return;
  // Reserved app chords must not print, save the page, or reload the webview.
  event.preventDefault();
  if (overlayOpen() && !(shortcut.id === "command-palette" && paletteOpen()))
    return;
  if (dispatch(shortcut.id)) event.stopPropagation();
}
