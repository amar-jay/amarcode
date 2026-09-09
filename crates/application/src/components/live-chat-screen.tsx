import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useAtomValue, useSetAtom } from "jotai";
import {
  CircleAlert,
  ChevronDown,
  ChevronUp,
  KeyRound,
  LoaderCircle,
  Search,
  Timer,
  Unplug,
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
import type { AgentFailureKind, PromptAttachment } from "@/types";
import { daemonApi } from "@/api";
import { notify } from "@/lib/notify";
import {
  Alert,
  AlertAction,
  AlertDescription,
  AlertTitle,
} from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import AppPromptInput from "./main-prompt-input";
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

function failureBannerCopy(kind: AgentFailureKind | null): {
  title: string;
  Icon: typeof CircleAlert;
} {
  switch (kind) {
    case "auth_required":
      return { title: "Sign in required", Icon: KeyRound };
    case "unavailable":
      return { title: "Agent runtime unavailable", Icon: CircleAlert };
    case "adapter_exited":
      return { title: "Agent adapter stopped", Icon: Unplug };
    case "timeout":
      return { title: "Agent timed out", Icon: Timer };
    default:
      return { title: "Agent turn failed", Icon: CircleAlert };
  }
}

function LiveChatFailureBanner({
  error,
  errorKind,
  authRequired,
  canDeleteChat,
  onSignedIn,
  onDeleteChat,
}: {
  error: string;
  errorKind: AgentFailureKind | null;
  authRequired: {
    agentId: string;
    runId: string | null;
    methods: unknown;
  } | null;
  canDeleteChat: boolean;
  onSignedIn: () => void;
  onDeleteChat: () => Promise<void>;
}) {
  const [signingIn, setSigningIn] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const { title, Icon } = failureBannerCopy(errorKind);
  const showSignIn = errorKind === "auth_required" && Boolean(authRequired);
  const authMethods = Array.isArray(authRequired?.methods)
    ? authRequired.methods.filter(
        (method): method is { id: string; name?: string } =>
          typeof method === "object" &&
          method !== null &&
          !Array.isArray(method) &&
          typeof method.id === "string",
      )
    : [];
  const preferredAuthMethod = authMethods[0];
  const busy = signingIn || deleting;

  return (
    <Alert
      variant="destructive"
      className="mx-auto mb-3 max-w-3xl border-destructive/30 bg-destructive/5 px-3 py-3 has-data-[slot=alert-action]:pr-28"
      aria-live="assertive"
    >
      <Icon />
      <AlertTitle className="text-sm">{title}</AlertTitle>
      <AlertDescription className="text-sm text-destructive/90">
        {error}
      </AlertDescription>
      {(showSignIn || canDeleteChat) && (
        <AlertAction className="top-2.5 right-2.5 flex items-center gap-2">
          {showSignIn && (
            <Button
              size="sm"
              variant="outline"
              className="border-destructive/40 text-destructive hover:bg-destructive/10"
              disabled={busy}
              onClick={() => {
                if (!authRequired) return;
                setSigningIn(true);
                void daemonApi
                  .authenticateAgent(
                    authRequired.agentId,
                    preferredAuthMethod?.id,
                  )
                  .then(() => {
                    notify("Sign-in completed", "success");
                    onSignedIn();
                  })
                  .catch((cause: unknown) => {
                    notify(
                      cause instanceof Error ? cause.message : "Sign-in failed",
                      "error",
                    );
                  })
                  .finally(() => setSigningIn(false));
              }}
            >
              {signingIn ? (
                <span className="inline-flex items-center gap-1.5">
                  <LoaderCircle className="size-3.5 animate-spin" />
                  Signing in…
                </span>
              ) : (
                (preferredAuthMethod?.name ?? "Sign in")
              )}
            </Button>
          )}
          {canDeleteChat && (
            <Button
              size="sm"
              variant="outline"
              className="border-destructive/40 text-destructive hover:bg-destructive/10"
              disabled={busy}
              onClick={() => {
                setDeleting(true);
                void onDeleteChat()
                  .catch((cause: unknown) => {
                    notify(
                      cause instanceof Error
                        ? cause.message
                        : "Failed to delete chat",
                      "error",
                    );
                  })
                  .finally(() => setDeleting(false));
              }}
            >
              {deleting ? (
                <span className="inline-flex items-center gap-1.5">
                  <LoaderCircle className="size-3.5 animate-spin" />
                  Deleting…
                </span>
              ) : (
                "Delete chat"
              )}
            </Button>
          )}
        </AlertAction>
      )}
    </Alert>
  );
}

/**
 * Live conversation surface. State lives in `liveChatAtom` (+ navigation /
 * agent atoms); this component is render + wire-up only.
 */
export function LiveChatScreen() {
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchChatId, setSearchChatId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [activeMatchIndex, setActiveMatchIndex] = useState(0);
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
  const showSearch = searchOpen && searchChatId === chatId;

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
              <X className="size-4"/>
            </Button>
          </div>
        ) : (
          <>
            <h1 className="min-w-0 truncate text-sm font-medium">
              {live.detail?.chat.title ?? session.chat.title ?? "Loading chat"}
            </h1>
            {live.loading && (
              <LoaderCircle className="ml-2 size-4 animate-spin text-muted-foreground" />
            )}
            {isWorking && (
              <span className="ml-3 text-xs text-muted-foreground">
                Working…
              </span>
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
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              className="ml-auto"
              aria-label="Find in chat"
              title="Find in chat (Ctrl+F)"
              onClick={openSearch}
            >
              <Search className="size-4" />
            </Button>
          </>
        )}
      </header>
      <Conversation>
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
