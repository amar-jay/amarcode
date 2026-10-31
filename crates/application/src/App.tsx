import { useAtom, useAtomValue, useSetAtom } from "jotai";
import { getCurrentWindow } from "@tauri-apps/api/window";
import MainPromptInput from "@/components/main-prompt-input";
import { AppSidebar } from "@/components/app-sidebar";
import { HomeWatermark } from "@/components/home-watermark";
import { LiveChatScreen } from "@/components/live-chat-screen";
import { TopBar } from "@/components/top-bar";
import { SettingsDialog } from "@/components/settings-dialog";
import { DaemonConnectionDialog } from "@/components/daemon-connection-dialog";
import { UpdateDialog } from "@/components/update-dialog";
import { Toaster } from "@/components/ui/sonner";
import { daemonApi } from "@/api";
import { useState } from "react";
import { useSidebar } from "@/components/ui/sidebar";
import {
  useAppCommands,
  useAppShortcutRouter,
} from "@/hooks/use-app-shortcuts";
import { AppCommandPalette } from "@/components/app-command-palette";
import {
  activeSessionAtom,
  agentsAtom,
  chatsAtom,
  defaultAgentIdAtom,
  defaultWorkspacePathAtom,
  latestTurnByChatAtom,
  liveChatIsWorkingAtom,
  openStartedChatAtom,
  paletteAtom,
  refreshChatsAtom,
  selectAgentByIdAtom,
  selectedAgentAtom,
  failStartedPromptAtom,
  selectChatAtom,
  setWorkspacePathAtom,
  settingsOpenAtom,
  sidePanelMaximizedAtom,
  sidePanelOpenAtom,
  sidePanelPinnedAtom,
  stopLiveChatAtom,
  startNewChatAtom,
  themeAtom,
  useAppBootstrap,
  workspacePathAtom,
} from "@/state";
import AppSidePanel from "./components/app-sidepanel";

/**
 * Shell only: route home vs live chat and host chrome.
 * Domain state lives in `src/state/*` (jotai).
 */
