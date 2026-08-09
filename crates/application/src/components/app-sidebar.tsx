import { Bot, MessageSquare, Plus } from "lucide-react";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
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
  workspacePath: string;
  chats: Chat[];
  onNewChat: () => void;
  onSelectChat: (chatId: string) => void;
};

export function AppSidebar({
  activeChatId,
  workspacePath,
  chats,
  onNewChat,
  onSelectChat,
}: ChatSidebarProps) {
  return (
    <Sidebar
      variant="floating"
      collapsible="icon"
      className="inset-y-auto! top-9! bottom-0! h-auto!"
    >
      <SidebarHeader className="gap-0 border-b border-sidebar-border p-0">
        <div className="flex items-center gap-1 pr-3 py-1 ml-auto">
          <SidebarTrigger />
        </div>
        <SidebarMenu className="px-2 pb-2">
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
                  >
                    <MessageSquare className="size-4" />
                    <span>{chat.title}</span>
                  </SidebarMenuButton>
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
        <div className="flex min-w-0 items-center gap-2 px-2 py-1 text-[11px] leading-5 text-muted-foreground group-data-[collapsible=icon]:hidden">
          <Bot className="size-3.5 shrink-0" />
          <span className="truncate" title={workspacePath}>
            {workspacePath}
          </span>
        </div>
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  );
}
