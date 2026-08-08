import { AgentDefinition, SessionSummary } from "@/types";
import { Bot, FolderOpen, Square, Sparkles } from "lucide-react";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";

// this is just an incredible terribly badly designed header
export function SessionHeader({
  active,
  agent,
  isWorking,
  onCancel,
}: {
  active: SessionSummary;
  agent?: AgentDefinition;
  isWorking: boolean;
  onCancel: () => void;
}) {
  const agentName = agent?.name ?? active.agentId;
  return (
    <header className="flex min-h-20 items-center justify-between gap-4 border-b border-border bg-background/90 px-8 py-3 backdrop-blur-sm">
      <div className="flex min-w-0 items-center gap-3">
        <div className="grid size-9 shrink-0 place-items-center rounded-lg border border-border bg-card shadow-sm">
          <Bot className="size-4 text-primary" />
        </div>
        <div className="min-w-0">
          <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <FolderOpen className="size-3 shrink-0" />
            <span className="truncate">{active.workspacePath}</span>
          </div>
          <h1 className="mt-0.5 truncate text-sm font-semibold tracking-tight">
            {agentName}
          </h1>
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-3">
        <Badge variant="secondary" className="gap-1.5 rounded-full px-2.5 py-1">
          <span
            className={`size-1.5 rounded-full ${isWorking ? "animate-pulse bg-primary" : "bg-emerald-500"}`}
          />
          {isWorking ? "Working" : "Ready"}
        </Badge>
        {isWorking && (
          <Button size="sm" variant="outline" onClick={onCancel}>
            <Square data-icon="inline-start" />
            Stop
          </Button>
        )}
        <Sparkles className="hidden size-4 text-muted-foreground sm:block" />
      </div>
    </header>
  );
}
