import { useEffect } from "react";
import { useAtomValue, useSetAtom, useStore } from "jotai";
import { toast } from "sonner";
import { daemonApi } from "@/api";
import { loadAgentsAtom, selectedAgentAtom, selectedAgentIdAtom } from "./agents";
import { refreshChatsAtom } from "./chats";
import { ensureDaemonEventStream, subscribeDaemonEvents } from "./daemon-events";
import { defaultAgentIdAtom, defaultSessionModeAtom, paletteAtom, themeAtom } from "./preferences";
import { composerSessionModeAtom } from "./navigation";

/**
 * One-time app shell effects: theme DOM sync, catalogs, event stream, defaults.
 * Mount once near the root of `App`.
 */
export function useAppBootstrap() {
  const store = useStore();
  const theme = useAtomValue(themeAtom);
  const palette = useAtomValue(paletteAtom);
  const defaultAgentId = useAtomValue(defaultAgentIdAtom);
  const defaultSessionMode = useAtomValue(defaultSessionModeAtom);
  const selectedAgent = useAtomValue(selectedAgentAtom);
  const selectedAgentId = useAtomValue(selectedAgentIdAtom);

  const loadAgents = useSetAtom(loadAgentsAtom);
  const refreshChats = useSetAtom(refreshChatsAtom);
  const setSelectedAgentId = useSetAtom(selectedAgentIdAtom);
  const setComposerMode = useSetAtom(composerSessionModeAtom);

  // Theme → <html class="dark">
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () =>
      document.documentElement.classList.toggle(
        "dark",
        theme === "dark" || (theme === "system" && media.matches),
      );
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);

  // Palette → data-style
  useEffect(() => {
    document.documentElement.dataset.style = palette;
  }, [palette]);

  // Catalogs + event stream bound to the Provider store (not getDefaultStore).
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        // `daemon_health` validates the shared wire protocol version in the
        // Tauri layer. Do not start subscriptions or catalogs against an
        // incompatible daemon.
        await daemonApi.health();
        if (cancelled) return;
        ensureDaemonEventStream(store);
        await Promise.all([loadAgents(), refreshChats()]);
      } catch (error) {
        if (cancelled) return;
        console.error("Daemon bootstrap failed:", error);
        toast.error(
          error instanceof Error
            ? error.message
            : typeof error === "string"
              ? error
              : "Unable to connect to a compatible daemon.",
        );
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [store, loadAgents, refreshChats]);

  // Keep sidebar list fresh when any chat metadata changes (every event).
  useEffect(() => {
    return subscribeDaemonEvents((event) => {
      if (event.type === "chatUpdated") void refreshChats();
    });
  }, [refreshChats]);

  // Seed selection from defaults once agents are known.
  useEffect(() => {
    if (!selectedAgentId && selectedAgent) {
      setSelectedAgentId(selectedAgent.id);
    }
  }, [selectedAgent, selectedAgentId, setSelectedAgentId]);

  useEffect(() => {
    if (!selectedAgentId) setSelectedAgentId(defaultAgentId);
  }, [defaultAgentId, selectedAgentId, setSelectedAgentId]);

  // Home composer mode tracks the settings default until the user overrides it.
  useEffect(() => {
    setComposerMode(defaultSessionMode);
  }, [defaultSessionMode, setComposerMode]);
}
