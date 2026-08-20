import { DiffArtifact, diffArtifacts } from "@/components/diff-artifact-card";
import { MessageDetail, MessagePart } from "@/types";
import {
  FileText,
  Pencil,
  Trash2,
  Move,
  Search,
  Terminal,
  Globe,
  Wrench,
  type LucideIcon,
  Dot,
	ListTree,
} from "lucide-react";

const TOOL_KIND_ICONS = {
  read: FileText,
	read_file: FileText,
  edit: Pencil,
  delete: Trash2,
	list_dir: ListTree,
  move: Move,
	run_terminal_command: Terminal,
  search: Search,  
	search_replace: Search, grep: Search,
  execute: Terminal,
  thinking: Dot,
  fetch: Globe,
  other: Wrench,
} as Record<ToolKind, LucideIcon>;

export type ToolKind =
  | "read" | "read_file"
  | "edit"
  | "delete"
  | "move"
  | "search" | "search_replace" | "grep"
  | "execute"
  | "thinking"
  | "fetch"
  | "other";

export function getToolKindIcon(kind?: ToolKind): LucideIcon {
	console.log("getToolKindIcon kind:", kind);
	if (!kind) return Wrench;
	if (!(kind in TOOL_KIND_ICONS)) return Wrench; 
  return TOOL_KIND_ICONS[kind];
}

//TODO: use proper one.
type ToolMessage = {
  _meta?: {
    terminal_info?: {
      cwd: string;
      terminal_id: string;
    };

    terminal_output_delta?: {
      data: string;
      terminal_id: string;
    };

    terminal_exit?: {
      exit_code: number;
      signal: string | null;
      terminal_id: string;
    };

    [key: string]: unknown;
  };

  sessionUpdate: "tool_call" | "tool_call_update";

  toolCallId: string;

  status?: "in_progress" | "completed" | "failed";

  kind?: string;

  title?: string;
  name?: string;
  toolName?: string;

  content?: Array<Record<string, unknown>>;

  rawInput?: {
    command?: string;
    cwd?: string;
    [key: string]: unknown;
  };

  rawOutput?: {
    exit_code: number;
    formatted_output: string;
  };

  [key: string]: unknown;
};

/** One user bubble, or one assistant turn (many ACP messages collapsed). */
export type ChatBlock =
  | { kind: "user"; key: string; item: MessageDetail }
  | {
      kind: "assistant";
      key: string;
      items: MessageDetail[];
      content: string;
      streaming: boolean;
      status: string;
      timeline: TimelineStep[];
      diffs: DiffArtifact[];
    };

type TimelineStep = {
  key: string;
  label: string;
  kind: string;
  description?: string;
  icon?: LucideIcon;
  status: "complete" | "active";
};

function partText(part: MessagePart): string | null {
  try {
    const value: unknown = JSON.parse(part.content_json);
    return typeof value === "object" &&
      value !== null &&
      "text" in value &&
      typeof value.text === "string" &&
      Boolean(value.text.trim())
      ? value.text
      : null;
  } catch {
    return null;
  }
}

