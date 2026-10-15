import { useEffect, useRef, useState } from "react";
import {
  LoaderCircle,
  Pencil,
  Pin,
  PinOff,
  Search,
  Trash2,
  X,
} from "lucide-react";
import {
  SidebarGroup,
  SidebarGroupContent,
  SidebarMenu,
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";
import {
  ContextMenu,
  ContextMenuTrigger,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
} from "@/components/ui/context-menu";
import type { Chat } from "@/types";

const MINUTE = 60 * 1000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const WEEK = 7 * DAY;
const MONTH = 30 * DAY;
const YEAR = 365 * DAY;

function formatRelativeTime(timestamp: string, now: number) {
  const elapsed = Math.max(0, now - new Date(timestamp).getTime());

  if (!Number.isFinite(elapsed)) return "";
  if (elapsed < MINUTE) return "now";
  if (elapsed < HOUR) return `${Math.floor(elapsed / MINUTE)}m`;
  if (elapsed < DAY) return `${Math.floor(elapsed / HOUR)}h`;
  if (elapsed < WEEK) return `${Math.floor(elapsed / DAY)}d`;
  if (elapsed < MONTH) return `${Math.floor(elapsed / WEEK)}w`;
  if (elapsed < YEAR) return `${Math.floor(elapsed / MONTH)}mo`;
  return `${Math.floor(elapsed / YEAR)}y`;
}

function formatFullDate(timestamp: string) {
  const date = new Date(timestamp);
  if (!Number.isFinite(date.getTime())) return undefined;

  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

type RecentChatsProps = {
  activeChatId: string | null;
  runningChatIds: ReadonlySet<string>;
  chats: Chat[];
  pinnedChatIds: readonly string[];
  onSelectChat: (chatId: string) => void;
  onDeleteChat: (chat: Chat) => void;
  onRenameChat: (chat: Chat) => void;
  onTogglePin: (chat: Chat) => void;
};

export function RecentChats({
  activeChatId,
  runningChatIds,
  chats,
  pinnedChatIds,
  onSelectChat,
  onDeleteChat,
  onRenameChat,
  onTogglePin,
}: RecentChatsProps) {
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [now, setNow] = useState(() => Date.now());
  const searchInputRef = useRef<HTMLInputElement>(null);

  const normalizedQuery = searchQuery.trim().toLocaleLowerCase();
  const filteredChats = normalizedQuery
    ? chats.filter((chat) =>
        chat.title.toLocaleLowerCase().includes(normalizedQuery),
      )
    : chats;

  useEffect(() => {
    if (isSearchOpen) searchInputRef.current?.focus();
  }, [isSearchOpen]);

  useEffect(() => {
    const interval = window.setInterval(() => setNow(Date.now()), MINUTE);
    return () => window.clearInterval(interval);
  }, []);

  const closeSearch = () => {
    setIsSearchOpen(false);
    setSearchQuery("");
  };

  const renderChat = (chat: Chat) => (
    <ContextMenu key={chat.id}>
      <ContextMenuTrigger asChild>
        <SidebarMenuItem>
          <SidebarMenuButton
            isActive={chat.id === activeChatId}
            onClick={() => onSelectChat(chat.id)}
            tooltip={chat.title}
            className="pr-8 hover:bg-sidebar-accent/50"
          >
            <span className="min-w-0 flex-1 truncate">{chat.title}</span>
          </SidebarMenuButton>
          {runningChatIds.has(chat.id) ? (
            <LoaderCircle
              aria-label="Running"
              className="pointer-events-none absolute top-1/2 right-2 size-3.5 -translate-y-1/2 animate-spin text-sidebar-foreground/55 transition-opacity group-focus-within/menu-item:opacity-0 group-hover/menu-item:opacity-0 group-data-[collapsible=icon]:hidden! motion-reduce:animate-none motion-reduce:transition-none"
            />
          ) : (
            <time
              dateTime={chat.updated_at}
              title={formatFullDate(chat.updated_at)}
              className="pointer-events-none absolute top-1/2 right-1 hidden w-5 -translate-y-1/2 text-center text-[10px] leading-none font-normal tabular-nums text-sidebar-foreground/45 transition-opacity group-focus-within/menu-item:opacity-0 group-hover/menu-item:opacity-0 group-data-[collapsible=icon]:hidden! md:block motion-reduce:transition-none"
            >
              {formatRelativeTime(chat.updated_at, now)}
            </time>
          )}
          <SidebarMenuAction
            showOnHover
            aria-label={`Delete ${chat.title}`}
            title="Delete chat"
            onClick={(event) => {
              event.stopPropagation();
              onDeleteChat(chat);
            }}
            className="hover:text-destructive"
          >
            <Trash2 />
          </SidebarMenuAction>
        </SidebarMenuItem>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem onSelect={() => onTogglePin(chat)}>
          {pinnedChatIds.includes(chat.id) ? <PinOff /> : <Pin />}
          {pinnedChatIds.includes(chat.id) ? "Unpin chat" : "Pin chat"}
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => onRenameChat(chat)}>
          <Pencil /> Rename chat
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem
          variant="destructive"
          onSelect={() => onDeleteChat(chat)}
        >
          <Trash2 /> Delete chat
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );

  const pinnedChats = filteredChats.filter((chat) =>
    pinnedChatIds.includes(chat.id),
  );

  return (
    <SidebarGroup className="p-0">
      {pinnedChats.length > 0 && (
        <div className="mb-3">
          <div className="flex h-8 items-center gap-1.5 pl-2 text-xs text-sidebar-foreground/70 group-data-[collapsible=icon]:hidden">
            <Pin className="size-3" /> Pinned chats
          </div>
          <SidebarGroupContent>
            <SidebarMenu>{pinnedChats.map(renderChat)}</SidebarMenu>
          </SidebarGroupContent>
        </div>
      )}
      <div className="relative h-8 group-data-[collapsible=icon]:-mt-8 group-data-[collapsible=icon]:opacity-0">
        <div
          className={`absolute inset-0 flex items-center justify-between pl-2 transition-[opacity,transform] duration-200 ease-out motion-reduce:transition-none ${
            isSearchOpen
              ? "pointer-events-none translate-x-1 opacity-0"
              : "translate-x-0 opacity-100"
          }`}
        >
          <span className="text-xs text-sidebar-foreground/70">
            Recent chats
          </span>
          <button
            type="button"
            aria-label="Search recent chats"
            title="Search recent chats"
            onClick={() => setIsSearchOpen(true)}
            className="flex size-7 items-center justify-center rounded text-sidebar-foreground/70 outline-none transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:ring-2 focus-visible:ring-sidebar-ring"
          >
            <Search className="size-3.5" />
          </button>
        </div>
        <div
          className={`absolute top-0 right-0 flex h-6 origin-right items-center overflow-hidden rounded bg-sidebar ring-1 ring-sidebar-border/50 transition-[width,opacity] duration-200 ease-out motion-reduce:transition-none ${
            isSearchOpen
              ? "w-full opacity-100"
              : "pointer-events-none w-7 opacity-0"
          }`}
        >
          <Search
            aria-hidden="true"
            className="ml-1 size-3.5 shrink-0 text-muted-foreground"
          />
          <input
            ref={searchInputRef}
            type="search"
            value={searchQuery}
            onChange={(event) => setSearchQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") closeSearch();
            }}
            aria-label="Filter recent chats"
            placeholder="Search"
            className="min-w-0 flex-1 bg-transparent px-2 text-xs text-sidebar-foreground outline-none placeholder:text-muted-foreground [&::-webkit-search-cancel-button]:hidden"
            tabIndex={isSearchOpen ? 0 : -1}
          />
          <button
            type="button"
            aria-label="Close search"
            title="Close search"
            onClick={closeSearch}
            tabIndex={isSearchOpen ? 0 : -1}
            className="flex size-7 shrink-0 items-center justify-center rounded text-muted-foreground outline-none transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
          >
            <X className="size-3.5" />
          </button>
        </div>
      </div>
      <SidebarGroupContent>
        <SidebarMenu>
          {filteredChats
            .filter((chat) => !pinnedChatIds.includes(chat.id))
            .map(renderChat)}
          {!filteredChats.length && (
            <p className="px-2 py-3 text-xs text-muted-foreground group-data-[collapsible=icon]:hidden">
              {chats.length ? "No chats found." : "No chats yet."}
            </p>
          )}
        </SidebarMenu>
      </SidebarGroupContent>
    </SidebarGroup>
  );
}
