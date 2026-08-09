import { useCallback, useEffect, useRef, useState } from "react";
import { LoaderCircle } from "lucide-react";
import { daemonApi } from "@/api";
import {
  Conversation,
  ConversationContent,
  ConversationEmptyState,
  ConversationScrollButton,
} from "@/components/ai-elements/conversation";
import { Message, MessageContent, MessageResponse } from "@/components/ai-elements/message";
import { WorkbenchPromptInput } from "@/components/prompt-input";
import type { AgentDefinition, Chat, ChatDetail, EditorEvent } from "@/types";
type LiveChatScreenProps = {
  workspacePath: string;
  agent: AgentDefinition | undefined;
  initialChatId: string;
  initialRunId: string | null;
  onChatsRefresh: () => Promise<void>;
};

function isChatDetail(value: Chat | ChatDetail): value is ChatDetail {
  return "messages" in value;
}

export function LiveChatScreen({ workspacePath, agent, initialChatId, initialRunId, onChatsRefresh }: LiveChatScreenProps) {
  const [activeChatId, setActiveChatId] = useState(initialChatId);
  const [activeChat, setActiveChat] = useState<ChatDetail | null>(null);
  const [runId, setRunId] = useState<string | null>(initialRunId);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const activeChatIdRef = useRef(activeChatId);
  const runIdRef = useRef(runId);

  useEffect(() => { activeChatIdRef.current = activeChatId; }, [activeChatId]);
  useEffect(() => { runIdRef.current = runId; }, [runId]);

  const loadChat = useCallback(async (chatId: string) => {
    setLoading(true);
    try {
      const result = await daemonApi.getChat(chatId, true);
      if (isChatDetail(result)) setActiveChat(result);
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to load this chat.");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { void loadChat(activeChatId); }, [activeChatId, loadChat]);

  useEffect(() => {
    let refreshTimer: ReturnType<typeof setTimeout> | undefined;
    const scheduleRefresh = () => {
      if (refreshTimer) clearTimeout(refreshTimer);
      refreshTimer = setTimeout(() => void loadChat(activeChatIdRef.current), 120);
    };
    const handleEvent = (event: EditorEvent) => {
      if (event.type === "chatUpdated" && event.payload.chat_id === activeChatIdRef.current) {
        scheduleRefresh();
        void onChatsRefresh();
      }
      if (event.type === "runUpdated" && event.payload.run_id === runIdRef.current) {
        scheduleRefresh();
        if (["completed", "stopped", "failed"].includes(event.payload.status)) setRunId(null);
      }
      // The daemon intentionally omits transcript content from these events.
      // Re-fetching is the sole path to render new assistant content.
      if ((event.type === "messageUpdated" || event.type === "messagePartAdded") && runIdRef.current) scheduleRefresh();
    };
    void daemonApi.subscribeEvents({}, handleEvent).catch((cause: unknown) => {
      setError(cause instanceof Error ? cause.message : "Live updates disconnected.");
    });
    return () => { if (refreshTimer) clearTimeout(refreshTimer); };
  }, [loadChat]);

  const submit = async ({ text }: { text: string }) => {
    if (!agent || !text.trim()) return;
    try {
      const result = await daemonApi.prompt(activeChatId, agent.id, text.trim());
      setRunId(result.run_id);
      await onChatsRefresh();
      await loadChat(activeChatId);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to send prompt.");
    }
  };

  const stop = async () => {
    try {
      await daemonApi.cancel(activeChatId);
      setRunId(null);
      await loadChat(activeChatId);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to cancel the run.");
    }
  };

  return (
    <main className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 items-center border-b px-6">
          <h1 className="truncate text-sm font-medium">{activeChat?.chat.title ?? "Loading chat"}</h1>
          {loading && <LoaderCircle className="ml-2 size-4 animate-spin text-muted-foreground" />}
        </header>
        <Conversation>
          <ConversationContent className="mx-auto w-full max-w-3xl px-6 py-8">
            {activeChat?.messages.map(({ message }) => (
              <Message from={message.role === "user" ? "user" : "assistant"} key={message.id}>
                <MessageContent>
                  {message.role === "assistant" ? <MessageResponse>{message.content}</MessageResponse> : <p className="whitespace-pre-wrap">{message.content}</p>}
                </MessageContent>
              </Message>
            ))}
            {!loading && !activeChat?.messages.length && <ConversationEmptyState title="This chat is ready" description="Send a prompt to start working with your agent." />}
          </ConversationContent>
          <ConversationScrollButton />
        </Conversation>
        <div className="border-t p-4">
          {error && <p className="mx-auto mb-2 max-w-3xl text-sm text-destructive">{error}</p>}
          <WorkbenchPromptInput agent={agent} workspacePath={workspacePath} isWorking={Boolean(runId)} onSubmit={submit} onStop={() => void stop()} />
        </div>
    </main>
  );
}
