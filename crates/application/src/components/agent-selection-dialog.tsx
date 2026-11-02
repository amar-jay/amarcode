"use client";

import { AgentSetupButton } from "@/components/agent-setup-button";
import { AgentInfo } from "@/types";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { useState } from "react";
import { useAtomValue, useSetAtom } from "jotai";
import { PromptInputButton } from "@/components/ai-elements/prompt-input";
import { Download, LoaderCircle } from "lucide-react";
import { AgentLogo } from "@/components/agent-logo";
import {
  agentSetupTargetAtom,
  installAgentAtom,
  installingAgentIdsAtom,
} from "@/state/agents";
import { notify } from "@/lib/notify";

function stripAcpSuffix(value: string): string {
  return value.replace(/\s*\bACP\s*$/i, "");
}

export function AgentSelectionDialog({
  setSelectedAgent,
  selectedAgent,
  agents,
}: {
  setSelectedAgent: (agentId: string) => void;
  selectedAgent: string;
  agents: AgentInfo[];
}) {
  const [open, setOpen] = useState(false);
  const [installingId, setInstallingId] = useState<string | null>(null);
  const installAgent = useSetAtom(installAgentAtom);
  const openSetup = useSetAtom(agentSetupTargetAtom);
  const installingAgentIds = useAtomValue(installingAgentIdsAtom);

  const selectedName =
    agents.find((agent) => agent.id === selectedAgent)?.name ?? "Agent";
  const selectedIcon = agents.find((agent) => agent.id === selectedAgent)?.icon;

  const handleInstall = async (agent: AgentInfo) => {
    if (installingId) return;
    setInstallingId(agent.id);
    try {
      const result = await installAgent(agent.id);
      openSetup({ id: result.agent.id, name: result.agent.name });
      setSelectedAgent(result.agent.id);
      setOpen(false);
      if (result.runtime_status === "ready") {
        notify(`${stripAcpSuffix(result.agent.name)} installed`, "success");
      } else if (result.runtime_status === "auth_required") {
        notify(
          `${stripAcpSuffix(result.agent.name)} installed: sign in required`,
          "info",
        );
      } else {
        notify(
          result.runtime_message ??
            `${stripAcpSuffix(result.agent.name)} installed but is not ready`,
          "error",
        );
      }
    } catch (error) {
      console.error("Failed to install agent:", error);
      notify(
        error instanceof Error
          ? error.message
          : `Failed to install ${stripAcpSuffix(agent.name)}.`,
        "error",
      );
    } finally {
      setInstallingId(null);
    }
  };

  const renderAgent = (agent: AgentInfo) => {
    const installing =
      installingId === agent.id || installingAgentIds.includes(agent.id);
    return (
      <CommandItem
        key={agent.id}
        value={stripAcpSuffix(agent.name)}
        disabled={installingId !== null || installing}
        data-checked={selectedAgent === agent.id && agent.available}
        title={agent.unavailable_reason ?? undefined}
        aria-label={`${agent.available ? "Select" : "Install"} ${stripAcpSuffix(agent.name)}`}
        onSelect={() => {
          if (installingId || installing) return;
          if (!agent.available) {
            void handleInstall(agent);
            return;
          }
          setSelectedAgent(agent.id);
          setOpen(false);
        }}
      >
        <AgentLogo icon={agent.icon} />
        <span className="flex min-w-0 flex-1 items-center gap-2">
          <span className="truncate">{stripAcpSuffix(agent.name)}</span>
          {agent.is_builtin && (
            <span className="shrink-0 text-[10px] font-normal text-muted-foreground">
              Built-in
            </span>
          )}
        </span>
        {agent.available && (
          <AgentSetupButton agent={agent} onOpen={() => setOpen(false)} />
        )}
        {!agent.available && (
          <span className="ml-auto flex shrink-0 items-center gap-1.5 text-[11px] text-primary">
            {installing ? (
              <LoaderCircle className="size-3.5 animate-spin" />
            ) : (
              <Download className="size-3.5" />
            )}
            {installing ? "Installing" : "Install"}
          </span>
        )}
      </CommandItem>
    );
  };

  const availableAgents = agents.filter((agent) => agent.available);
  const installableAgents = agents.filter((agent) => !agent.available);

  return (
    <div className="flex flex-col gap-4">
      <PromptInputButton
        tooltip="Select agent"
        onClick={() => setOpen(true)}
        className="w-fit max-w-36"
      >
        <AgentLogo icon={selectedIcon} />
        <span className="truncate">{stripAcpSuffix(selectedName)}</span>
      </PromptInputButton>

      <CommandDialog
        open={open}
        onOpenChange={setOpen}
        variant="palette"
        compact
        title="Choose an agent"
        description="Pick who you want to work with."
      >
        <Command loop>
          <CommandInput
            placeholder="Search agents…"
            aria-label="Search agents"
          />
          <CommandList>
            <CommandEmpty>No matching agents. Try another name.</CommandEmpty>
            {[...availableAgents, ...installableAgents].map(renderAgent)}
          </CommandList>
        </Command>
      </CommandDialog>
    </div>
  );
}
