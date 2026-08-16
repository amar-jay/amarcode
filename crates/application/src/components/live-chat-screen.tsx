import { useEffect, useMemo } from "react";
import { useAtomValue, useSetAtom } from "jotai";
import { LoaderCircle } from "lucide-react";
import {
  Conversation,
  ConversationContent,
  ConversationEmptyState,
  ConversationScrollButton,
} from "@/components/ai-elements/conversation";
import {
  Message,
  MessageContent,
} from "@/components/ai-elements/message";
import { Shimmer } from "@/components/ai-elements/shimmer";
import type { PromptAttachment } from "@/types";
import AppPromptInput from "./main-prompt-input";
import { PendingAgentRequestCard } from "./pending-agent-request";
import {
  activeSessionAtom,
  applyLiveChatEventAtom,
  bindSessionAgentAtom,
  liveChatAtom,
  liveChatIsWorkingAtom,
  loadLiveChatAtom,
  openLiveChatAtom,
  respondLiveRequestAtom,
  selectedAgentAtom,
  setLiveSessionModeAtom,
  stopLiveChatAtom,
  submitLivePromptAtom,
  subscribeDaemonEvents,
  verboseReasoningAtom,
  type SessionMode,
} from "@/state";
import {
  groupChatBlocks,
} from "@/lib/message-parsing";
import { UserMessage } from "./user-message";

function TurnLoadingIndicator({ label = "Thinking" }: { label?: string }) {
  return (
    <Message from="assistant" className="space-between">
      <MessageContent className="w-full">
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <LoaderCircle className="size-3.5 shrink-0 animate-spin" />
          <Shimmer className="text-sm" duration={1.4}>
            {`${label}…`}
          </Shimmer>
        </div>
      </MessageContent>
    </Message>
  );
}

/**
 * Live conversation surface. State lives in `liveChatAtom` (+ navigation /
 * agent atoms); this component is render + wire-up only.
 */
export function LiveChatScreen() {
  const session = useAtomValue(activeSessionAtom);
  const live = useAtomValue(liveChatAtom);
  const isWorking = useAtomValue(liveChatIsWorkingAtom);
  const verboseReasoning = useAtomValue(verboseReasoningAtom);
  const agent = useAtomValue(selectedAgentAtom) ?? session?.agent;
  const workspacePath = session?.chat.workspace_path ?? "";

  const openLiveChat = useSetAtom(openLiveChatAtom);
  const loadLiveChat = useSetAtom(loadLiveChatAtom);
  const applyEvent = useSetAtom(applyLiveChatEventAtom);
  const submitPrompt = useSetAtom(submitLivePromptAtom);
  const changeMode = useSetAtom(setLiveSessionModeAtom);
  const stop = useSetAtom(stopLiveChatAtom);
  const respond = useSetAtom(respondLiveRequestAtom);
  const bindAgent = useSetAtom(bindSessionAgentAtom);

  // Open only when chat identity changes. Seed (turn-active, mode) is read
  // from activeSessionAtom inside the write atom — don't re-open on those.
  const chatId = session?.chat.id;

  useEffect(() => {
    if (!chatId) return;
    openLiveChat(chatId);
  }, [chatId, openLiveChat]);

  // Initial + id-change load.
  useEffect(() => {
    if (!live?.chatId) return;
    void loadLiveChat(live.chatId);
  }, [live?.chatId, loadLiveChat]);

  // Every daemon event → live chat reducer (not just the latest atom value).
  useEffect(() => {
    return subscribeDaemonEvents((event) => {
      applyEvent(event);
    });
  }, [applyEvent]);

  const messages = live?.detail?.messages ?? [];
  const blocks = useMemo(
    () => groupChatBlocks(messages, isWorking, verboseReasoning),
    [messages, isWorking, verboseReasoning],
  );

  if (!session || !live || live.chatId !== session.chat.id) {
    return (
      <main className="flex min-w-0 flex-1 items-center justify-center">
        <LoaderCircle className="size-5 animate-spin text-muted-foreground" />
      </main>
    );
  }

  const lastMessage = messages[messages.length - 1]?.message;
  const lastAssistantBlock = [...blocks]
    .reverse()
    .find((block) => block.kind === "assistant");
  // Only show a bottom placeholder when nothing assistant-visible exists yet.
  const showTurnPlaceholder =
    isWorking &&
    (!lastMessage || lastMessage.role === "user" || !lastAssistantBlock);
  const waitingLabel = live.pendingRequest
    ? live.pendingRequest.kind === "approval"
      ? "Waiting for approval"
      : "Waiting for input"
    : "Thinking";

  const submit = async (
    text: string,
    attachments: PromptAttachment[],
    mode: SessionMode,
  ) => {
    if (!agent) return;
    await submitPrompt({ text, attachments, mode, agentId: agent.id });
  };

  return (
    <main className="flex min-w-0 flex-1 flex-col">
      <header className="mr-2 flex items-center border-b px-6 py-1">
        <h1 className="truncate text-sm font-medium">
          {live.detail?.chat.title ?? session.chat.title ?? "Loading chat"}
        </h1>
        {live.loading && (
          <LoaderCircle className="ml-2 size-4 animate-spin text-muted-foreground" />
        )}
        {isWorking && (
          <span className="ml-3 text-xs text-muted-foreground">Working…</span>
        )}
        {live.contextRestoration && (
          <span className="ml-3 text-xs text-muted-foreground">
            {live.contextRestoration}…
          </span>
        )}
        {!isWorking && live.runStatus && live.runStatus !== "running" && (
          <span className="ml-3 text-xs text-muted-foreground">
            {live.runStatus}
          </span>
        )}
      </header>
      <Conversation>
        <ConversationContent className="mx-auto w-full max-w-3xl gap-2 py-8">
          {blocks.map((block) => (
            <UserMessage
              block={block}
              verboseReasoning={verboseReasoning}
              waitingLabel={waitingLabel}
            />
          ))}
          {showTurnPlaceholder && <TurnLoadingIndicator label={waitingLabel} />}
          {!live.loading && !messages.length && !isWorking && (
            <ConversationEmptyState
              title="This chat is ready"
              description="Send a prompt to start working with your agent."
            />
          )}
        </ConversationContent>
        <ConversationScrollButton />
      </Conversation>
      <div className="px-4 pb-4">
        {live.pendingRequest && (
          <PendingAgentRequestCard
            request={live.pendingRequest}
            onRespond={async (result) => {
              await respond(result);
            }}
          />
        )}
        {live.error && (
          <p className="mx-auto mb-2 max-w-3xl text-sm text-destructive">
            {live.error}
          </p>
        )}
        <AppPromptInput
          workspacePath={workspacePath}
          selectedAgentId={agent?.id ?? ""}
          onAgentSelected={(next) => bindAgent(next)}
          onSendPrompt={submit}
          sessionMode={live.sessionMode}
          onSessionModeChange={(mode) => void changeMode(mode)}
          isWorking={isWorking}
          onStop={() => void stop()}
        />
      </div>
    </main>
  );
}
