import { useEffect, useState } from "react";
import MainPromptInput, { type SessionMode } from "@/components/main-prompt-input";
import { AppSidebar } from "@/components/app-sidebar";
import { LiveChatScreen } from "@/components/live-chat-screen";
import { TopBar } from "@/components/top-bar";
import { SettingsDialog } from "@/components/settings-dialog";
import { Toaster } from "@/components/ui/sonner";
import { useChats } from "@/hooks/use-chats";
import { useDaemonEvents } from "@/hooks/use-daemon-events";
import { useTheme } from "@/hooks/use-theme";
import { useAgentCatalog } from "@/hooks/use-agent-catalog";
import { toast } from "sonner";
import type { AgentDefinition, Chat } from "@/types";

/**
 * Deliberately minimal application shell.
 *
 * The previous session/sidebar/controller composition was tied to RPC methods
 * that no longer exist. New chat state will be introduced here only after the
 * daemon-backed controller layer is designed.
 */
export default function App() {
  const { theme, setTheme, palette, setPalette } = useTheme();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [workspacePath, setWorkspacePath] = useState("");
  const [defaultAgentId, setDefaultAgentId] = useState(() => localStorage.getItem("amarcode-default-agent") ?? "codex-acp");
  const [defaultSessionMode, setDefaultSessionMode] = useState<SessionMode>(() => {
    const saved = localStorage.getItem("amarcode-default-session-mode");
    return saved === "plan" || saved === "build" || saved === "ask" ? saved : "build";
  });
  const [newChatMode, setNewChatMode] = useState<SessionMode>(defaultSessionMode);
  const [selectedAgent, setSelectedAgent] = useState<AgentDefinition>();
  const agents = useAgentCatalog();
  const [chatSession, setChatSession] = useState<{
    chat: Chat;
    agent?: AgentDefinition;
    initialRunId: string | null;
    sessionMode?: SessionMode;
  } | null>(null);
  const { chats, handleNewChat, handleSelectChat, refresh } = useChats(setChatSession);
  const daemonEvent = useDaemonEvents();

  useEffect(() => {
    void refresh().catch((error: unknown) => {
      console.error("Failed to load chats:", error);
      toast.error("Failed to load chats. Please try again.");
    });
  }, [refresh]);

  useEffect(() => {
    if (daemonEvent?.type === "chatUpdated") void refresh();
  }, [daemonEvent, refresh]);

  useEffect(() => { localStorage.setItem("amarcode-default-agent", defaultAgentId); }, [defaultAgentId]);
  useEffect(() => {
    localStorage.setItem("amarcode-default-session-mode", defaultSessionMode);
    setNewChatMode(defaultSessionMode);
  }, [defaultSessionMode]);
  useEffect(() => {
    if (!selectedAgent && agents.length) {
      setSelectedAgent(agents.find((agent) => agent.id === defaultAgentId) ?? agents[0]);
    }
  }, [agents, defaultAgentId, selectedAgent]);

  const selectWorkspace = (path: string) => {
    setWorkspacePath(path);
    setChatSession(null);
  };

  const startNewChat = () => {
    setSelectedAgent(agents.find((agent) => agent.id === defaultAgentId) ?? agents[0]);
    setNewChatMode(defaultSessionMode);
    handleNewChat();
  };

  return (
    <>
      <TopBar />
      <div className="flex h-svh pt-9 w-full">
        <AppSidebar
          activeChatId={chatSession?.chat.id ?? null}
          workspacePath={workspacePath}
          chats={chats}
          onNewChat={startNewChat}
          onSelectChat={handleSelectChat}
          onOpenSettings={() => setSettingsOpen(true)}
        />
        {chatSession ? (
          <LiveChatScreen
            agent={chatSession.agent ?? selectedAgent}
            initialChatId={chatSession.chat.id}
            initialRunId={chatSession.initialRunId}
            initialSessionMode={chatSession.sessionMode}
            workspacePath={workspacePath}
            onChatsRefresh={refresh}
            daemonEvent={daemonEvent}
            onAgentSelected={(agent) => {
              setSelectedAgent(agent);
              setChatSession((session) => session ? { ...session, agent } : session);
            }}
          />
        ) : (
          <main className="m-auto w-full max-w-2xl px-8 py-12">
            <MainPromptInput
              workspacePath={workspacePath}
              onWorkspacePathChange={selectWorkspace}
              selectedAgentId={selectedAgent?.id ?? ""}
              onAgentSelected={setSelectedAgent}
              sessionMode={newChatMode}
              onSessionModeChange={setNewChatMode}
              onChatStarted={(chat, agent, _workspacePath, sessionMode) => {
                setChatSession({ chat, agent, initialRunId: null, sessionMode });
                void refresh();
              }}
            />
          </main>
        )}
      </div>
      <Toaster
        position="bottom-right"
        closeButton
        theme={theme}
        className="pointer-events-auto !z-[100]"
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
          const agent = agents.find((candidate) => candidate.id === agentId);
          if (agent) setSelectedAgent(agent);
        }}
        defaultSessionMode={defaultSessionMode}
        onDefaultSessionModeChange={setDefaultSessionMode}
      />
    </>
  );
}
