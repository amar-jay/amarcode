import { QueuedPrompts } from "./queued-prompts";
import { ChatHistoryLoader } from "./chat-history-loader";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useAtomValue, useSetAtom } from "jotai";
import { LoaderCircle } from "lucide-react";
import {
  Conversation,
  ConversationContent,
  ConversationEmptyState,
  ConversationScrollButton,
} from "@/components/ai-elements/conversation";
import { Message, MessageContent } from "@/components/ai-elements/message";
import { Shimmer } from "@/components/ai-elements/shimmer";
import type { AcpEvent, AgentRun, PromptAttachment } from "@/types";
import { shortcutOverlayOpen } from "@/lib/app-shortcuts";
import { daemonApi } from "@/api";
import AppPromptInput from "./main-prompt-input";
import { ActivityView } from "./activity/activity-view";
import { LiveChatFailureToast } from "./live-chat-failure-toast";
import { PendingAgentRequestCard } from "./pending-agent-request";
import {
  activeSessionAtom,
  agentsAtom,
  applyLiveChatEventAtom,
  bindSessionAgentAtom,
  clearLiveChatFailureAtom,
  liveChatAtom,
  liveChatIsWorkingAtom,
  liveChatHasQueuedDeliveryAtom,
  loadLiveChatAtom,
  loadOlderChatMessagesAtom,
  openLiveChatAtom,
  refreshChatsAtom,
  respondLiveRequestAtom,
  selectedAgentAtom,
  setLiveSessionConfigOptionAtom,
  startNewChatAtom,
  stopLiveChatAtom,
  submitLivePromptAtom,
  updatePromptQueueAtom,
  subscribeDaemonEvents,
  daemonEventStreamStateAtom,
  verboseReasoningAtom,
  sidePanelMaximizedAtom,
} from "@/state";
import {
  groupChatBlocks,
  type ChatBlock,
  type ChatBlockCache,
} from "@/lib/message-parsing";
import { UserMessage } from "./user-message";

import { LiveChatHeader, type ConversationView } from "./live-chat-header";
import { chatSearchVisibleAtom } from "@/state/chat-header";

