import { useState } from "react";
import { Bot, Plus, Settings, Trash2 } from "lucide-react";
import { notify } from "@/lib/notify";
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
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarRail,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import type { Chat } from "@/types";

type ChatSidebarProps = {
  activeChatId: string | null;
  chats: Chat[];
  onNewChat: () => void;
  onSelectChat: (chatId: string) => void;
  onDeleteChat: (chatId: string) => Promise<void>;
  onOpenSettings: () => void;
};

export function AppSidebar({
  activeChatId,
  chats,
  onNewChat,
  onSelectChat,
  onDeleteChat,
  onOpenSettings,
}: ChatSidebarProps) {
  const [chatToDelete, setChatToDelete] = useState<Chat | null>(null);
  const [deleting, setDeleting] = useState(false);

  const deleteChat = async () => {
    if (!chatToDelete) return;
    setDeleting(true);
    try {
      await onDeleteChat(chatToDelete.id);
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
          <div className="flex items-center gap-1 pl-1.5">
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
            <SidebarTrigger className="shrink-0" />
          </div>
        </SidebarHeader>
        <SidebarContent className="px-2 py-2">
          <SidebarGroup className="p-0">
            <SidebarGroupLabel>Recent chats</SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu>
                {chats.map((chat) => (
                  <SidebarMenuItem key={chat.id}>
                    <SidebarMenuButton
                      isActive={chat.id === activeChatId}
                      onClick={() => onSelectChat(chat.id)}
                      tooltip={chat.title}
                      className="pr-8 hover:bg-sidebar-accent/50"
                    >
                      <span>{chat.title}</span>
                    </SidebarMenuButton>
                    <SidebarMenuAction
                      showOnHover
                      aria-label={`Delete ${chat.title}`}
                      title="Delete chat"
                      onClick={(event) => {
                        event.stopPropagation();
                        setChatToDelete(chat);
                      }}
                      className="hover:text-destructive"
                    >
                      <Trash2 />
                    </SidebarMenuAction>
                  </SidebarMenuItem>
                ))}
                {!chats.length && (
                  <p className="px-2 py-3 text-xs text-muted-foreground group-data-[collapsible=icon]:hidden">
                    No chats yet.
                  </p>
                )}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
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
              <SidebarMenuButton onClick={onOpenSettings} tooltip="Settings">
                <Settings />
                <span>Settings</span>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarFooter>
        <SidebarRail />
      </Sidebar>
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
