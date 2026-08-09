"use client";

import { useState } from "react";
import {
  PromptInput,
  PromptInputBody,
  PromptInputButton,
  PromptInputFooter,
  PromptInputMessage,
  PromptInputSubmit,
  PromptInputTextarea,
  PromptInputTools,
} from "@/components/ai-elements/prompt-input";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
	BotIcon,
	Check,
  FolderOpen,
  MessageCircle,
  Ruler,
  Wrench,
} from "lucide-react";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command"
import { useAgentCatalog } from "@/hooks/use-agent-catalog";
import { daemonApi } from "@/api";
import { toast } from "sonner";
import { open } from "@tauri-apps/plugin-dialog";
import type { AgentDefinition, Chat } from "@/types";
import { SESSION_MODES, type SessionMode } from "@/state";

export type { SessionMode };
const SET_MODES = SESSION_MODES;

const modeLabels: Record<SessionMode, string> = {
  plan: "Plan",
  build: "Build",
  ask: "Ask",
};

const modeIcons: Record<SessionMode, typeof Ruler> = {
  plan: Ruler,
  build: Wrench,
  ask: MessageCircle,
};


export function AgentSelection({
  setSelectedAgent,
  selectedAgent,
  agents,
}: {
  setSelectedAgent: (agentId: string) => void;
  selectedAgent: string;
  agents: AgentDefinition[];
}) {
  const [open, setOpen] = useState(false);

  const selectedName =
    agents.find((agent) => agent.id === selectedAgent)?.name ?? "Agent";

  return (
    <div className="flex flex-col gap-4">
      <PromptInputButton
        tooltip="Select agent"
        onClick={() => setOpen(true)}
        className="w-fit"
      >
        <BotIcon size={16} />
        {stripAcpSuffix(selectedName)}
      </PromptInputButton>

      <CommandDialog open={open} onOpenChange={setOpen}>
        <Command>
          <CommandInput placeholder="Search agents..." />

          <CommandList>
            <CommandEmpty>No agents found.</CommandEmpty>

            {agents.map((agent) => (
              <CommandItem
                key={agent.id}
                value={stripAcpSuffix(agent.name)}
                onSelect={() => {
                  setSelectedAgent(agent.id);
                  setOpen(false);
                }}
                className="cursor-pointer w-full"
              >
                <span>{stripAcpSuffix(agent.name)}</span>

                {selectedAgent === agent.id && (
                  <Check className="ml-auto size-4" />
                )}
              </CommandItem>
            ))}
          </CommandList>
        </Command>
      </CommandDialog>
    </div>
  );
}
function stripAcpSuffix(value: string): string {
  return value.replace(/\s*\bACP\s*$/i, "");
}

interface AppPromptInputProps {
  onChatStarted?: (chat: Chat, agent: AgentDefinition, workspacePath: string, sessionMode: SessionMode) => void;
  onSendPrompt?: (text: string, sessionMode: SessionMode) => Promise<void>;
  workspacePath: string;
  onWorkspacePathChange?: (workspacePath: string) => void;
  selectedAgentId: string;
  onAgentSelected?: (agent: AgentDefinition) => void;
  isWorking?: boolean;
  onStop?: () => void;
  sessionMode?: SessionMode;
  onSessionModeChange?: (mode: SessionMode) => Promise<void> | void;
};

function AppPromptInput({ onChatStarted, onSendPrompt, workspacePath, onWorkspacePathChange, selectedAgentId, onAgentSelected, isWorking = false, onStop, sessionMode, onSessionModeChange }: AppPromptInputProps) {
  const [uncontrolledMode, setUncontrolledMode] = useState<SessionMode>("build");
  const mode = sessionMode ?? uncontrolledMode;
  const ModeIcon = modeIcons[mode];
	const isChatComposer = Boolean(onSendPrompt);

	const openDirectory = async () => {
		try {
			const path = await open({
				directory: true,
				multiple: false,
				title: "Choose a project folder",
			});

			if (typeof path === "string") {
				onWorkspacePathChange?.(path);
			}
		} catch (error) {
			console.error("Error choosing workspace directory:", error);
			toast.error("Unable to open the directory picker.");
		}
	};

	const handleSubmit = async (message: PromptInputMessage) => {
		const text = message.text.trim();
		if (!text) return;
		if (onSendPrompt) {
			await onSendPrompt(text, mode);
			return;
		}
		if (!workspacePath || !selectedAgentId) {
			toast.error("Choose a workspace and agent, then enter a prompt.");
			return;
		}
		try{
		const chat = await daemonApi.createChat(workspacePath, text.slice(0, 72))
		const agent = agents.find((candidate) => candidate.id === selectedAgentId);
		if (!agent) throw new Error("Selected agent is no longer available.");

		// Transition immediately. The daemon's prompt RPC remains open until the
		// agent turn finishes, while the chat screen renders via the event stream.
		onChatStarted?.(chat, agent, workspacePath, mode);
		void daemonApi.prompt(chat.id, selectedAgentId, text, mode).catch((error: unknown) => {
			console.error("Error submitting prompt:", error);
			toast.error("The agent could not start this prompt.");
		});
		} catch (error) {
			console.error("Error submitting prompt:", error);
			toast.error("An error occurred while submitting the prompt.");
		}
	}
	const agents = useAgentCatalog();
	const selectAgent = (agentId: string) => {
		const agent = agents.find((candidate) => candidate.id === agentId);
		if (agent) onAgentSelected?.(agent);
	};
	const selectMode = async (nextMode: SessionMode) => {
		if (onSessionModeChange) await onSessionModeChange(nextMode);
		else setUncontrolledMode(nextMode);
	};
	const showModeControl = !isChatComposer || selectedAgentId === "codex-acp";
  return (
    <PromptInput onSubmit={handleSubmit}>
      <PromptInputBody>
        <PromptInputTextarea />
      </PromptInputBody>
      <PromptInputFooter>
        <PromptInputTools>
          {showModeControl && <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <PromptInputButton
                size="sm"
								className="w-12 mx-auto ring-0 focus:outline-none focus:ring-0 focus:ring-offset-0 focus-visible:ring-0 focus-visible:ring-offset-0"
              >
                <ModeIcon size={3} />
                <span>{modeLabels[mode]}</span>
              </PromptInputButton>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-8">
              {SET_MODES.map((value) => {
                const Icon = modeIcons[value];

                return (
                  <DropdownMenuItem key={value} onSelect={() => void selectMode(value)}>
                    <Icon size={3} />
                    <span>{modeLabels[value]}</span>
                    {mode === value && (
											<Check className="ml-auto size-3.5 text-current" />
                    )}
                  </DropdownMenuItem>
                );
              })}
            </DropdownMenuContent>
          </DropdownMenu>}
					{
						onAgentSelected && (
								<AgentSelection
										setSelectedAgent={selectAgent}
										selectedAgent={selectedAgentId}
										agents={agents}
								/>
						)
					}
					{
						!isChatComposer && (
			          <PromptInputButton
										tooltip={{
											content: workspacePath || "Choose a project folder",
										}}
									onClick={openDirectory}
										className="max-w-40"
			          >
			            <FolderOpen size={16} />
										{workspacePath && (
											<span className="truncate">…{workspacePath.slice(-15)}</span>
										)}
			          </PromptInputButton>
						)
					}
        </PromptInputTools>
        <PromptInputSubmit
          disabled={!selectedAgentId || !workspacePath}
          status={isWorking ? "streaming" : "ready"}
          onStop={onStop}
        />
      </PromptInputFooter>
    </PromptInput>
  );
};

export default AppPromptInput;