export default function App() {
  const {
    daemonConnection,
    retryDaemon,
    installDaemon,
    updatePlan,
    updateProgress,
    updateError,
    updateOpen,
    updateChecking,
    updating,
    updateAvailable,
    checkUpdates,
    installUpdates,
    closeUpdate,
  } = useAppBootstrap();

  const [theme, setTheme] = useAtom(themeAtom);
  const [palette, setPalette] = useAtom(paletteAtom);
  const [settingsOpen, setSettingsOpen] = useAtom(settingsOpenAtom);
  const workspacePath = useAtomValue(workspacePathAtom);
  const workspaceMaximized = useAtomValue(sidePanelMaximizedAtom);
  const setWorkspaceMaximized = useSetAtom(sidePanelMaximizedAtom);
  const setWorkspaceOpen = useSetAtom(sidePanelOpenAtom);
  const workspacePinned = useAtomValue(sidePanelPinnedAtom);
  const stopResponse = useSetAtom(stopLiveChatAtom);
  const { toggleSidebar } = useSidebar();
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  useAppShortcutRouter();
  const setWorkspacePath = useSetAtom(setWorkspacePathAtom);
  const agents = useAtomValue(agentsAtom);
  const selectedAgent = useAtomValue(selectedAgentAtom);
  const failStartedPrompt = useSetAtom(failStartedPromptAtom);
  const [defaultAgentId, setDefaultAgentId] = useAtom(defaultAgentIdAtom);
  const [defaultWorkspacePath, setDefaultWorkspacePath] = useAtom(
    defaultWorkspacePathAtom,
  );
  const chats = useAtomValue(chatsAtom);
  const activeSession = useAtomValue(activeSessionAtom);
  const latestTurns = useAtomValue(latestTurnByChatAtom);
  const activeChatIsWorking = useAtomValue(liveChatIsWorkingAtom);
  const runningChatIds = new Set(
    Object.entries(latestTurns)
      .filter(([, turn]) => turn.status === "started")
      .map(([chatId]) => chatId),
  );
  if (activeChatIsWorking && activeSession) {
    runningChatIds.add(activeSession.chat.id);
  }

  const toasterTheme =
    theme === "system"
      ? typeof window !== "undefined" &&
        window.matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : theme;

  const selectChat = useSetAtom(selectChatAtom);
  const startNewChat = useSetAtom(startNewChatAtom);
  const openStartedChat = useSetAtom(openStartedChatAtom);
  const selectAgentById = useSetAtom(selectAgentByIdAtom);
  const refreshChats = useSetAtom(refreshChatsAtom);

  const focusPrompt = () => {
    setWorkspaceMaximized(false);
    if (!workspacePinned) setWorkspaceOpen(false);
    requestAnimationFrame(() => {
      document
        .querySelector<HTMLTextAreaElement>("[data-chat-composer] textarea")
        ?.focus();
    });
  };
  useAppCommands({
    "new-chat": () => {
      startNewChat();
      focusPrompt();
    },
    "focus-prompt": focusPrompt,
    "stop-response": {
      enabled: activeChatIsWorking,
      run: () => {
        void stopResponse();
      },
    },
    settings: () => setSettingsOpen(true),
    sidebar: toggleSidebar,
    "command-palette": () => setCommandPaletteOpen((open) => !open),
  });

  return (
    <div className="flex h-full w-full flex-col overflow-hidden">
      <TopBar />
      <div className="flex min-h-0 flex-1 w-full">
        <AppSidebar
          activeChatId={activeSession?.chat.id ?? null}
          runningChatIds={runningChatIds}
          chats={chats}
          onNewChat={() => startNewChat()}
          onSelectChat={(chatId) => selectChat(chatId)}
          onUpdateChat={async (chatId, changes) => {
            await daemonApi.updateChat(chatId, changes);
            await refreshChats();
          }}
          onDeleteChat={async (chatId) => {
            await daemonApi.deleteChat(chatId);
            if (activeSession?.chat.id === chatId) startNewChat();
            await refreshChats();
          }}
          onOpenSettings={() => setSettingsOpen(true)}
          updateAvailable={updateAvailable}
          updateBusy={updateChecking || updating}
          onCheckForUpdates={() => void checkUpdates(true)}
        />
        <div className="flex min-h-0 min-w-0 flex-1">
          <div
            className={
              workspaceMaximized ? "hidden" : "flex min-h-0 min-w-0 flex-1"
            }
          >
            {activeSession ? (
              <LiveChatScreen />
            ) : (
              <main
                data-home-stage
                className="relative flex min-h-0 min-w-0 flex-1 items-center justify-center overflow-hidden px-8 py-12"
              >
                <HomeWatermark />
                <div
                  data-prompt-shell
                  className="relative z-10 w-full max-w-2xl"
                >
                  <MainPromptInput
                    workspacePath={workspacePath}
                    onWorkspacePathChange={setWorkspacePath}
                    selectedAgentId={selectedAgent?.id ?? ""}
                    onAgentSelected={(agent) => selectAgentById(agent.id)}
                    onChatStarted={(chat, agent) => {
                      openStartedChat({ chat, agent });
                      void refreshChats();
                    }}
                    onStartedPromptFailed={(chatId, error) => {
                      failStartedPrompt({ chatId, error });
                      startNewChat();
                      void refreshChats();
                    }}
                  />
                </div>
              </main>
            )}
          </div>
          <AppSidePanel workspacePath={workspacePath} />
        </div>
      </div>
      <Toaster
        position="bottom-right"
        closeButton
        theme={toasterTheme}
        className="pointer-events-auto z-100!"
      />
      <AppCommandPalette
        open={commandPaletteOpen}
        onOpenChange={setCommandPaletteOpen}
      />
      <SettingsDialog
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        theme={theme}
        onThemeChange={setTheme}
        palette={palette}
        onPaletteChange={setPalette}
        agents={agents}
        defaultAgentId={defaultAgentId}
        defaultWorkspacePath={defaultWorkspacePath}
        onDefaultWorkspacePathChange={setDefaultWorkspacePath}
        onDefaultAgentChange={(agentId) => {
          setDefaultAgentId(agentId);
          selectAgentById(agentId);
        }}
      />
      {daemonConnection.status !== "ready" && (
        <DaemonConnectionDialog
          status={daemonConnection}
          onRetry={retryDaemon}
          onInstall={() => void installDaemon()}
          onCloseApplication={() => void getCurrentWindow().close()}
        />
      )}
      <UpdateDialog
        open={updateOpen}
        plan={updatePlan}
        progress={updateProgress}
        error={updateError}
        updating={updating}
        onConfirm={() => void installUpdates()}
        onClose={closeUpdate}
      />
    </div>
  );
}
