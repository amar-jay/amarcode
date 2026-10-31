import { useEffect } from "react";
import {
  COMMAND_EVENT,
  COMMANDS_CHANGED_EVENT,
  routeAppShortcut,
  type AppCommand,
  type CommandRequest,
} from "@/lib/app-shortcuts";

export function useAppCommands(
  handlers: Partial<
    Record<
      AppCommand,
      (() => void | boolean) | { run: () => void | boolean; enabled: boolean }
    >
  >,
) {
  useEffect(() => {
    const listener = (event: Event) => {
      const request = (event as CustomEvent<CommandRequest>).detail;
      if (request.handled) return;
      const handler = handlers[request.id];
      if (!handler) return;
      const enabled = typeof handler === "function" || handler.enabled;
      if (!enabled) return;
      if (request.check) {
        request.handled = true;
        return;
      }
      request.handled =
        (typeof handler === "function" ? handler() : handler.run()) !== false;
    };
    window.addEventListener(COMMAND_EVENT, listener);
    window.dispatchEvent(new Event(COMMANDS_CHANGED_EVENT));
    return () => {
      window.removeEventListener(COMMAND_EVENT, listener);
      window.dispatchEvent(new Event(COMMANDS_CHANGED_EVENT));
    };
  }, [handlers]);
}

export function useAppShortcutRouter() {
  useEffect(() => {
    const listener = (event: KeyboardEvent) => routeAppShortcut(event);
    window.addEventListener("keydown", listener, true);
    return () => window.removeEventListener("keydown", listener, true);
  }, []);
}
