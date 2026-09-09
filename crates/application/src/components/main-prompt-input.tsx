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
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Check,
  Ellipsis,
  FolderOpen,
  ShieldCheck,
  ShieldQuestion,
} from "lucide-react";
import { useAgentCatalog } from "@/hooks/use-agent-catalog";
import { daemonApi } from "@/api";
import { notify } from "@/lib/notify";
import { cn } from "@/lib/utils";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  AgentInfo,
  Chat,
  PromptAttachment,
  SessionConfigAssignment,
  SessionConfigOption,
  SessionConfigValue,
} from "@/types";
import { permissionModeAtom, type PermissionMode } from "@/state";
import {
  assignmentsFromOptions,
  loadLastSessionConfig,
  rememberSessionConfig,
} from "@/state/session-config";
import { AgentSelectionDialog } from "./agent-selection-dialog";
import { SessionConfigControls } from "./session-config-controls";

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

function NewChatOptionsMenu({
  options,
  permissionMode,
  onPermissionModeChange,
  onConfigChange,
}: {
  options: SessionConfigOption[];
  permissionMode: PermissionMode;
  onPermissionModeChange: (mode: PermissionMode) => void;
  onConfigChange: (configId: string, value: SessionConfigValue) => void;
}) {
  const visibleOptions = options.filter(
    (option) => option.type === "boolean" || option.type === "select",
  );

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <PromptInputButton tooltip="More options" aria-label="More options">
          <Ellipsis className="size-4" />
        </PromptInputButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-44">
        <DropdownMenuLabel>New chat options</DropdownMenuLabel>
        <DropdownMenuSub>
          <DropdownMenuSubTrigger>
            {permissionMode === "confirm" ? (
              <ShieldQuestion />
            ) : (
              <ShieldCheck />
            )}
            <span>Permissions</span>
            <span className="ml-auto text-[10px] text-muted-foreground">
              {
                permissionModes.find((item) => item.value === permissionMode)
                  ?.label
              }
            </span>
          </DropdownMenuSubTrigger>
          <DropdownMenuSubContent className="min-w-32">
            {permissionModes.map((item) => (
              <DropdownMenuItem
                key={item.value}
                onSelect={() => onPermissionModeChange(item.value)}
              >
                <span className="min-w-0 flex-1">{item.label}</span>
                {permissionMode === item.value && <Check />}
              </DropdownMenuItem>
            ))}
          </DropdownMenuSubContent>
        </DropdownMenuSub>
        {visibleOptions.length > 0 && <DropdownMenuSeparator />}
        {visibleOptions.map((option) =>
          option.type === "boolean" ? (
            <DropdownMenuCheckboxItem
              key={option.id}
              checked={Boolean(option.current_value)}
              title={option.description ?? undefined}
              onCheckedChange={(checked) =>
                onConfigChange(option.id, {
                  type: "boolean",
                  value: checked === true,
                })
              }
            >
              {option.name}
            </DropdownMenuCheckboxItem>
          ) : (
            <DropdownMenuSub key={option.id}>
              <DropdownMenuSubTrigger title={option.description ?? undefined}>
                <span>{option.name}</span>
                <span className="ml-auto max-w-20 truncate text-[10px] text-muted-foreground">
                  {
                    (option.options ?? []).find(
                      (choice) => choice.value === option.current_value,
                    )?.name
                  }
                </span>
              </DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="min-w-36">
                {(option.options ?? []).map((choice) => (
                  <DropdownMenuItem
                    key={choice.value}
                    title={choice.description ?? undefined}
                    onSelect={() =>
                      onConfigChange(option.id, {
                        type: "id",
                        value: choice.value,
                      })
                    }
                  >
                    <span className="min-w-0 flex-1">{choice.name}</span>
                    {option.current_value === choice.value && <Check />}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuSubContent>
            </DropdownMenuSub>
          ),
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

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
  onChatStarted?: (chat: Chat, agent: AgentInfo, workspacePath: string) => void;
  onStartedPromptFailed?: (chatId: string, error: string) => void;
  onSendPrompt?: (
    text: string,
    attachments: PromptAttachment[],
    configValues: SessionConfigAssignment[],
  ) => Promise<void>;
  workspacePath: string;
  onWorkspacePathChange?: (workspacePath: string) => void;
  selectedAgentId: string;
  onAgentSelected?: (agent: AgentInfo) => void;
  isWorking?: boolean;
  onStop?: () => void;
  sessionConfig?: SessionConfigOption[];
  onSessionConfigChange?: (
    configId: string,
    value: SessionConfigValue,
  ) => Promise<void> | void;
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
  sessionConfig,
  onSessionConfigChange,
}: AppPromptInputProps) {
  const [permissionMode, setPermissionMode] = useAtom(permissionModeAtom);
  const [pendingConfig, setPendingConfig] = useState<{
    agentId: string;
    options: SessionConfigOption[];
  }>({ agentId: selectedAgentId, options: [] });
  const isChatComposer = Boolean(onSendPrompt);
  const lastKnown = loadLastSessionConfig(selectedAgentId);
  const pendingOptions =
    pendingConfig.agentId === selectedAgentId ? pendingConfig.options : [];
  const options =
    sessionConfig !== undefined
      ? sessionConfig
      : pendingOptions.length
        ? pendingOptions
        : lastKnown;

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
      await onSendPrompt(text, attachments, assignmentsFromOptions(options));
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
      onChatStarted?.(chat, agent, workspacePath);
      void daemonApi
        .prompt(
          chat.id,
          selectedAgentId,
          text,
          attachments,
          assignmentsFromOptions(options),
        )
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
  const selectConfig = async (configId: string, value: SessionConfigValue) => {
    if (onSessionConfigChange) {
      await onSessionConfigChange(configId, value);
      return;
    }
    setPendingConfig((current) => {
      const base =
        current.agentId === selectedAgentId && current.options.length
          ? current.options
          : options;
      const next = base.map((option) =>
        option.id === configId
          ? {
              ...option,
              current_value:
                value.type === "boolean" ? value.value : value.value,
            }
          : option,
      );
      rememberSessionConfig(selectedAgentId, next);
      return { agentId: selectedAgentId, options: next };
    });
  };
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
          {isChatComposer && (
            <>
              <SessionConfigControls
                options={options}
                disabled={isWorking}
                onChange={(configId, value) =>
                  void selectConfig(configId, value)
                }
              />
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
                      {permissionModes.find(
                        (item) => item.value === permissionMode,
                      )?.label ?? "Confirm actions"}
                    </span>
                  </PromptInputButton>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="center" className="w-24">
                  {permissionModes.map((item) => (
                    <DropdownMenuItem
                      key={item.value}
                      onSelect={() => setPermissionMode(item.value)}
                      className="min-h-0 items-center py-1 text-foreground opacity-100 hover:text-foreground!"
                    >
                      <span className="min-w-0 flex-1">
                        <span className="block py-0 text-xs">{item.label}</span>
                      </span>
                      {permissionMode === item.value && (
                        <Check className="size-3.5 shrink-0" />
                      )}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            </>
          )}
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
          {!isChatComposer && (
            <NewChatOptionsMenu
              options={options}
              permissionMode={permissionMode}
              onPermissionModeChange={setPermissionMode}
              onConfigChange={(configId, value) =>
                void selectConfig(configId, value)
              }
            />
          )}
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
