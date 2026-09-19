import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useAtomValue, useSetAtom } from "jotai";
import {
  ChevronDown,
  ChevronUp,
	RotateCwFadingClock,
  LoaderCircle,
  Search,
  X,
} from "lucide-react";
import {
  Conversation,
  ConversationContent,
  ConversationEmptyState,
  ConversationScrollButton,
} from "@/components/ai-elements/conversation";
import { Message, MessageContent } from "@/components/ai-elements/message";
import { Shimmer } from "@/components/ai-elements/shimmer";
import type {
  AcpEvent,
  AgentRun,
  PromptAttachment,
} from "@/types";
import { daemonApi } from "@/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Toggle } from "@/components/ui/toggle";
import AppPromptInput from "./main-prompt-input";
import { ActivityView } from "./activity/activity-view";
import { ChatStateIndicators } from "./chat-state-indicators";
import { LiveChatFailureBanner } from "./live-chat-failure-banner";
import { PendingAgentRequestCard } from "./pending-agent-request";
import {
  activeSessionAtom,
  agentsAtom,
  applyLiveChatEventAtom,
  bindSessionAgentAtom,
  clearLiveChatFailureAtom,
  liveChatAtom,
  liveChatIsWorkingAtom,
  loadLiveChatAtom,
  openLiveChatAtom,
  refreshChatsAtom,
  respondLiveRequestAtom,
  selectedAgentAtom,
  setLiveSessionConfigOptionAtom,
  startNewChatAtom,
  stopLiveChatAtom,
  submitLivePromptAtom,
  subscribeDaemonEvents,
  verboseReasoningAtom,
} from "@/state";
import { groupChatBlocks, type ChatBlock } from "@/lib/message-parsing";
import { UserMessage } from "./user-message";

