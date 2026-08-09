import { useEffect, useState } from "react";
import type { SourceDocumentUIPart } from "ai";
import {
  Attachment,
  AttachmentInfo,
  AttachmentPreview,
  AttachmentRemove,
  Attachments,
} from "@/components/ai-elements/attachments";
import {
  PromptInput,
  PromptInputActionAddAttachments,
  PromptInputActionMenu,
  PromptInputActionMenuContent,
  PromptInputActionMenuTrigger,
  PromptInputBody,
  PromptInputButton,
  PromptInputFooter,
  PromptInputHeader,
  type PromptInputMessage,
  PromptInputProvider,
  PromptInputSubmit,
  PromptInputTextarea,
  PromptInputTools,
  usePromptInputAttachments,
  usePromptInputReferencedSources,
} from "@/components/ai-elements/prompt-input";
import { Badge } from "@/components/ui/badge";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { AgentDefinition } from "@/types";
import {
  AtSign,
  Bot,
  FolderOpen,
  MessageCircle,
  Ruler,
  Wrench,
} from "lucide-react";

export type WorkMode = "plan" | "build" | "ask";
type Source = SourceDocumentUIPart & { id: string };
type Props = {
  agent?: AgentDefinition;
  workspacePath: string;
  isWorking: boolean;
  onSubmit: (input: {
    text: string;
    files: PromptInputMessage["files"];
    sources: Source[];
    mode: WorkMode;
  }) => Promise<void>;
  onStop: () => void;
};
const modes: WorkMode[] = ["plan", "build", "ask"];
const modeLabel = { plan: "Plan", build: "Build", ask: "Ask" };

function ContextChips() {
  const attachments = usePromptInputAttachments();
  const refs = usePromptInputReferencedSources();
  if (!attachments.files.length && !refs.sources.length) return null;
  return (
    <div className="flex flex-wrap gap-1.5">
      <Attachments variant="inline">
        {attachments.files.map((file) => (
          <Attachment
            data={file}
            key={file.id}
            onRemove={() => attachments.remove(file.id)}
          >
            <AttachmentPreview />
            <AttachmentInfo />
            <AttachmentRemove />
          </Attachment>
        ))}
      </Attachments>
      <Attachments variant="inline">
        {refs.sources.map((source) => (
          <Attachment
            data={source as Source}
            key={source.id}
            onRemove={() => refs.remove(source.id)}
          >
            <AttachmentPreview />
            <AttachmentInfo />
            <AttachmentRemove />
          </Attachment>
        ))}
      </Attachments>
    </div>
  );
}

function SourceSync({ onChange }: { onChange: (sources: Source[]) => void }) {
  const refs = usePromptInputReferencedSources();
  useEffect(() => onChange(refs.sources), [onChange, refs.sources]);
  return null;
}

function PasteAwareTextarea() {
  const attachments = usePromptInputAttachments();
  return (
    <PromptInputTextarea
      className="px-3 py-2 text-sm"
      onPaste={(event) => {
        const files = [...event.clipboardData.files].filter((file) =>
          file.type.startsWith("image/"),
        );
        if (files.length) {
          event.preventDefault();
          attachments.add(files);
        }
      }}
      placeholder="Plan, search, or build anything… Paste an image or drop files."
    />
  );
}

