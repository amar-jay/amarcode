import { describe, expect, it } from "vitest";
import type { MessageDetail } from "@/types";
import { terminalArtifacts } from "./terminal-artifacts";

function message(
  updates: Record<string, unknown>[],
  status: MessageDetail["message"]["status"] = "streaming",
): MessageDetail {
  return {
    message: {
      id: "message",
      chat_id: "chat",
      agent_run_id: "run",
      role: "assistant",
      content: "",
      status,
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:01Z",
    },
    agent_id: "agent",
    parts: updates.map((update, ordinal) => ({
      message_id: "message",
      ordinal,
      kind: "tool_call",
      content_json: JSON.stringify(update),
    })),
  };
}
const start = {
  sessionUpdate: "tool_call",
  toolCallId: "command",
  title: "Run echo",
  kind: "execute",
  status: "in_progress",
  rawInput: { command: "echo", args: ["hello world"] },
  content: [{ type: "terminal", terminalId: "terminal" }],
};
const update = (meta: Record<string, unknown>) => ({
  sessionUpdate: "tool_call_update",
  toolCallId: "command",
  _meta: meta,
});

describe("terminal artifacts", () => {
  it("replaces client snapshots and retains the exact command and final exit", () => {
    const [terminal] = terminalArtifacts(
      [
        message(
          [
            start,
            update({
              terminal_output: {
                terminal_id: "terminal",
                data: "first",
                truncated: false,
              },
            }),
            update({
              terminal_info: { terminal_id: "terminal", cwd: "/workspace" },
              terminal_output: {
                terminal_id: "terminal",
                data: "first\nlast",
                truncated: true,
              },
              terminal_exit: { terminal_id: "terminal", exit_code: 0 },
            }),
            {
              sessionUpdate: "tool_call_update",
              toolCallId: "command",
              status: "completed",
            },
          ],
          "complete",
        ),
      ],
      false,
    );
    expect(terminal).toMatchObject({
      command: "echo 'hello world'",
      cwd: "/workspace",
      output: "first\nlast",
      truncated: true,
      exitCode: 0,
      status: "completed",
    });
  });

  it("accumulates legacy metadata deltas without requiring update titles", () => {
    const [terminal] = terminalArtifacts(
      [
        message([
          start,
          update({
            terminal_output_delta: { terminal_id: "terminal", data: "first\n" },
          }),
          update({
            terminal_output_delta: { terminal_id: "terminal", data: "last\n" },
          }),
          update({ terminal_exit: { terminal_id: "terminal", exit_code: 2 } }),
        ]),
      ],
      true,
    );
    expect(terminal.output).toBe("first\nlast\n");
    expect(terminal.status).toBe("failed");
    expect(terminal.exitCode).toBe(2);
  });

  it("keeps deferred terminal summaries visible on chat reload", () => {
    const [terminal] = terminalArtifacts(
      [
        message(
          [
            {
              ...start,
              _deferred: true,
              status: "completed",
              _meta: {
                terminal_info: { terminal_id: "terminal", cwd: "/workspace" },
                terminal_exit: { terminal_id: "terminal", exit_code: 0 },
                terminal_output: { terminal_id: "terminal", truncated: false },
              },
            },
          ],
          "complete",
        ),
      ],
      false,
    );
    expect(terminal).toMatchObject({
      deferred: true,
      output: "",
      exitCode: 0,
      command: "echo 'hello world'",
    });
  });

  it("does not turn every execute tool into a terminal and marks interrupted commands", () => {
    expect(
      terminalArtifacts(
        [
          message([
            { toolCallId: "unrelated", title: "Execute", kind: "execute" },
          ]),
        ],
        true,
      ),
    ).toEqual([]);
    expect(
      terminalArtifacts([message([start], "interrupted")], false)[0].status,
    ).toBe("cancelled");
  });
});
