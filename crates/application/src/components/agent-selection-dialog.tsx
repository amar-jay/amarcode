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
import {
  PromptInputButton,
} from "@/components/ai-elements/prompt-input";
import {
  Check,
} from "lucide-react";
import { AgentLogo } from "@/components/agent-logo";


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

  const selectedName =
    agents.find((agent) => agent.id === selectedAgent)?.name ?? "Agent";
  const selectedIcon = agents.find((agent) => agent.id === selectedAgent)?.icon;
  const renderAgent = (agent: AgentInfo) => (
    <CommandItem
      key={agent.id}
      value={stripAcpSuffix(agent.name)}
      disabled={!agent.available}
      title={agent.unavailable_reason ?? undefined}
      onSelect={() => {
        setSelectedAgent(agent.id);
        setOpen(false);
      }}
      className="w-full cursor-pointer data-[disabled=true]:cursor-not-allowed data-[disabled=true]:opacity-50"
    >
      <AgentLogo icon={agent.icon} />
      <span className="min-w-0 flex-1 truncate">
        {stripAcpSuffix(agent.name)}
      </span>

      {!agent.available && (
        <span className="shrink-0 text-xs text-muted-foreground">
          Not installed
        </span>
      )}

      {agent.available && selectedAgent === agent.id && (
        <Check className="ml-auto size-4" />
      )}
    </CommandItem>
  );
  const availableAgents = agents.filter((agent) => agent.available);

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
              <CommandGroup>{agents.map(renderAgent)}</CommandGroup>
            )}
          </CommandList>
        </Command>
      </CommandDialog>
    </div>
  );
}