function stringed(value: unknown, fallback: string): string {
  return typeof value === "string" ? value.trim() : fallback;
}
function getToolLabel(msg: ToolMessage): string | undefined {
  switch ("string") {
    case typeof msg.title:
      return msg.title?.trim().toLocaleLowerCase();
    case typeof msg.name:
      return msg.name?.trim().toLocaleLowerCase();
    case typeof msg.toolName:
      return msg.toolName?.trim().toLocaleLowerCase();
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function toolSummary(part: MessagePart, verbose: boolean) {
  try {
    const value = JSON.parse(part.content_json);
    if (typeof value !== "object" || value === null) return null;
    const record = value as ToolMessage;
    let label = getToolLabel(record);
    if (!label) return null;
    const id = stringed(record.toolCallId, label);
		if (!record.kind) {
			record.kind = cleanToolTitle(label).toLocaleLowerCase();
		}
		console.log("tool kind: ", record.kind, "label:", label);
    const kind = stringed(record.kind, stringed(record.label, "other")) as ToolKind;

    // In verbose mode, surface rawInput command when title is generic.
    if (verbose) {
      const command = isRecord(record.rawInput)
        ? stringed(record.rawInput.command, stringed(record.command, label))
        : label;
      return { id, label: command, kind };
    }
    return { id, label, kind };
  } catch {
    return null;
  }
}

export function cleanToolTitle(value: string): string {
  const result = value.trim().replace(/^[\r\n\s]*\*\*+|\*\*+[\r\n\s]*$/g, "");
  return result.trim();
}

export function cleanThinking(value: string): string {
  const sp = value.trim().split(/\r?\n+/);
  return sp.map((s) => cleanToolTitle(s)).join("\n");
}

export function assistantMessageTone(
  content: string,
): "warning" | "error" | null {
  const text = content.trim();
  if (/^(error|failed|failure|unable to|cannot |can't )/i.test(text))
    return "error";
  if (/^(warning|caution|notice)/i.test(text)) return "warning";
  return null;
}

/** Hide the exact prompt echo produced by older daemon versions. */
function removeLeadingPromptEcho(content: string, prompt: string): string {
  const echoedPrompt = prompt.trim();
  return echoedPrompt && content.startsWith(echoedPrompt)
    ? content.slice(echoedPrompt.length).trimStart()
    : content;
}

/** Keep an initial notice distinct from the assistant's actual response. */
export function splitLeadingWarning(
  content: string,
): { warning: string; response: string } | null {
  const paragraphs = content.trim().split(/\n\s*\n/);
  const [warning, ...response] = paragraphs;

  if (
    !warning ||
    !response.length ||
    !/^(warning|caution|notice)/i.test(warning)
  ) {
    return null;
  }

  return { warning, response: response.join("\n\n") };
}

function buildTimeline(
  items: MessageDetail[],
  streaming: boolean,
  verbose: boolean,
): TimelineStep[] {
  const seenTools = new Set<string>();
  const steps: TimelineStep[] = [];

  for (const { message, parts } of items) {
    const ordered = parts
      .filter((part) => part.kind === "thinking" || part.kind === "tool_call")
      .sort((left, right) => left.ordinal - right.ordinal);

    for (const part of ordered) {
      if (part.kind === "thinking") {
        const text = partText(part);
        if (!text?.trim()) continue;
        // Compact mode: skip very short status-style thoughts that only add noise.
        if (!verbose && text.trim().length < 8) continue;
        steps.push({
          key: `${message.id}-thinking-${part.ordinal}`,
          label: text,
          kind: "thinking",
          status: "complete",
        });
        continue;
      }
      const tool = toolSummary(part, verbose);
      if (!tool || seenTools.has(tool.id)) continue;
      seenTools.add(tool.id);
      steps.push({
        key: `${message.id}-tool-${part.ordinal}`,
        label: tool.label,
        icon: getToolKindIcon(tool.kind),
        kind: tool.kind,
        status:
          streaming && message.status === "streaming" ? "active" : "complete",
      });
    }
  }
  return steps;
}

/**
 * ACP may emit several assistant messages per turn (commentary vs final, or
 * per-messageId streams). Render them as one bubble so the UI doesn't look
 * like interleaved "Reasoning…" islands.
 */
export function groupChatBlocks(
  messages: MessageDetail[],
  turnWorking: boolean,
  verbose: boolean,
): ChatBlock[] {
  const blocks: ChatBlock[] = [];

  for (const item of messages) {
    if (item.message.role === "user") {
      blocks.push({ kind: "user", key: item.message.id, item });
      continue;
    }
    if (item.message.role !== "assistant") continue;

    // Skip empty assistant shells — they only produce orphan "Thinking…" rows.
    const hasText = Boolean(item.message.content.trim());
    const hasParts = item.parts.some(
      (part) => part.kind === "thinking" || part.kind === "tool_call",
    );
    const hasDiff = diffArtifacts(item.parts).length > 0;
    if (!hasText && !hasParts && !hasDiff) continue;

    const runId = item.message.agent_run_id;
    const prev = blocks[blocks.length - 1];
    const canMerge =
      prev?.kind === "assistant" &&
      runId != null &&
      prev.items[0]?.message.agent_run_id === runId;

    if (canMerge && prev.kind === "assistant") {
      prev.items.push(item);
      continue;
    }

    blocks.push({
      kind: "assistant",
      key: item.message.id,
      items: [item],
      content: "",
      streaming: false,
      status: item.message.status,
      timeline: [],
      diffs: [],
    });
  }

  // Materialize merged fields.
  for (const [index, block] of blocks.entries()) {
    if (block.kind !== "assistant") continue;
    block.content = block.items
      .map((item) => item.message.content)
      .filter((text) => text.trim())
      .join("\n\n");
    const previous = blocks[index - 1];
    if (
      previous?.kind === "user" &&
      previous.item.message.agent_run_id ===
        block.items[0]?.message.agent_run_id
    ) {
      block.content = removeLeadingPromptEcho(
        block.content,
        previous.item.message.content,
      );
    }
    block.streaming = block.items.some(
      (item) =>
        item.message.status === "streaming" ||
        (turnWorking &&
          item.message.id === messages[messages.length - 1]?.message.id),
    );
    block.status = block.items.some((item) => item.message.status === "failed")
      ? "failed"
      : block.items.some((item) => item.message.status === "interrupted")
        ? "interrupted"
        : block.streaming
          ? "streaming"
          : "complete";
    block.timeline = buildTimeline(block.items, block.streaming, verbose);
    block.diffs = block.items.flatMap((item) => diffArtifacts(item.parts));
  }

  return blocks;
}
