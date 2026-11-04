import { createPortal } from "react-dom";
import type { RefObject } from "react";
import { useAtomValue } from "jotai";
import { chatHeaderHostAtom } from "@/state/chat-header";
import {
  ChevronDown,
  ChevronUp,
  RotateCwFadingClock,
  Search,
  X,
} from "lucide-react";
import { TooltipButton as Button } from "@/components/ui/tooltip-button";
import { Input } from "@/components/ui/input";
import { Toggle } from "@/components/ui/toggle";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

export type ConversationView = "chat" | "activity";

type LiveChatHeaderProps = {
  title: string;
  conversationView: ConversationView;
  onConversationViewChange: (view: ConversationView) => void;
  showSearch: boolean;
  searchQuery: string;
  onSearchQueryChange: (query: string) => void;
  searchInputRef: RefObject<HTMLInputElement | null>;
  hasOlderMessages: boolean;
  matchCount: number;
  activeMatchIndex: number;
  openSearch: () => void;
  closeSearch: () => void;
  moveSearchResult: (direction: 1 | -1) => void;
};

/** Conversation title and search controls, rendered into the shared title bar. */
export function LiveChatHeader({
  title,
  conversationView,
  onConversationViewChange,
  showSearch,
  searchQuery,
  onSearchQueryChange,
  searchInputRef,
  hasOlderMessages,
  matchCount,
  activeMatchIndex,
  openSearch,
  closeSearch,
  moveSearchResult,
}: LiveChatHeaderProps) {
  const headerHost = useAtomValue(chatHeaderHostAtom);
  if (!headerHost) return null;
  return createPortal(
    <header className="flex h-9 min-w-0 flex-1 items-center gap-2 px-2">
      {showSearch ? (
        <div className="flex min-w-0 flex-1 items-center justify-end gap-1.5">
          <div className="relative w-full max-w-sm">
            <Search className="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              ref={searchInputRef}
              type="search"
              value={searchQuery}
              onChange={(event) => onSearchQueryChange(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Escape") closeSearch();
                if (event.key === "Enter")
                  moveSearchResult(event.shiftKey ? -1 : 1);
              }}
              placeholder={
                hasOlderMessages ? "Find in loaded messages" : "Find in chat"
              }
              aria-label="Find in chat"
              className="pr-16 pl-7 [&::-webkit-search-cancel-button]:hidden"
            />
            <span className="pointer-events-none absolute top-1/2 right-2 -translate-y-1/2 text-[10px] tabular-nums text-muted-foreground">
              {searchQuery.trim()
                ? matchCount
                  ? `${activeMatchIndex + 1} / ${matchCount}`
                  : "No results"
                : ""}
            </span>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            disabled={!matchCount}
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
            disabled={!matchCount}
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
          <h1
            data-tauri-drag-region
            className="min-w-0 flex-1 truncate text-xs font-medium text-muted-foreground"
          >
            {title}
          </h1>
          <div className="ml-auto flex">
            <Tooltip>
              <TooltipTrigger asChild>
                <Toggle
                  size="sm"
                  pressed={conversationView === "activity"}
                  onPressedChange={(pressed) => {
                    onConversationViewChange(pressed ? "activity" : "chat");
                  }}
                  aria-label={
                    conversationView === "activity"
                      ? "Show conversation"
                      : "Show activity"
                  }
                >
                  <RotateCwFadingClock className="size-4" />
                </Toggle>
              </TooltipTrigger>
              <TooltipContent side="bottom">
                {conversationView === "activity"
                  ? "Show conversation"
                  : "Show activity"}
              </TooltipContent>
            </Tooltip>

            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label="Find in chat"
              title="Find in chat (Ctrl+F)"
              onClick={openSearch}
            >
              <Search className="size-4" />
            </Button>
          </div>
        </>
      )}
    </header>,
    headerHost,
  );
}
