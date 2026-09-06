"use client";

import { useState } from "react";
import { useAtom } from "jotai";
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
  Check,
  FolderOpen,
  MessageCircle,
  Ruler,
  ShieldCheck,
  ShieldQuestion,
  Wrench,
} from "lucide-react";
import { AgentLogo } from "@/components/agent-logo";

import { useAgentCatalog } from "@/hooks/use-agent-catalog";
import { daemonApi } from "@/api";
import { notify } from "@/lib/notify";
import { cn } from "@/lib/utils";
import { open } from "@tauri-apps/plugin-dialog";
import type { AgentInfo, Chat, PromptAttachment } from "@/types";
import {
  permissionModeAtom,
  SESSION_MODES,
  type PermissionMode,
  type SessionMode,
} from "@/state";
import { AgentSelectionDialog } from "./agent-selection-dialog";

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

const permissionModes: Array<{
  value: PermissionMode;
  label: string;
}> = [
  {
    value: "confirm",
    label: "Confirm",
  },
  {
    value: "auto-edit",
    label: "Auto-edit",
  },
  {
    value: "full-agentic",
    label: "Agentic",
  },
];

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
  onStartedPromptFailed?: (chatId: string, error: string) => void;
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
  onStartedPromptFailed,
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
  const [permissionMode, setPermissionMode] = useAtom(permissionModeAtom);
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
          const message =
            error instanceof Error
              ? error.message
              : "The agent could not start this prompt.";
          onStartedPromptFailed?.(chat.id, message);
          notify(message, "error");
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
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <PromptInputButton
                size="sm"
                className={cn(
                  "hover:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100",
                )}
              >
                {permissionMode === "confirm" ? (
                  <ShieldQuestion className="size-4" />
                ) : (
                  <ShieldCheck className="size-4" />
                )}
                <span>
                  {permissionModes.find((item) => item.value === permissionMode)
                    ?.label ?? "Confirm actions"}
                </span>
              </PromptInputButton>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="center" className="w-24">
              {permissionModes.map((item) => (
                <DropdownMenuItem
                  key={item.value}
                  onSelect={() => setPermissionMode(item.value)}
                  className="opacity-100 items-center py-1 min-h-0 text-foreground hover:text-foreground!"
                >
                  <span className="min-w-0 flex-1 ">
                    <span className="block text-xs py-0">{item.label}</span>
                  </span>
                  {permissionMode === item.value && (
                    <Check className="size-3.5 shrink-0" />
                  )}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
          {onAgentSelected && (
            <AgentSelectionDialog
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
