import { useAtom } from "jotai";
import { pinnedChatIdsAtom } from "@/state/preferences";
import { useState } from "react";
import { Download, Plus, Settings } from "lucide-react";
import { notify } from "@/lib/notify";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { RecentChats } from "@/components/recent-chats";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarRail,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import type { Chat } from "@/types";

type ChatSidebarProps = {
  activeChatId: string | null;
  runningChatIds: ReadonlySet<string>;
  chats: Chat[];
  onNewChat: () => void;
  onSelectChat: (chatId: string) => void;
  onDeleteChat: (chatId: string) => Promise<void>;
  onUpdateChat: (chatId: string, changes: { title: string }) => Promise<void>;
  onOpenSettings: () => void;
  updateAvailable: boolean;
  onCheckForUpdates: () => void;
};

export function AppSidebar({
  activeChatId,
  runningChatIds,
  chats,
  onNewChat,
  onSelectChat,
  onDeleteChat,
  onUpdateChat,
  onOpenSettings,
  updateAvailable,
  onCheckForUpdates,
}: ChatSidebarProps) {
  const [pinnedChatIds, setPinnedChatIds] = useAtom(pinnedChatIdsAtom);
  const [chatToDelete, setChatToDelete] = useState<Chat | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [chatToRename, setChatToRename] = useState<Chat | null>(null);
  const [title, setTitle] = useState("");
  const [renaming, setRenaming] = useState(false);

  const updateChat = async (chat: Chat, changes: { title: string }) => {
    try {
      await onUpdateChat(chat.id, changes);
      return true;
    } catch (error) {
      notify(
        JSON.stringify(error),
        // error instanceof Error ? error.message : "Unable to update this chat.",
        "error",
      );
      return false;
    }
  };

  const renameChat = async () => {
    if (!chatToRename || !title.trim() || renaming) return;
    setRenaming(true);
    try {
      if (await updateChat(chatToRename, { title: title.trim() }))
        setChatToRename(null);
    } finally {
      setRenaming(false);
    }
  };

  const deleteChat = async () => {
    if (!chatToDelete) return;
    setDeleting(true);
    try {
      await onDeleteChat(chatToDelete.id);
      setPinnedChatIds((ids) => ids.filter((id) => id !== chatToDelete.id));
      setChatToDelete(null);
    } catch (error) {
      console.error("Failed to delete chat:", error);
      notify(
        error instanceof Error ? error.message : "Unable to delete this chat.",
        "error",
      );
    } finally {
      setDeleting(false);
    }
  };

  return (
    <>
      <Sidebar
        variant="floating"
        collapsible="icon"
        className="inset-y-auto! top-9! bottom-0! h-auto!  select-none"
      >
        <SidebarHeader className="gap-0 border-b border-sidebar-border">
          <div className="flex items-center gap-1">
            <SidebarMenu className="min-w-0 flex-1 group-data-[collapsible=icon]:hidden">
              <SidebarMenuItem>
                <SidebarMenuButton
                  isActive={false}
                  tooltip="New chat"
                  onClick={onNewChat}
                >
                  <Plus />
                  <span>New chat</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
            <SidebarTrigger className="shrink-0 text-card-foreground/80" />
          </div>
        </SidebarHeader>
        <SidebarContent className="px-2 py-2">
          <RecentChats
            activeChatId={activeChatId}
            runningChatIds={runningChatIds}
            chats={chats}
            pinnedChatIds={pinnedChatIds}
            onSelectChat={onSelectChat}
            onDeleteChat={setChatToDelete}
            onRenameChat={(chat) => {
              setChatToRename(chat);
              setTitle(chat.title);
            }}
            onTogglePin={(chat) => {
              setPinnedChatIds((ids) =>
                ids.includes(chat.id)
                  ? ids.filter((id) => id !== chat.id)
                  : [...ids, chat.id],
              );
            }}
          />
        </SidebarContent>
        <SidebarFooter className="gap-1 border-t border-sidebar-border p-2">
          {/* <div className="flex min-w-0 items-center gap-2 px-2 py-1 text-[11px] leading-5 text-muted-foreground group-data-[collapsible=icon]:hidden">
            <Bot className="size-3.5 shrink-0" />
            <span className="truncate" title={workspacePath}>
              {workspacePath}
            </span>
          </div> */}
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton
                onClick={onCheckForUpdates}
                tooltip={
                  updateAvailable ? "Update available" : "Check for updates"
                }
              >
                <Download />
                <span>
                  {updateAvailable ? "Update available" : "Check for updates"}
                </span>
              </SidebarMenuButton>
            </SidebarMenuItem>
            <SidebarMenuItem>
              <SidebarMenuButton onClick={onOpenSettings} tooltip="Settings">
                <Settings />
                <span>Settings</span>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarFooter>
        <SidebarRail />
      </Sidebar>
      <Dialog
        open={chatToRename !== null}
        onOpenChange={(open) => {
          if (!open && !renaming) setChatToRename(null);
        }}
      >
        <DialogContent showCloseButton={!renaming}>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void renameChat();
            }}
            className="grid gap-4"
          >
            <DialogHeader>
              <DialogTitle>Rename chat</DialogTitle>
              <DialogDescription>
                Choose a new name for this chat.
              </DialogDescription>
            </DialogHeader>
            <Input
              aria-label="Chat name"
              value={title}
              onChange={(event) => setTitle(event.target.value)}
              disabled={renaming}
              onFocus={(event) => event.target.select()}
              autoFocus
            />
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                disabled={renaming}
                onClick={() => setChatToRename(null)}
              >
                Cancel
              </Button>
              <Button type="submit" disabled={renaming || !title.trim()}>
                {renaming ? "Saving…" : "Save"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      <AlertDialog
        open={chatToDelete !== null}
        onOpenChange={(open) => {
          if (!open && !deleting) setChatToDelete(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete chat?</AlertDialogTitle>
            <AlertDialogDescription>
              “{chatToDelete?.title}” and its complete message history will be
              permanently deleted.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleting}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              disabled={deleting}
              onClick={(event) => {
                event.preventDefault();
                void deleteChat();
              }}
            >
              {deleting ? "Deleting…" : "Delete"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