function ContextPicker({ workspacePath }: { workspacePath: string }) {
  const refs = usePromptInputReferencedSources();
  const [files, setFiles] = useState<string[]>([]);
  useEffect(() => {
    // void api
    //   .workspaceFiles(workspacePath)
    //   .then(setFiles)
    //   .catch(() => setFiles([]));
  }, [workspacePath]);
  const sources: SourceDocumentUIPart[] = files.map((filename) => ({
    type: "source-document",
    sourceId: filename,
    title: filename.split("/").at(-1) ?? filename,
    filename,
    mediaType: "text/plain",
  }));
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <PromptInputButton tooltip="Add workspace file" variant="ghost">
          <AtSign className="size-4" />
        </PromptInputButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        className="max-h-72 w-72 overflow-y-auto"
      >
        {sources
          .filter(
            (source) =>
              !refs.sources.some((added) => added.sourceId === source.sourceId),
          )
          .map((source) => (
            <DropdownMenuItem
              key={source.sourceId}
              onSelect={() => refs.add(source)}
            >
              <FolderOpen className="size-3.5" />
              <span className="truncate">{source.filename}</span>
            </DropdownMenuItem>
          ))}
        {files.length === 0 && (
          <DropdownMenuItem disabled>Loading workspace files…</DropdownMenuItem>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function RulesPicker({
  mode,
  onMode,
}: {
  mode: WorkMode;
  onMode: (value: WorkMode) => void;
}) {
  const icons = { plan: Ruler, build: Wrench, ask: MessageCircle };
  const colors = {
    plan: "text-violet-500",
    build: "text-amber-500",
    ask: "text-sky-500",
  };
  const Icon = icons[mode];
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <PromptInputButton tooltip="Working mode" size="sm" variant="ghost">
          <Icon className={`size-3.5 ${colors[mode]}`} />
          <span>{modeLabel[mode]}</span>
        </PromptInputButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-40">
        {modes.map((value) => {
          const ModeIcon = icons[value];
          return (
            <DropdownMenuItem key={value} onSelect={() => onMode(value)}>
              <ModeIcon className={`${colors[value]} size-3.5`} />
              <span>{modeLabel[value]}</span>
              {mode === value && (
                <span className="ml-auto size-1.5 rounded-full bg-current" />
              )}
            </DropdownMenuItem>
          );
        })}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function Composer({
  agent,
  workspacePath,
  isWorking,
  onSubmit,
  onStop,
}: Props) {
  const [mode, setMode] = useState<WorkMode>("build");
  const [sources, setSources] = useState<Source[]>([]);
  return (
    <div className="mx-auto w-full max-w-3xl">
      <div className="mb-2 flex min-w-0 items-center gap-2 px-1 text-[11px] text-muted-foreground">
        <Tooltip>
          <TooltipTrigger asChild>
            <span className="flex min-w-0 items-center gap-1.5">
              <FolderOpen className="size-3 shrink-0" />
              <span className="truncate">{workspacePath}</span>
            </span>
          </TooltipTrigger>
          <TooltipContent>{workspacePath}</TooltipContent>
        </Tooltip>
        <span className="text-border">/</span>
        <span className="flex shrink-0 items-center gap-1.5">
          <Bot className="size-3" />
          {agent?.name ?? "ACP agent"}
        </span>
      </div>
      <PromptInput
        accept="image/*,.txt,.md,.json,.ts,.tsx,.js,.jsx,.rs,.py,.log"
        className="rounded-xl border border-border bg-card shadow-sm"
        globalDrop
        maxFiles={10}
        maxFileSize={10 * 1024 * 1024}
        multiple
        onSubmit={async (message) => {
          if (!message.text.trim() && !message.files.length) return;
          await onSubmit({
            files: message.files,
            mode,
            sources,
            text: message.text.trim(),
          });
        }}
      >
        <PromptInputHeader className="items-center px-3 pt-2">
          <SourceSync onChange={setSources} />
          <ContextChips />
        </PromptInputHeader>
        <PromptInputBody>
          <PasteAwareTextarea />
        </PromptInputBody>
        <PromptInputFooter className="border-t border-border/70 px-2 py-1.5">
          <PromptInputTools>
            <PromptInputActionMenu>
              <PromptInputActionMenuTrigger tooltip="Attach files" />
              <PromptInputActionMenuContent>
                <PromptInputActionAddAttachments label="Attach files" />
              </PromptInputActionMenuContent>
            </PromptInputActionMenu>
            <ContextPicker workspacePath={workspacePath} />
            <RulesPicker mode={mode} onMode={setMode} />
          </PromptInputTools>
          <div className="flex items-center gap-2">
            <Badge variant="secondary" className="hidden sm:inline-flex">
              ⌘↵ Send
            </Badge>
            <PromptInputSubmit
              onStop={onStop}
              status={isWorking ? "streaming" : "ready"}
            />
          </div>
        </PromptInputFooter>
      </PromptInput>
      <p className="mt-2 px-1 text-[11px] text-muted-foreground">
        Paste images directly, drop files, or use @ to add workspace files.
      </p>
    </div>
  );
}

export function WorkbenchPromptInput(props: Props) {
  return (
    <PromptInputProvider>
      <Composer {...props} />
    </PromptInputProvider>
  );
}
