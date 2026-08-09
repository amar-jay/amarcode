import { useCallback, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { AppSidebar } from "@/components/app-sidebar";
import { DaemonConnectionDialog } from "@/components/daemon-connection-dialog";
import { NewSessionCard } from "@/components/new-session-card";
import { WorkbenchPromptInput } from "@/components/prompt-input";
import { SessionHeader } from "@/components/session-header";
import { SessionTimeline } from "@/components/session/session-timeline";
import { SettingsDialog } from "@/components/settings-dialog";
import { TopBar } from "@/components/top-bar";
import { SidebarInset } from "@/components/ui/sidebar";
import { Toaster } from "@/components/ui/sonner";
import { useAgentCatalog } from "@/hooks/use-agent-catalog";
import { useDaemonConnection } from "@/hooks/use-daemon-connection";
import { useSessionController } from "@/hooks/use-session-controller";
import { useTheme } from "@/hooks/use-theme";
import { useWorkspacePicker } from "@/hooks/use-workspace-picker";
import type { AgentDefinition } from "@/types";

export default function App() {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const { theme, setTheme } = useTheme();
  const workspace = useWorkspacePicker();
  const agents = useAgentCatalog();
  const sessions = useSessionController(agents.agents);
  const onDaemonReady = useCallback(
    ({
      agents: loadedAgents,
      sessions: loadedSessions,
    }: {
      agents: AgentDefinition[];
      sessions: any[];
    }) => {
			if (!loadedAgents) return
      agents.loadAgents(loadedAgents);
      sessions.setSessions(loadedSessions);
    },
    [agents.loadAgents, sessions.setSessions],
  );
  const daemon = useDaemonConnection(onDaemonReady);
  const error = daemon.error || agents.error || sessions.error;

  return (
    <>
      <TopBar />
      <AppSidebar
        active={sessions.activeSession}
        sessions={sessions.sessions}
        onNewSession={() => sessions.setActiveSession(undefined)}
        onRestore={sessions.restoreSession}
        onSettings={() => setSettingsOpen(true)}
      />
      <SidebarInset className="min-w-0 pt-9">
        {!sessions.activeSession ? (
          <NewSessionCard
            agent={agents.selectedAgent}
            workspace={workspace.workspace}
            agents={agents.agents}
            selectedAgent={agents.selectedAgentId}
            showAgentForm={agents.showAgentForm}
            agentForm={agents.agentForm}
            error={error}
            isLaunching={sessions.isLaunching}
            onChooseWorkspace={() => void workspace.chooseWorkspace()}
            onSelectAgent={agents.setSelectedAgentId}
            onShowAgentForm={() => agents.setShowAgentForm((shown) => !shown)}
            onAgentFormChange={agents.setAgentForm}
            onAddAgent={agents.addAgent}
            onStart={() =>
              void sessions.startSession(
                workspace.workspace,
                agents.selectedAgent,
              )
            }
          />
        ) : (
          <>
            <SessionHeader
              active={sessions.activeSession}
              agent={sessions.activeAgent}
              isWorking={sessions.isPromptWorking}
              onCancel={() => void sessions.stopPrompt()}
            />
            <div className="min-h-0 flex-1">
              <SessionTimeline
                events={sessions.events}
                isWorking={sessions.isPromptWorking}
                onRespond={sessions.respondToRequest}
              />
            </div>
            <div className="sticky bottom-0 z-20 shrink-0 border-t border-border bg-background/95 px-8 py-4 backdrop-blur-sm">
              <WorkbenchPromptInput
                agent={sessions.activeAgent}
                workspacePath={sessions.activeSession.workspacePath}
                isWorking={sessions.isPromptWorking}
                onStop={() => void sessions.stopPrompt()}
                onSubmit={sessions.submitPrompt}
              />
              {error && (
                <p className="mx-auto mt-2 max-w-3xl text-xs text-destructive">
                  {error}
                </p>
              )}
            </div>
          </>
        )}
      </SidebarInset>
      <SettingsDialog
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        theme={theme}
        onThemeChange={setTheme}
      />
      {daemon.status !== "connected" && (
        <DaemonConnectionDialog
          status={daemon.status}
          onRetry={() => void daemon.connect()}
          onCloseApplication={() => void getCurrentWindow().close()}
        />
      )}
      <Toaster
        position="bottom-right"
        closeButton
        theme={theme}
        className="!z-[100] pointer-events-auto"
      />
    </>
  );
}