function searchableBlockText(block: ChatBlock): string {
  if (block.kind === "user") return block.item.message.content;
  return [
    block.content,
    ...block.timeline.flatMap((step) => [step.label, step.description ?? ""]),
    ...block.diffs.flatMap((artifact) => [
      artifact.title,
      ...artifact.changes.map((change) => change.path),
    ]),
  ].join("\n");
}

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
  const [conversationView, setConversationView] =
    useState<ConversationView>("chat");
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchChatId, setSearchChatId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [activeMatchIndex, setActiveMatchIndex] = useState(0);
  const [activityEvents, setActivityEvents] = useState<AcpEvent[]>([]);
  const [activityRuns, setActivityRuns] = useState<AgentRun[]>([]);
  const [activityLoading, setActivityLoading] = useState(false);
  const [activityError, setActivityError] = useState<string | null>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const session = useAtomValue(activeSessionAtom);
  const workspaceMaximized = useAtomValue(sidePanelMaximizedAtom);
  const live = useAtomValue(liveChatAtom);
  const eventStream = useAtomValue(daemonEventStreamStateAtom);
  const isWorking = useAtomValue(liveChatIsWorkingAtom);
  const hasPendingQueueDelivery = useAtomValue(liveChatHasQueuedDeliveryAtom);
  const verboseReasoning = useAtomValue(verboseReasoningAtom);
  const agents = useAtomValue(agentsAtom);
  const agent = useAtomValue(selectedAgentAtom) ?? session?.agent;
  const workspacePath = session?.chat.workspace_path ?? "";

  const openLiveChat = useSetAtom(openLiveChatAtom);
  const loadLiveChat = useSetAtom(loadLiveChatAtom);
  const loadOlderMessages = useSetAtom(loadOlderChatMessagesAtom);
  const applyEvent = useSetAtom(applyLiveChatEventAtom);
  const submitPrompt = useSetAtom(submitLivePromptAtom);
  const updateQueue = useSetAtom(updatePromptQueueAtom);
  const changeSessionConfig = useSetAtom(setLiveSessionConfigOptionAtom);
  const stop = useSetAtom(stopLiveChatAtom);
  const respond = useSetAtom(respondLiveRequestAtom);
  const bindAgent = useSetAtom(bindSessionAgentAtom);
  const clearFailure = useSetAtom(clearLiveChatFailureAtom);
  const startNewChat = useSetAtom(startNewChatAtom);
  const refreshChats = useSetAtom(refreshChatsAtom);

  // Open only when chat identity changes. Seed (turn-active, mode) is read
  // from activeSessionAtom inside the write atom: don't re-open on those.
  const chatId = session?.chat.id;
  const showSearch =
    conversationView === "chat" && searchOpen && searchChatId === chatId;

  useEffect(() => {
    queueMicrotask(() => {
      setConversationView("chat");
      setSearchOpen(false);
    });
  }, [chatId]);

  useEffect(() => {
    if (conversationView !== "activity" || !chatId) return;

    let active = true;
    queueMicrotask(() => {
      if (!active) return;
      setActivityEvents([]);
      setActivityRuns([]);
      setActivityError(null);
      setActivityLoading(true);
    });
    void Promise.all([
      daemonApi.listAcpEventsForChat(chatId),
      daemonApi.listAgentRunsForChat(chatId),
    ])
      .then(([events, runs]) => {
        if (!active) return;
        setActivityEvents(events);
        setActivityRuns(runs);
      })
      .catch((cause: unknown) => {
        if (!active) return;
        setActivityError(
          cause instanceof Error ? cause.message : String(cause),
        );
      })
      .finally(() => {
        if (active) setActivityLoading(false);
      });

    return () => {
      active = false;
    };
  }, [chatId, conversationView]);

  useEffect(() => {
    if (!chatId) return;
    openLiveChat(chatId);
  }, [chatId, openLiveChat]);

  // Initial + id-change load.
  useEffect(() => {
    if (!live?.chatId || eventStream.status !== "connected") return;
    void loadLiveChat(live.chatId);
  }, [live?.chatId, loadLiveChat, eventStream.status]);

  // Every daemon event → live chat reducer (not just the latest atom value).
  useEffect(() => {
    return subscribeDaemonEvents((event) => {
      applyEvent(event);
    });
  }, [applyEvent]);

  const messages = useMemo(
    () => live?.detail?.messages ?? [],
    [live?.detail?.messages],
  );
  const blockCache = useMemo(() => {
    void chatId;
    return new Map() as ChatBlockCache;
  }, [chatId]);
  const blocks = useMemo(
    () => groupChatBlocks(messages, isWorking, verboseReasoning, blockCache),
    [messages, isWorking, verboseReasoning, blockCache],
  );

  const normalizedSearchQuery = searchQuery.trim().toLocaleLowerCase();
  const matchingBlockKeys = useMemo(
    () =>
      normalizedSearchQuery
        ? blocks
            .filter((block) =>
              searchableBlockText(block)
                .toLocaleLowerCase()
                .includes(normalizedSearchQuery),
            )
            .map((block) => block.key)
        : [],
    [blocks, normalizedSearchQuery],
  );
  const matchingBlockKeySet = useMemo(
    () => new Set(matchingBlockKeys),
    [matchingBlockKeys],
  );
  const safeActiveMatchIndex = Math.min(
    activeMatchIndex,
    Math.max(0, matchingBlockKeys.length - 1),
  );
  const activeMatchKey = matchingBlockKeys[safeActiveMatchIndex] ?? null;
  const agentNames = useMemo(
    () => new Map(agents.map((candidate) => [candidate.id, candidate.name])),
    [agents],
  );

  const closeSearch = useCallback(() => {
    setSearchOpen(false);
    setSearchQuery("");
    setActiveMatchIndex(0);
  }, []);

  const openSearch = useCallback(() => {
    if (searchOpen && searchChatId === chatId) {
      searchInputRef.current?.focus();
      searchInputRef.current?.select();
      return;
    }
    setSearchChatId(chatId ?? null);
    setSearchQuery("");
    setActiveMatchIndex(0);
    setSearchOpen(true);
  }, [chatId, searchChatId, searchOpen]);

  const moveSearchResult = useCallback(
    (direction: 1 | -1) => {
      if (!matchingBlockKeys.length) return;
      setActiveMatchIndex(
        (current) =>
          (Math.min(current, matchingBlockKeys.length - 1) +
            direction +
            matchingBlockKeys.length) %
          matchingBlockKeys.length,
      );
    },
    [matchingBlockKeys.length],
  );

  const setSearchVisible = useSetAtom(chatSearchVisibleAtom);
  useEffect(() => {
    if (!showSearch || workspaceMaximized) return;
    searchInputRef.current?.focus();
  }, [showSearch, workspaceMaximized]);

  useEffect(() => {
    setSearchVisible(showSearch);
    return () => setSearchVisible(false);
  }, [setSearchVisible, showSearch]);

  useEffect(() => {
    if (!activeMatchKey) return;
    document
      .querySelector<HTMLElement>(
        `[data-chat-search-key="${CSS.escape(activeMatchKey)}"]`,
      )
      ?.scrollIntoView({ behavior: "smooth", block: "center" });
  }, [activeMatchKey]);

  useEffect(() => {
    if (workspaceMaximized) return;
    const handleFindShortcut = (event: KeyboardEvent) => {
      if (
        event.defaultPrevented ||
        event.isComposing ||
        event.repeat ||
        event.altKey ||
        shortcutOverlayOpen()
      )
        return;
      if (
        (event.metaKey || event.ctrlKey) &&
        !event.shiftKey &&
        event.key.toLowerCase() === "f"
      ) {
        event.preventDefault();
        openSearch();
      }
    };
    window.addEventListener("keydown", handleFindShortcut);
    return () => window.removeEventListener("keydown", handleFindShortcut);
  }, [openSearch, workspaceMaximized]);

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
    configValues: import("@/types").SessionConfigAssignment[],
  ) => {
    if (!agent) return;
    await submitPrompt({ text, attachments, configValues, agentId: agent.id });
  };

  return (
    <main className="flex min-w-0 flex-1 flex-col">
      <LiveChatHeader
        title={live.detail?.chat.title ?? session.chat.title ?? "Loading chat"}
        conversationView={conversationView}
        onConversationViewChange={(view) => {
          closeSearch();
          setConversationView(view);
        }}
        showSearch={showSearch}
        searchQuery={searchQuery}
        onSearchQueryChange={(query) => {
          setSearchQuery(query);
          setActiveMatchIndex(0);
        }}
        searchInputRef={searchInputRef}
        hasOlderMessages={!!live.detail?.has_older_messages}
        matchCount={matchingBlockKeys.length}
        activeMatchIndex={safeActiveMatchIndex}
        openSearch={openSearch}
        closeSearch={closeSearch}
        moveSearchResult={moveSearchResult}
      />
      <Conversation
        initial="instant"
        resize="instant"
        className={conversationView === "activity" ? "hidden" : ""}
      >
        <ConversationContent className="mx-auto w-full max-w-3xl gap-2 py-8">
          <ChatHistoryLoader
            oldestId={messages[0]?.message.id}
            hasOlder={Boolean(live.detail?.has_older_messages)}
            loading={Boolean(live.olderMessagesLoading)}
            error={live.olderMessagesError}
            onLoad={loadOlderMessages}
          />
          {blocks.map((block) => {
            const matches = matchingBlockKeySet.has(block.key);
            const active = block.key === activeMatchKey;
            return (
              <div
                key={block.key}
                data-chat-search-key={block.key}
                style={
                  block.kind === "assistant" && !block.streaming
                    ? {
                        contentVisibility: "auto",
                        containIntrinsicSize: "auto 160px",
                      }
                    : undefined
                }
                className={`rounded-lg transition-[background-color,box-shadow] motion-reduce:scroll-auto ${
                  active
                    ? "bg-primary/8 ring-2 ring-primary/35"
                    : matches
                      ? "bg-primary/4 ring-1 ring-primary/15"
                      : ""
                }`}
              >
                <UserMessage
                  block={block}
                  verboseReasoning={verboseReasoning}
                  waitingLabel={waitingLabel}
                  agentNames={agentNames}
                />
              </div>
            );
          })}
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
      <div
        className={
          conversationView === "activity" ? "flex min-h-0 flex-1" : "hidden"
        }
      >
        {conversationView === "activity" && (
          <ActivityView
            key={chatId}
            events={activityEvents}
            runs={activityRuns}
            loading={activityLoading}
            error={activityError}
          />
        )}
      </div>
      <div className="px-4 pb-4">
        {live.pendingRequest && (
          <PendingAgentRequestCard
            key={live.pendingRequest.requestId}
            request={live.pendingRequest}
            onRespond={async (result) => {
              await respond(result);
            }}
          />
        )}
        {live.error && (
          <LiveChatFailureToast
            error={live.error}
            errorKind={live.errorKind}
            authRequired={live.authRequired}
            canDeleteChat={!live.loading && !isWorking && messages.length === 0}
            onSignedIn={() => clearFailure()}
            onDeleteChat={async () => {
              await daemonApi.deleteChat(live.chatId);
              startNewChat();
              await refreshChats();
            }}
          />
        )}
        <QueuedPrompts
          key={`queue-${chatId}`}
          queue={live.detail?.prompt_queue}
          onUpdate={updateQueue}
        />
        <AppPromptInput
          chatId={chatId ?? undefined}
          key={chatId}
          workspacePath={workspacePath}
          selectedAgentId={agent?.id ?? ""}
          onAgentSelected={(next) => bindAgent(next)}
          onSendPrompt={submit}
          sessionCommands={live.sessionCommands}
          sessionConfig={live.sessionConfig}
          sessionConfigAgentId={live.sessionConfigAgentId}
          contextUsage={live.contextUsage}
          onSessionConfigChange={(configId, value) =>
            changeSessionConfig({ configId, value })
          }
          isWorking={isWorking}
          hasQueuedPrompts={
            hasPendingQueueDelivery ||
            Boolean(live.detail?.prompt_queue?.items.length)
          }
          onStop={() => void stop()}
        />
      </div>
    </main>
  );
}
