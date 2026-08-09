import { useEffect, useState } from "react";
import MainPromptInput from "@/components/main-prompt-input";
import { AppSidebar } from "@/components/app-sidebar";
import { LiveChatScreen } from "@/components/live-chat-screen";
import { TopBar } from "@/components/top-bar";
import { Toaster } from "@/components/ui/sonner";
import { useChats } from "@/hooks/use-chats";
import { useTheme } from "@/hooks/use-theme";
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
  const { theme } = useTheme();
  const [workspacePath, setWorkspacePath] = useState("");
  const [selectedAgent, setSelectedAgent] = useState<AgentDefinition>();
  const [chatSession, setChatSession] = useState<{
    chat: Chat;
    agent?: AgentDefinition;
    initialRunId: string | null;
  } | null>(null);
  const { chats, handleNewChat, handleSelectChat, refresh } = useChats(workspacePath, setChatSession);

  useEffect(() => {
    void refresh().catch((error: unknown) => {
      console.error("Failed to load chats:", error);
      toast.error("Failed to load chats. Please try again.");
    });
  }, [refresh]);

  const selectWorkspace = (path: string) => {
    setWorkspacePath(path);
    setChatSession(null);
  };

  return (
    <>
      <TopBar />
      <div className="flex h-svh pt-9 w-full">
        <AppSidebar
          activeChatId={chatSession?.chat.id ?? null}
          workspacePath={workspacePath}
          chats={chats}
          onNewChat={handleNewChat}
          onSelectChat={handleSelectChat}
        />
        {chatSession ? (
          <LiveChatScreen
            agent={chatSession.agent ?? selectedAgent}
            initialChatId={chatSession.chat.id}
            initialRunId={chatSession.initialRunId}
            workspacePath={workspacePath}
            onChatsRefresh={refresh}
          />
        ) : (
          <main className="m-auto w-full max-w-2xl px-8 py-12">
            <MainPromptInput
              workspacePath={workspacePath}
              onWorkspacePathChange={selectWorkspace}
              selectedAgentId={selectedAgent?.id ?? ""}
              onAgentSelected={setSelectedAgent}
              onChatStarted={(chat, agent, prompt) => {
                setChatSession({ chat, agent, initialRunId: prompt.run_id });
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
    </>
  );
}
