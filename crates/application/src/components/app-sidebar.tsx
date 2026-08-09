import { SessionSummary } from "@/types";
import {  Plus, FolderOpen,  Cog } from "lucide-react";
import { SidebarHeader, SidebarTrigger, SidebarMenu, SidebarMenuItem, SidebarMenuButton, SidebarContent, SidebarGroup, SidebarGroupLabel, SidebarGroupContent, SidebarFooter, SidebarRail, Sidebar } from "./ui/sidebar";

export function AppSidebar({
  active,
  sessions,
  onNewSession,
  onRestore,
  onSettings,
}: {
  active?: SessionSummary;
  sessions: SessionSummary[];
  onNewSession: () => void;
  onRestore: (session: SessionSummary) => void;
  onSettings: () => void;
}) {
  return (
    <Sidebar
      variant="floating"
      collapsible="icon"
      className="inset-y-auto! top-9! bottom-0! h-auto!"
    >
      <SidebarHeader className="border-b border-sidebar-border">
        <div className="flex items-center gap-1 px-2 py-1">
          <SidebarTrigger />
          <span className="font-heading text-sm font-semibold tracking-tight group-data-[collapsible=icon]:hidden">
            AMARCODE
          </span>
        </div>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              isActive={!active}
              tooltip="New session"
              onClick={onNewSession}
            >
              <Plus />
              <span>New session</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>
      <SidebarContent className="px-2 py-2">
        <SidebarGroup className="p-0">
          <SidebarGroupLabel>Recent sessions</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {sessions.length === 0 ? (
                <p className="px-2 py-3 text-xs text-muted-foreground group-data-[collapsible=icon]:hidden">
                  No saved sessions yet.
                </p>
              ) : (
                sessions.map((session) => (
                  <SidebarMenuItem key={session.id}>
                    <SidebarMenuButton
                      isActive={active?.id === session.id}
                      tooltip={
                        session.workspacePath.split("/").pop() ??
                        session.workspacePath
                      }
                      onClick={() => onRestore(session)}
                    >
                      <FolderOpen />
                      <span>{session.workspacePath.split("/").pop()}</span>
                      {/* <Badge
                        variant="secondary"
                        className="ml-auto text-[10px] group-data-[collapsible=icon]:hidden"
                      >
                        {session.status}
                      </Badge> */}
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                ))
              )}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter className="border-t border-sidebar-border">
        <div className="px-2 py-1 text-[11px] leading-5 text-muted-foreground group-data-[collapsible=icon]:hidden">
          Local-only ACP client.
          <br />
          Secrets stay in the OS keychain.
        </div>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton tooltip="Settings" onClick={onSettings}>
              <Cog />
              <span>Settings</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  );
}
