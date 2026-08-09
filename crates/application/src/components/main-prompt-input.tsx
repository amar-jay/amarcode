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
	BellIcon,
	BotIcon,
	CalculatorIcon,
	CalendarIcon,
	Check,
  ClipboardPasteIcon,
  CodeIcon,
  CopyIcon,
  CreditCardIcon,
  FileTextIcon,
  FolderIcon,
  FolderOpen,
  FolderOpenDot,
  FolderPlusIcon,
  GlobeIcon,
  HelpCircleIcon,
  HomeIcon,
  ImageIcon,
  InboxIcon,
  LayoutGridIcon,
  ListIcon,
  MessageCircle,
  PaperclipIcon,
  PlusIcon,
  Ruler,
  ScissorsIcon,
  SettingsIcon,
  TrashIcon,
  UserIcon,
  Wrench,
	ZoomInIcon,
	ZoomOutIcon,
} from "lucide-react";
import { DialogTrigger, DialogContent, Dialog, DialogDescription, DialogHeader, DialogTitle } from "./ui/dialog";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
} from "@/components/ui/command"
import { Button } from "./ui/button";
import { useAgentCatalog } from "@/hooks/use-agent-catalog";
import { daemonApi } from "@/api";
import { toast } from "sonner";
import { open } from "@tauri-apps/plugin-dialog";
import type { AgentDefinition, Chat, PromptResult } from "@/types";

const handleSubmit = () => {
  // Handle submit

};

const SET_MODES = ["plan", "build", "ask"] as const;
const SET_MODELS = ["codex", "gemini", "claude"] as const;
type SetMode = (typeof SET_MODES)[number];

const modeLabels: Record<SetMode, string> = {
  plan: "Plan",
  build: "Build",
  ask: "Ask",
};

const modeIcons: Record<SetMode, typeof Ruler> = {
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


type MainPromptInputProps = {
  onChatStarted?: (chat: Chat, agent: AgentDefinition, prompt: PromptResult, workspacePath: string) => void;
  workspacePath: string;
  onWorkspacePathChange: (workspacePath: string) => void;
  selectedAgentId: string;
  onAgentSelected: (agent: AgentDefinition) => void;
};

const MainPromptInput = ({ onChatStarted, workspacePath, onWorkspacePathChange, selectedAgentId, onAgentSelected }: MainPromptInputProps) => {
  const [mode, setMode] = useState<SetMode>("build");
  const ModeIcon = modeIcons[mode];

	const openDirectory = async () => {
		try {
			const path = await open({
				directory: true,
				multiple: false,
				title: "Choose a project folder",
			});

			if (typeof path === "string") {
				onWorkspacePathChange(path);
			}
		} catch (error) {
			console.error("Error choosing workspace directory:", error);
			toast.error("Unable to open the directory picker.");
		}
	};

	const handleSubmit = async (message: PromptInputMessage) => {
		if (!workspacePath || !selectedAgentId || !message.text.trim()) {
			toast.error("Choose a workspace and agent, then enter a prompt.");
			return;
		}
		try{
		const chat = await daemonApi.createChat(workspacePath, message.text.trim().slice(0, 72))
		const prompt = await daemonApi.prompt(chat.id, selectedAgentId, message.text)
		const agent = agents.find((candidate) => candidate.id === selectedAgentId);
		if (agent) onChatStarted?.(chat, agent, prompt, workspacePath);
		} catch (error) {
			console.error("Error submitting prompt:", error);
			toast.error("An error occurred while submitting the prompt.");
		}
	}
	const agents = useAgentCatalog();
	const selectAgent = (agentId: string) => {
		const agent = agents.find((candidate) => candidate.id === agentId);
		if (agent) onAgentSelected(agent);
	};
  return (
    <PromptInput onSubmit={handleSubmit}>
      <PromptInputBody>
        <PromptInputTextarea />
      </PromptInputBody>
      <PromptInputFooter>
        <PromptInputTools>
          <DropdownMenu>
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
                  <DropdownMenuItem key={value} onSelect={() => setMode(value)}>
                    <Icon size={3} />
                    <span>{modeLabels[value]}</span>
                    {mode === value && (
											<Check className="ml-auto size-3.5 text-current" />
                    )}
                  </DropdownMenuItem>
                );
              })}
            </DropdownMenuContent>
          </DropdownMenu>
					<AgentSelection
							setSelectedAgent={selectAgent}
							selectedAgent={selectedAgentId}
							agents={agents}
					/>
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
        </PromptInputTools>
        <PromptInputSubmit disabled={!selectedAgentId || !workspacePath} />
      </PromptInputFooter>
    </PromptInput>
  );
};

export default MainPromptInput;
