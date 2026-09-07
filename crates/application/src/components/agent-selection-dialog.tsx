"use client";

import { AgentInfo } from "@/types";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { useState } from "react";
import { useSetAtom } from "jotai";
import { PromptInputButton } from "@/components/ai-elements/prompt-input";
import { Check, Download, LoaderCircle } from "lucide-react";
import { AgentLogo } from "@/components/agent-logo";
import { installAgentAtom } from "@/state/agents";
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

  const selectedName =
    agents.find((agent) => agent.id === selectedAgent)?.name ?? "Agent";
  const selectedIcon = agents.find((agent) => agent.id === selectedAgent)?.icon;

  const handleInstall = async (agent: AgentInfo) => {
    if (installingId) return;
    setInstallingId(agent.id);
    try {
      const result = await installAgent(agent.id);
      setSelectedAgent(result.agent.id);
      setOpen(false);
      if (result.runtime_status === "ready") {
        notify(`${stripAcpSuffix(result.agent.name)} installed`, "success");
      } else if (result.runtime_status === "auth_required") {
        notify(
          `${stripAcpSuffix(result.agent.name)} installed — sign in required`,
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
    const installing = installingId === agent.id;
    return (
      <CommandItem
        key={agent.id}
        value={stripAcpSuffix(agent.name)}
        disabled={installingId !== null && !installing}
        title={agent.unavailable_reason ?? undefined}
        onSelect={() => {
          if (!agent.available) return;
          setSelectedAgent(agent.id);
          setOpen(false);
        }}
        className="w-full cursor-pointer data-[disabled=true]:cursor-not-allowed data-[disabled=true]:opacity-50"
      >
        <AgentLogo icon={agent.icon} />
        <span className="min-w-0 flex-1 truncate">
          {stripAcpSuffix(agent.name)}
        </span>

        {!agent.available ? (
          <button
            type="button"
            className="inline-flex size-6 shrink-0 items-center justify-center rounded-md text-primary hover:bg-foreground/10 disabled:opacity-50"
            aria-label={`Install ${stripAcpSuffix(agent.name)}`}
            title={`Install ${stripAcpSuffix(agent.name)}`}
            disabled={installingId !== null}
            onClick={(event) => {
              event.preventDefault();
              event.stopPropagation();
              void handleInstall(agent);
            }}
          >
            {installing ? (
              <LoaderCircle className="size-3.5 animate-spin" />
            ) : (
              <Download className="size-3.5" />
            )}
          </button>
        ) : selectedAgent === agent.id ? (
          <Check className="ml-auto size-4" />
        ) : null}
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
        className="w-fit"
      >
        <AgentLogo icon={selectedIcon} />
        {stripAcpSuffix(selectedName)}
      </PromptInputButton>

      <CommandDialog open={open} onOpenChange={setOpen}>
        <Command>
          <CommandInput placeholder="Search agents..." />
          <CommandList>
            <CommandEmpty>No agents found.</CommandEmpty>
            {availableAgents.length > 0 && (
              <CommandGroup heading="Available">
                {availableAgents.map(renderAgent)}
              </CommandGroup>
            )}
            {installableAgents.length > 0 && (
              <CommandGroup heading="Install">
                {installableAgents.map(renderAgent)}
              </CommandGroup>
            )}
          </CommandList>
        </Command>
      </CommandDialog>
    </div>
  );
}
