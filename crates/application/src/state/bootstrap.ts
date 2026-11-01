import { useWorkspaceLaunch } from "@/hooks/use-workspace-launch";
import { useCallback, useEffect, useRef, useState } from "react";
import { useDaemonConnectionStatus } from "@/hooks/use-daemon-connection-status";
import { useAtomValue, useSetAtom, useStore } from "jotai";
import { notify } from "@/lib/notify";
import { useUpdates } from "@/hooks/use-updates";
import { daemonApi, type DaemonBootstrapStatus } from "@/api";
import {
  loadAgentsAtom,
  selectedAgentAtom,
  selectedAgentIdAtom,
} from "./agents";
import { refreshChatsAtom } from "./chats";
import {
  daemonConnectionGenerationAtom,
  ensureDaemonEventStream,
  subscribeDaemonEvents,
} from "./daemon-events";
import { defaultAgentIdAtom, paletteAtom, themeAtom } from "./preferences";
import { liveChatAtom, loadLiveChatAtom } from "./live-chat";
import {
  effectiveDefaultWorkspacePathAtom,
  workspacePathAtom,
} from "./workspace";
import { applyDaemonWorkspaceAtom } from "./navigation";
import type { Health } from "@/types";

/**
 * One-time app shell effects: theme DOM sync, catalogs, event stream, defaults.
 * Mount once near the root of `App`.
 */
export function useAppBootstrap() {
  const [daemonConnection, setDaemonConnection] =
    useState<DaemonBootstrapStatus>({
      status: "checking",
    });
  const [clientReady, setClientReady] = useState(false);
  useWorkspaceLaunch(clientReady && daemonConnection.status === "ready");
  const [daemonAttempt, setDaemonAttempt] = useState(0);
  const sshStatus = useDaemonConnectionStatus();
  const recoveredGeneration = useRef<number | null>(null);
  const store = useStore();
  const theme = useAtomValue(themeAtom);
  const palette = useAtomValue(paletteAtom);
  const defaultAgentId = useAtomValue(defaultAgentIdAtom);
  const defaultWorkspacePath = useAtomValue(effectiveDefaultWorkspacePathAtom);
  const selectedAgent = useAtomValue(selectedAgentAtom);
  const selectedAgentId = useAtomValue(selectedAgentIdAtom);

  const loadAgents = useSetAtom(loadAgentsAtom);
  const refreshChats = useSetAtom(refreshChatsAtom);
  const loadLiveChat = useSetAtom(loadLiveChatAtom);
  const setSelectedAgentId = useSetAtom(selectedAgentIdAtom);
  const setWorkspacePath = useSetAtom(workspacePathAtom);
  const retryDaemon = useCallback(
    () => setDaemonAttempt((attempt) => attempt + 1),
    [],
  );
  useEffect(() => {
    if (
      sshStatus?.status === "connected" &&
      daemonConnection.status === "failed" &&
      recoveredGeneration.current !== sshStatus.generation
    ) {
      recoveredGeneration.current = sshStatus.generation;
      retryDaemon();
    }
  }, [sshStatus, daemonConnection.status, retryDaemon]);
  const initializeDaemonClient = useCallback(
    async (knownHealth?: Health) => {
      ensureDaemonEventStream(store);
      const generation = store.get(daemonConnectionGenerationAtom);
      const [, , health] = await Promise.all([
        loadAgents(),
        refreshChats(),
        knownHealth ? Promise.resolve(knownHealth) : daemonApi.health(),
      ]);
      if (store.get(daemonConnectionGenerationAtom) === generation) {
        store.set(
          applyDaemonWorkspaceAtom,
          health.default_workspace_path ?? "",
        );
      }
    },
    [store, loadAgents, refreshChats],
  );
  const reconcileDaemonClient = useCallback(async () => {
    await initializeDaemonClient();
    const live = store.get(liveChatAtom);
    if (live) await loadLiveChat({ chatId: live.chatId, silent: true });
  }, [initializeDaemonClient, loadLiveChat, store]);

  const updates = useUpdates(daemonConnection.status, reconcileDaemonClient);

  const installDaemon = useCallback(async () => {
    try {
      const health = await daemonApi.install(setDaemonConnection);
      await initializeDaemonClient(health);
      setClientReady(true);
    } catch (error) {
      console.error("Daemon installation failed:", error);
      const message =
        error instanceof Error
          ? error.message
          : typeof error === "string"
            ? error
            : "Unable to install the Amarcode background service.";
      setDaemonConnection({ status: "failed", error: message });
      notify(message, "error");
    }
  }, [initializeDaemonClient]);

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
        // Bootstrap may connect or start an already-installed service. It is
        // deliberately read-only when service installation has not occurred.
        setClientReady(false);
        setDaemonConnection({ status: "checking" });
        const health = await daemonApi.bootstrap((status) => {
          if (!cancelled) setDaemonConnection(status);
        });
        if (cancelled || health === null) return;
        await initializeDaemonClient(health);
        if (!cancelled) setClientReady(true);
      } catch (error) {
        if (cancelled) return;
        console.error("Daemon bootstrap failed:", error);
        const message =
          error instanceof Error
            ? error.message
            : typeof error === "string"
              ? error
              : "Unable to install or start a compatible daemon.";
        setDaemonConnection({ status: "failed", error: message });
        notify(message, "error");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [initializeDaemonClient, daemonAttempt]);

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

  // Seed the home composer without replacing a workspace already selected in
  // this app session or one loaded from an existing chat.
  useEffect(() => {
    if (!store.get(workspacePathAtom) && defaultWorkspacePath) {
      setWorkspacePath(defaultWorkspacePath);
    }
  }, [defaultWorkspacePath, setWorkspacePath, store]);

  return {
    daemonConnection,
    retryDaemon,
    installDaemon,
    ...updates,
  };
}
