import { useAtom, useAtomValue, useSetAtom } from "jotai";
import { getCurrentWindow } from "@tauri-apps/api/window";
import MainPromptInput from "@/components/main-prompt-input";
import { AppSidebar } from "@/components/app-sidebar";
import { HomeWatermark } from "@/components/home-watermark";
import { LiveChatScreen } from "@/components/live-chat-screen";
import { TopBar } from "@/components/top-bar";
import { SettingsDialog } from "@/components/settings-dialog";
import { DaemonConnectionDialog } from "@/components/daemon-connection-dialog";
import { DaemonUpdateDialog } from "@/components/daemon-update-dialog";
import { Toaster } from "@/components/ui/sonner";
import { daemonApi } from "@/api";
import {
  activeSessionAtom,
  agentsAtom,
  chatsAtom,
  composerSessionModeAtom,
  defaultAgentIdAtom,
  defaultSessionModeAtom,
  openStartedChatAtom,
  paletteAtom,
  refreshChatsAtom,
  selectAgentByIdAtom,
  selectedAgentAtom,
  selectChatAtom,
  setWorkspacePathAtom,
  settingsOpenAtom,
  startNewChatAtom,
  themeAtom,
  useAppBootstrap,
  workspacePathAtom,
} from "@/state";

/**
 * Shell only: route home vs live chat and host chrome.
 * Domain state lives in `src/state/*` (jotai).
 */
export default function App() {
  const {
    daemonConnection,
    retryDaemon,
    installDaemon,
    daemonUpdateVersion,
    daemonUpdateStatus,
    updateDaemon,
    closeDaemonUpdate,
  } = useAppBootstrap();

  const [theme, setTheme] = useAtom(themeAtom);
  const [palette, setPalette] = useAtom(paletteAtom);
  const [settingsOpen, setSettingsOpen] = useAtom(settingsOpenAtom);
  const workspacePath = useAtomValue(workspacePathAtom);
  const setWorkspacePath = useSetAtom(setWorkspacePathAtom);
  const agents = useAtomValue(agentsAtom);
  const selectedAgent = useAtomValue(selectedAgentAtom);
  const [defaultAgentId, setDefaultAgentId] = useAtom(defaultAgentIdAtom);
  const [defaultSessionMode, setDefaultSessionMode] = useAtom(
    defaultSessionModeAtom,
  );
  const [composerMode, setComposerMode] = useAtom(composerSessionModeAtom);
  const chats = useAtomValue(chatsAtom);
  const activeSession = useAtomValue(activeSessionAtom);

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

  return (
    <div className="flex h-full w-full flex-col overflow-hidden">
      <TopBar />
      <div className="flex min-h-0 flex-1 w-full">
        <AppSidebar
          activeChatId={activeSession?.chat.id ?? null}
          workspacePath={workspacePath}
          chats={chats}
          onNewChat={() => startNewChat()}
                onSelectChat={(chatId) => selectChat(chatId)}
                onDeleteChat={async (chatId) => {
                  await daemonApi.deleteChat(chatId);
                  if (activeSession?.chat.id === chatId) startNewChat();
                  await refreshChats();
                }}
          onOpenSettings={() => setSettingsOpen(true)}
        />
        {activeSession ? (
          <LiveChatScreen />
        ) : (
          <main
            data-home-stage
            className="relative flex min-h-0 flex-1 items-center justify-center overflow-hidden px-8 py-12"
          >
            <HomeWatermark />
            <div data-prompt-shell className="relative z-10 w-full max-w-2xl">
              <MainPromptInput
                workspacePath={workspacePath}
                onWorkspacePathChange={setWorkspacePath}
                selectedAgentId={selectedAgent?.id ?? ""}
                onAgentSelected={(agent) => selectAgentById(agent.id)}
                sessionMode={composerMode}
                onSessionModeChange={setComposerMode}
                onChatStarted={(chat, agent, _workspacePath, sessionMode) => {
                  openStartedChat({ chat, agent, sessionMode });
                  void refreshChats();
                }}
              />
            </div>
          </main>
        )}
      </div>
      <Toaster
        position="bottom-right"
        closeButton
        theme={toasterTheme}
        className="pointer-events-auto z-100!"
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
        onDefaultAgentChange={(agentId) => {
          setDefaultAgentId(agentId);
          selectAgentById(agentId);
        }}
        defaultSessionMode={defaultSessionMode}
        onDefaultSessionModeChange={setDefaultSessionMode}
      />
      {daemonConnection.status !== "ready" && (
        <DaemonConnectionDialog
          status={daemonConnection}
          onRetry={retryDaemon}
          onInstall={() => void installDaemon()}
          onCloseApplication={() => void getCurrentWindow().close()}
        />
      )}
      <DaemonUpdateDialog
        version={daemonUpdateVersion}
        status={daemonUpdateStatus}
        onConfirm={() => void updateDaemon()}
        onClose={closeDaemonUpdate}
      />
    </div>
  );
}
