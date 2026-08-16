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
  usePromptInputAttachments,
} from "@/components/ai-elements/prompt-input";
import {
  Attachment,
  AttachmentPreview,
  AttachmentRemove,
  Attachments,
} from "@/components/ai-elements/attachments";
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
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { useAgentCatalog } from "@/hooks/use-agent-catalog";
import { daemonApi } from "@/api";
import { notify } from "@/lib/notify";
import { open } from "@tauri-apps/plugin-dialog";
import type { AgentInfo, Chat, PromptAttachment } from "@/types";
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

function PromptAttachmentPreviews() {
  const attachments = usePromptInputAttachments();
  if (attachments.files.length === 0) return null;

  return (
    <Attachments className="w-full justify-start px-3 pt-3" variant="grid">
      {attachments.files.map((file) => (
        <Attachment
          data={file}
          key={file.id}
          onRemove={() => attachments.remove(file.id)}
          title={file.filename ?? "Attachment"}
        >
          <AttachmentPreview />
          <AttachmentRemove />
        </Attachment>
      ))}
    </Attachments>
  );
}

export function AgentSelection({
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
      <span>{stripAcpSuffix(agent.name)}</span>

      {!agent.available && (
        <span className="ml-auto text-xs text-muted-foreground">
          Not installed
        </span>
      )}

      {agent.available && selectedAgent === agent.id && (
        <Check className="ml-auto size-4" />
      )}
    </CommandItem>
  );
  const availableAgents = agents.filter((agent) => agent.available);
  const unavailableAgents = agents.filter((agent) => !agent.available);

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
            {availableAgents.length > 0 && (
              <CommandGroup>{agents.map(renderAgent)}</CommandGroup>
            )}
          </CommandList>
        </Command>
      </CommandDialog>
    </div>
  );
}
function stripAcpSuffix(value: string): string {
  return value.replace(/\s*\bACP\s*$/i, "");
}

function toPromptAttachments(
  files: PromptInputMessage["files"],
): PromptAttachment[] {
  return files.map((file) => {
    const match = /^data:([^;,]+);base64,(.+)$/s.exec(file.url);
    if (!match?.[1] || !match[2]) {
      throw new Error("The attachment could not be prepared for sending.");
    }
    return {
      filename: file.filename ?? null,
      mime_type: match[1],
      data: match[2],
    };
  });
}

interface AppPromptInputProps {
  onChatStarted?: (
    chat: Chat,
    agent: AgentInfo,
    workspacePath: string,
    sessionMode: SessionMode,
  ) => void;
  onSendPrompt?: (
    text: string,
    attachments: PromptAttachment[],
    sessionMode: SessionMode,
  ) => Promise<void>;
  workspacePath: string;
  onWorkspacePathChange?: (workspacePath: string) => void;
  selectedAgentId: string;
  onAgentSelected?: (agent: AgentInfo) => void;
  isWorking?: boolean;
  onStop?: () => void;
  sessionMode?: SessionMode;
  onSessionModeChange?: (mode: SessionMode) => Promise<void> | void;
}

function AppPromptInput({
  onChatStarted,
  onSendPrompt,
  workspacePath,
  onWorkspacePathChange,
  selectedAgentId,
  onAgentSelected,
  isWorking = false,
  onStop,
  sessionMode,
  onSessionModeChange,
}: AppPromptInputProps) {
  const [uncontrolledMode, setUncontrolledMode] =
    useState<SessionMode>("build");
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
      notify("Unable to open the directory picker.", "error");
    }
  };

  const handleSubmit = async (message: PromptInputMessage) => {
    const text = message.text.trim();
    const attachments = toPromptAttachments(message.files);
    if (!text && attachments.length === 0) return;
    if (onSendPrompt) {
      await onSendPrompt(text, attachments, mode);
      return;
    }
    if (!workspacePath || !selectedAgentId) {
      notify("Choose a workspace and agent, then enter a prompt.", "error");
      return;
    }
    try {
      const agent = agents.find(
        (candidate) => candidate.id === selectedAgentId,
      );
      if (!agent) throw new Error("Selected agent is no longer available.");
      if (!agent.available)
        throw new Error(
          agent.unavailable_reason ?? "Selected agent is not installed.",
        );
      const title =
        text.slice(0, 72) || message.files[0]?.filename || "Attachment prompt";
      const chat = await daemonApi.createChat(workspacePath, title);

      // Transition immediately. The daemon's prompt RPC remains open until the
      // agent turn finishes, while the chat screen renders via the event stream.
      onChatStarted?.(chat, agent, workspacePath, mode);
      void daemonApi
        .prompt(chat.id, selectedAgentId, text, attachments, mode)
        .catch((error: unknown) => {
          console.error("Error submitting prompt:", error);
          notify("The agent could not start this prompt.", "error");
        });
    } catch (error) {
      console.error("Error submitting prompt:", error);
      notify(
        error instanceof Error
          ? error.message
          : "An error occurred while submitting the prompt.",
        "error",
      );
    }
  };
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
    <PromptInput
      accept="image/png,image/jpeg,image/webp,image/gif,text/plain"
      maxFiles={4}
      maxFileSize={10 * 1024 * 1024}
      multiple
      onError={({ message }) => notify(message, "error")}
      onSubmit={handleSubmit}
    >
      <PromptInputBody>
        <PromptAttachmentPreviews />
        <PromptInputTextarea />
      </PromptInputBody>
      <PromptInputFooter>
        <PromptInputTools>
          {showModeControl && (
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
                    <DropdownMenuItem
                      key={value}
                      onSelect={() => void selectMode(value)}
                    >
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
          )}
          {onAgentSelected && (
            <AgentSelection
              setSelectedAgent={selectAgent}
              selectedAgent={selectedAgentId}
              agents={agents}
            />
          )}
          <PromptInputButton
            tooltip={workspacePath || "Choose a project folder"}
            disabled={isChatComposer}
            onClick={openDirectory}
            className="max-w-40"
            title={workspacePath || ""}
          >
            <FolderOpen size={16} />
            {workspacePath && (
              <span className="min-w-0 truncate text-left [direction:rtl]">
                {workspacePath}
              </span>
            )}
          </PromptInputButton>
        </PromptInputTools>
        <PromptInputSubmit
          disabled={!selectedAgentId || !workspacePath}
          status={isWorking ? "streaming" : "ready"}
          onStop={onStop}
        />
      </PromptInputFooter>
    </PromptInput>
  );
}

export default AppPromptInput;