type ConversationView = "chat" | "activity";

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
  const live = useAtomValue(liveChatAtom);
  const isWorking = useAtomValue(liveChatIsWorkingAtom);
  const verboseReasoning = useAtomValue(verboseReasoningAtom);
  const agents = useAtomValue(agentsAtom);
  const agent = useAtomValue(selectedAgentAtom) ?? session?.agent;
  const workspacePath = session?.chat.workspace_path ?? "";

  const openLiveChat = useSetAtom(openLiveChatAtom);
  const loadLiveChat = useSetAtom(loadLiveChatAtom);
  const applyEvent = useSetAtom(applyLiveChatEventAtom);
  const submitPrompt = useSetAtom(submitLivePromptAtom);
  const changeSessionConfig = useSetAtom(setLiveSessionConfigOptionAtom);
  const stop = useSetAtom(stopLiveChatAtom);
  const respond = useSetAtom(respondLiveRequestAtom);
  const bindAgent = useSetAtom(bindSessionAgentAtom);
  const clearFailure = useSetAtom(clearLiveChatFailureAtom);
  const startNewChat = useSetAtom(startNewChatAtom);
  const refreshChats = useSetAtom(refreshChatsAtom);

  // Open only when chat identity changes. Seed (turn-active, mode) is read
  // from activeSessionAtom inside the write atom — don't re-open on those.
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
    if (!live?.chatId) return;
    void loadLiveChat(live.chatId);
  }, [live?.chatId, loadLiveChat]);

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
  const blocks = useMemo(
    () => groupChatBlocks(messages, isWorking, verboseReasoning),
    [messages, isWorking, verboseReasoning],
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

  useEffect(() => {
    if (!showSearch) return;
    searchInputRef.current?.focus();
  }, [showSearch]);

  useEffect(() => {
    if (!activeMatchKey) return;
    document
      .querySelector<HTMLElement>(
        `[data-chat-search-key="${CSS.escape(activeMatchKey)}"]`,
      )
      ?.scrollIntoView({ behavior: "smooth", block: "center" });
  }, [activeMatchKey]);

  useEffect(() => {
    const handleFindShortcut = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "f") {
        event.preventDefault();
        openSearch();
      }
    };
    window.addEventListener("keydown", handleFindShortcut);
    return () => window.removeEventListener("keydown", handleFindShortcut);
  }, [openSearch]);

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
      <header className="mr-2 flex min-h-9 items-center border-b px-6 py-1">
        {showSearch ? (
          <div className="flex min-w-0 flex-1 items-center justify-end gap-1.5">
            <div className="relative w-full max-w-sm">
              <Search className="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                ref={searchInputRef}
                type="search"
                value={searchQuery}
                onChange={(event) => {
                  setSearchQuery(event.target.value);
                  setActiveMatchIndex(0);
                }}
                onKeyDown={(event) => {
                  if (event.key === "Escape") closeSearch();
                  if (event.key === "Enter")
                    moveSearchResult(event.shiftKey ? -1 : 1);
                }}
                placeholder="Find in chat"
                aria-label="Find in chat"
                className="pr-16 pl-7 [&::-webkit-search-cancel-button]:hidden"
              />
              <span className="pointer-events-none absolute top-1/2 right-2 -translate-y-1/2 text-[10px] tabular-nums text-muted-foreground">
                {normalizedSearchQuery
                  ? matchingBlockKeys.length
                    ? `${safeActiveMatchIndex + 1} / ${matchingBlockKeys.length}`
                    : "No results"
                  : ""}
              </span>
            </div>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              disabled={!matchingBlockKeys.length}
              aria-label="Previous result"
              title="Previous result (Shift+Enter)"
              onClick={() => moveSearchResult(-1)}
            >
              <ChevronUp className="size-4" />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              disabled={!matchingBlockKeys.length}
              aria-label="Next result"
              title="Next result (Enter)"
              onClick={() => moveSearchResult(1)}
            >
              <ChevronDown className="size-4" />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label="Close chat search"
              title="Close chat search (Escape)"
              onClick={closeSearch}
            >
              <X className="size-4" />
            </Button>
          </div>
        ) : (
          <>
            <h1 className="min-w-0 truncate text-sm font-medium w-2xl">
              {live.detail?.chat.title ?? session.chat.title ?? "Loading chat"}
            </h1>
            <ChatStateIndicators
              loading={live.loading}
              isWorking={isWorking}
              contextRestoration={live.contextRestoration}
              runStatus={live.runStatus}
            />

						<div className="ml-auto">
            <Toggle
              size="sm"
              className="ml-auto"
              pressed={conversationView === "activity"}
              onPressedChange={(pressed) => {
                closeSearch();
                setConversationView(pressed ? "activity" : "chat");
              }}
              aria-label={
                conversationView === "activity"
                  ? "Show conversation"
                  : "Show activity"
              }
              title={
                conversationView === "activity"
                  ? "Show conversation"
                  : "Show activity"
              }
            >
              <RotateCwFadingClock className="size-4" />
              {/* <span className="hidden sm:inline">Activity</span> */}
            </Toggle>

            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label="Find in chat"
              title="Find in chat (Ctrl+F)"
              onClick={openSearch}
              className="ml-auto"
            >
              <Search className="size-4" />
            </Button>
						</div>
          </>
        )}
      </header>
      <Conversation className={conversationView === "activity" ? "hidden" : ""}>
        <ConversationContent className="mx-auto w-full max-w-3xl gap-2 py-8">
          {blocks.map((block) => {
            const matches = matchingBlockKeySet.has(block.key);
            const active = block.key === activeMatchKey;
            return (
              <div
                key={block.key}
                data-chat-search-key={block.key}
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
        <ActivityView
          key={chatId}
          events={activityEvents}
          runs={activityRuns}
          loading={activityLoading}
          error={activityError}
        />
      </div>
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
          <LiveChatFailureBanner
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
        <AppPromptInput
          workspacePath={workspacePath}
          selectedAgentId={agent?.id ?? ""}
          onAgentSelected={(next) => bindAgent(next)}
          onSendPrompt={submit}
          sessionConfig={live.sessionConfig}
          contextUsage={live.contextUsage}
          onSessionConfigChange={(configId, value) =>
            changeSessionConfig({ configId, value })
          }
          isWorking={isWorking}
          onStop={() => void stop()}
        />
      </div>
    </main>
  );
}
