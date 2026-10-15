import type { MessageDetail } from "@/types";
import { exactCommand } from "@/lib/command-display";

export type TerminalArtifact = {
  key: string;
  toolCallId: string;
  terminalId: string;
  command: string;
  cwd?: string;
  output: string;
  truncated: boolean;
  exitCode?: number;
  signal?: string;
  status: "running" | "completed" | "failed" | "cancelled";
  deferred: boolean;
};

type ToolState = {
  kind?: string;
  name?: string;
  title?: string;
  input?: Record<string, unknown>;
  status?: string;
  terminals: Set<string>;
};
const record = (value: unknown): Record<string, unknown> | undefined =>
  typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
const text = (value: unknown): string | undefined =>
  typeof value === "string" ? value : undefined;

/** ACP v1 terminal references, client snapshots, and agent-owned metadata deltas. */
export function terminalArtifacts(
  items: MessageDetail[],
  streaming: boolean,
): TerminalArtifact[] {
  const tools = new Map<string, ToolState>();
  const terminals = new Map<string, TerminalArtifact>();
  for (const item of items) {
    for (const part of [...item.parts].sort((a, b) => a.ordinal - b.ordinal)) {
      if (part.kind !== "tool_call") continue;
      let update: Record<string, unknown> | undefined;
      try {
        update = record(JSON.parse(part.content_json));
      } catch {
        continue;
      }
      if (!update) continue;
      const toolCallId = text(update.toolCallId);
      if (!toolCallId) continue;
      const tool = tools.get(toolCallId) ?? { terminals: new Set<string>() };
      tool.kind = text(update.kind) ?? tool.kind;
      tool.name = text(update.name) ?? text(update.toolName) ?? tool.name;
      tool.title = text(update.title) ?? text(update.name) ?? tool.title;
      tool.input = record(update.rawInput) ?? tool.input;
      tool.status = text(update.status) ?? tool.status;
      tools.set(toolCallId, tool);
      const meta = record(update._meta);
      const info = record(meta?.terminal_info);
      const snapshot = record(meta?.terminal_output);
      const delta = record(meta?.terminal_output_delta);
      const exit = record(meta?.terminal_exit);
      for (const value of [info, snapshot, delta, exit]) {
        const id = text(value?.terminal_id);
        if (id) tool.terminals.add(id);
      }
      if (Array.isArray(update.content)) {
        for (const value of update.content) {
          const content = record(value);
          if (
            content?.type === "terminal" &&
            typeof content.terminalId === "string"
          )
            tool.terminals.add(content.terminalId);
        }
      }
      for (const terminalId of tool.terminals) {
        const key = `${toolCallId}:${terminalId}`;
        const terminal =
          terminals.get(key) ??
          ({
            key,
            terminalId,
            toolCallId,
            command: "Terminal",
            output: "",
            truncated: false,
            status: "running",
            deferred: false,
          } satisfies TerminalArtifact);
        if (typeof tool.input?.command === "string")
          terminal.command = exactCommand(tool.input.command, tool.input.args);
        else terminal.command = tool.title ?? terminal.command;
        terminal.cwd = text(info?.cwd) ?? text(tool.input?.cwd) ?? terminal.cwd;
        if (snapshot?.terminal_id === terminalId) {
          if (typeof snapshot.data === "string")
            terminal.output = snapshot.data;
          terminal.truncated = snapshot.truncated === true;
        }
        if (delta?.terminal_id === terminalId && typeof delta.data === "string")
          terminal.output += delta.data;
        const raw = record(update.rawOutput);
        const rawText = text(raw?.formatted_output) ?? text(raw?.output);
        if (rawText !== undefined) terminal.output = rawText;
        if (exit?.terminal_id === terminalId) {
          if (typeof exit.exit_code === "number")
            terminal.exitCode = exit.exit_code;
          terminal.signal = text(exit.signal) ?? terminal.signal;
        }
        const rawExit = raw?.exit_code ?? raw?.exitCode;
        if (typeof rawExit === "number") terminal.exitCode = rawExit;
        if (terminal.output.length > 1024 * 1024) {
          terminal.output = terminal.output.slice(-1024 * 1024);
          terminal.truncated = true;
        }
        terminal.deferred ||= update._deferred === true;
        terminal.status =
          tool.status === "cancelled" || item.message.status === "interrupted"
            ? "cancelled"
            : tool.status === "failed" ||
                item.message.status === "failed" ||
                terminal.signal ||
                (terminal.exitCode !== undefined && terminal.exitCode !== 0)
              ? "failed"
              : terminal.exitCode === 0 ||
                  tool.status === "completed" ||
                  !streaming
                ? "completed"
                : "running";
        terminals.set(key, terminal);
      }
    }
  }
  // Some agents attach terminal metadata to non-command tools. Their semantic
  // kind still belongs in reasoning, regardless of how the agent ran the tool.
  const reasoningTools = new Set([
    "read",
    "read_file",
    "list_dir",
    "search",
    "search_replace",
    "grep",
    "edit",
    "write_file",
    "delete",
    "move",
    "fetch",
    "web_search",
    "thinking",
    "think",
  ]);
  return [...terminals.values()].filter((terminal) => {
    const tool = tools.get(terminal.toolCallId);
    return ![tool?.kind, tool?.name].some(
      (value) =>
        value &&
        reasoningTools.has(value.trim().toLowerCase().replaceAll(" ", "_")),
    );
  });
}
