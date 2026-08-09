import { useEffect } from "react";
import { useAtomValue, useSetAtom } from "jotai";
import { toast } from "sonner";
import { loadAgentsAtom, selectedAgentAtom, selectedAgentIdAtom } from "./agents";
import { refreshChatsAtom } from "./chats";
import { ensureDaemonEventStream, lastDaemonEventAtom } from "./daemon-events";
import { defaultAgentIdAtom, defaultSessionModeAtom, paletteAtom, themeAtom } from "./preferences";
import { composerSessionModeAtom } from "./navigation";

/**
 * One-time app shell effects: theme DOM sync, catalogs, event stream, defaults.
 * Mount once near the root of `App`.
 */
export function useAppBootstrap() {
  const theme = useAtomValue(themeAtom);
  const palette = useAtomValue(paletteAtom);
  const defaultAgentId = useAtomValue(defaultAgentIdAtom);
  const defaultSessionMode = useAtomValue(defaultSessionModeAtom);
  const selectedAgent = useAtomValue(selectedAgentAtom);
  const selectedAgentId = useAtomValue(selectedAgentIdAtom);
  const lastEvent = useAtomValue(lastDaemonEventAtom);

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

  // Catalogs + event stream
  useEffect(() => {
    ensureDaemonEventStream();
    void loadAgents().catch((error: unknown) => {
      console.error("Failed to load agents:", error);
    });
    void refreshChats().catch((error: unknown) => {
      console.error("Failed to load chats:", error);
      toast.error("Failed to load chats. Please try again.");
    });
  }, [loadAgents, refreshChats]);

  // Keep sidebar list fresh when any chat metadata changes.
  useEffect(() => {
    if (lastEvent?.type === "chatUpdated") void refreshChats();
  }, [lastEvent, refreshChats]);

  // Seed selection from defaults once agents are known.
  useEffect(() => {
    if (!selectedAgentId && selectedAgent) {
      setSelectedAgentId(selectedAgent.id);
    }
  }, [selectedAgent, selectedAgentId, setSelectedAgentId]);

  // Prefer default agent when the setting changes and nothing is selected yet.
  useEffect(() => {
    if (!selectedAgentId) setSelectedAgentId(defaultAgentId);
  }, [defaultAgentId, selectedAgentId, setSelectedAgentId]);

  // Home composer mode tracks the settings default until the user overrides it.
  useEffect(() => {
    setComposerMode(defaultSessionMode);
  }, [defaultSessionMode, setComposerMode]);
}
