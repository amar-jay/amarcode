import { describe, expect, it, vi } from "vitest";
import type { MessageDetail, MessagePart } from "@/types";

function toolPart(
  ordinal: number,
  value: Record<string, unknown>,
): MessagePart {
  return {
    message_id: "assistant-1",
    ordinal,
    kind: "tool_call",
    content_json: JSON.stringify(value),
  };
}

describe("assistant tool presentation", () => {
  it("stops animating stale streaming chunks when the turn completes", async () => {
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => undefined,
    });
    const { groupChatBlocks } = await import("./message-parsing");
    const chunk: MessageDetail = {
      message: {
        id: "chunk", chat_id: "chat", agent_run_id: "run",
        role: "assistant", content: "Working", status: "streaming",
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:01Z",
      },
      parts: [], agent_id: "agent",
    };
    const final: MessageDetail = {
      ...chunk,
      message: { ...chunk.message, id: "final", content: "Done",
        status: "complete", updated_at: "2026-01-01T00:00:05Z" },
    };
    const cache: import("./message-parsing").ChatBlockCache = new Map();
    const active = groupChatBlocks([chunk, final], true, false, cache)[0];
    expect(active.kind === "assistant" && active.streaming).toBe(true);
    const finished = groupChatBlocks([chunk, final], false, false, cache)[0];
    expect(finished).toMatchObject({ streaming: false, status: "complete",
      completedAt: final.message.updated_at });
    expect(finished).not.toBe(active);
    const nextPrompt: MessageDetail = {
      ...final, message: { ...final.message, id: "next", role: "user" },
    };
    expect(groupChatBlocks([chunk, final, nextPrompt], true, false)[0])
      .toMatchObject({ streaming: false });
  });

  it("renders structured edits as file changes instead of duplicate edit steps", async () => {
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => undefined,
    });
    const { groupChatBlocks } = await import("./message-parsing");
    const item: MessageDetail = {
      message: {
        id: "assistant-1",
        chat_id: "chat-1",
        agent_run_id: "run-1",
        role: "assistant",
        content: "",
        status: "complete",
        created_at: "2026-01-01T00:00:01Z",
        updated_at: "2026-01-01T00:00:02Z",
      },
      agent_id: "agent-1",
      parts: [
        toolPart(0, {
          sessionUpdate: "tool_call",
          toolCallId: "edit-1",
          name: "search_replace",
          title: "search_replace",
        }),
        toolPart(1, {
          sessionUpdate: "tool_call_update",
          toolCallId: "edit-1",
          kind: "edit",
          title: "Edit `/workspace/src/app.ts`",
          content: [
            {
              type: "diff",
              path: "/workspace/src/app.ts",
              oldText: "const value = 1;",
              newText: "const value = 2;",
            },
          ],
        }),
        toolPart(2, {
          sessionUpdate: "tool_call",
          toolCallId: "read-1",
          kind: "read",
          title: "Read `/workspace/src/app.ts`",
          content: [{ type: "terminal", terminalId: "read-internal-terminal" }],
        }),
      ],
    };

    const [block] = groupChatBlocks([item], false, false);
    expect(block.kind).toBe("assistant");
    if (block.kind !== "assistant") return;

    expect(block.entries.map((entry) => entry.kind)).toEqual([
      "diff",
      "reasoning",
    ]);
    expect(block.timeline.map((step) => step.label)).toEqual([
      "Read `/workspace/src/app.ts`",
    ]);
    expect(block.terminals).toHaveLength(0);
    expect(block.diffs).toHaveLength(1);
    expect(block.diffs[0]).toMatchObject({
      toolCallId: "edit-1",
      title: "File changes",
      changes: [{ path: "/workspace/src/app.ts", operation: "modify" }],
    });
    expect(block.diffs[0].patch).toContain("-const value = 1;");
    expect(block.diffs[0].patch).toContain("+const value = 2;");
  });

  it("shows streamed ACP thinking text at its original position as snapshots update", async () => {
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => undefined,
    });
    const { groupChatBlocks } = await import("./message-parsing");
    const snapshots = [
      toolPart(0, {
        toolCallId: "think-1",
        title: "Thinking",
        kind: "think",
        status: "in_progress",
        content: [
          {
            type: "content",
            content: { type: "text", text: "Inspect the files." },
          },
        ],
      }),
      toolPart(2, {
        toolCallId: "think-1",
        content: [
          {
            type: "content",
            content: {
              type: "text",
              text: "Inspect the files. Then verify the fix.",
            },
          },
        ],
      }),
      toolPart(3, { toolCallId: "think-1", status: "completed" }),
    ];
    for (const verbose of [false, true]) {
      for (const count of [1, 2, 3]) {
        const item: MessageDetail = {
          message: {
            id: "assistant-1",
            chat_id: "chat-1",
            agent_run_id: "run-1",
            role: "assistant",
            content: "Answer",
            status: count === 3 ? "complete" : "streaming",
            created_at: "2026-01-01T00:00:01Z",
            updated_at: "2026-01-01T00:00:02Z",
          },
          agent_id: "agent-1",
          parts: [
            ...snapshots.slice(0, count),
            {
              message_id: "assistant-1",
              ordinal: 1,
              kind: "text",
              content_json: JSON.stringify({ text: "Answer" }),
            } as MessagePart,
          ].reverse(),
        };
        const [block] = groupChatBlocks([item], count !== 3, verbose);
        if (block.kind !== "assistant") throw new Error("Expected assistant");
        expect(block.timeline).toHaveLength(1);
        expect(block.timeline[0]).toMatchObject({
          kind: "thinking",
          label:
            count === 1
              ? "Inspect the files."
              : "Inspect the files. Then verify the fix.",
        });
        expect(block.entries.map((entry) => entry.kind)).toEqual([
          "reasoning",
          "text",
        ]);
        expect(block.content).toBe("Answer");
      }
    }
  });

  it("keeps separate assistant log messages visually divided", async () => {
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => undefined,
    });
    const { groupChatBlocks } = await import("./message-parsing");
    const message = (id: string, content: string): MessageDetail => ({
      message: {
        id,
        chat_id: "chat-1",
        agent_run_id: "run-1",
        role: "assistant",
        content,
        status: "complete",
        created_at: "2026-01-01T00:00:01Z",
        updated_at: "2026-01-01T00:00:02Z",
      },
      agent_id: "agent-1",
      parts: [],
    });

    const [block] = groupChatBlocks(
      [
        message("progress-1", "Checking the implementation."),
        message("final-1", "The fix is complete."),
      ],
      false,
      false,
    );

    expect(block.kind).toBe("assistant");
    if (block.kind !== "assistant") return;
    expect(block.content).toBe(
      "Checking the implementation.\n\n---\n\nThe fix is complete.",
    );
  });
  it("interleaves text, reasoning, terminal and edit snapshots in event order", async () => {
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => undefined,
    });
    const { groupChatBlocks } = await import("./message-parsing");
    const textPart = (
      ordinal: number,
      kind: "text" | "thinking",
      text: string,
    ): MessagePart => ({
      message_id: "assistant-1",
      ordinal,
      kind,
      content_json: JSON.stringify({ text }),
    });
    const item: MessageDetail = {
      message: {
        id: "assistant-1",
        chat_id: "chat-1",
        agent_run_id: "run-1",
        role: "assistant",
        content: "Checking. Done.",
        status: "complete",
        created_at: "2026-01-01T00:00:01Z",
        updated_at: "2026-01-01T00:00:02Z",
      },
      agent_id: "agent-1",
      parts: [
        textPart(0, "text", "Checking."),
        toolPart(1, {
          toolCallId: "terminal-1",
          title: "Read file",
          kind: "execute",
          rawInput: { command: "cat", args: ["file.ts"] },
          content: [{ type: "terminal", terminalId: "term-1" }],
        }),
        textPart(2, "thinking", "Now update the file."),
        toolPart(3, { toolCallId: "edit-1", title: "Edit file", kind: "edit" }),
        textPart(4, "text", "Done."),
        // Completion updates must refresh the existing cards without moving them.
        toolPart(5, {
          toolCallId: "terminal-1",
          status: "completed",
          rawOutput: { exit_code: 0, formatted_output: "file contents" },
        }),
        toolPart(6, {
          toolCallId: "edit-1",
          title: "Edit file",
          kind: "edit",
          content: [
            { type: "diff", path: "file.ts", oldText: "old", newText: "new" },
          ],
        }),
      ].reverse(),
    };
    const [block] = groupChatBlocks([item], false, false);
    if (block.kind !== "assistant") throw new Error("Expected assistant");
    expect(block.entries.map((entry) => entry.kind)).toEqual([
      "text",
      "terminal",
      "reasoning",
      "diff",
      "text",
    ]);
    const terminal = block.entries[1];
    expect(terminal.kind === "terminal" && terminal.terminal.output).toBe(
      "file contents",
    );
    expect(
      block.entries
        .filter((entry) => entry.kind === "text")
        .map((entry) => entry.content),
    ).toEqual(["Checking.", "Done."]);
    const streaming = groupChatBlocks([item], true, false)[0];
    if (streaming.kind !== "assistant") throw new Error("Expected assistant");
    expect(streaming.entries.map((entry) => entry.key)).toEqual(
      block.entries.map((entry) => entry.key),
    );
  });
});

describe("completed conversation block caching", () => {
  it("reuses completed blocks while streaming changes and invalidates them when details change", async () => {
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => undefined,
    });
    const { groupChatBlocks } = await import("./message-parsing");
    const row = (
      id: string,
      run: string,
      role: "user" | "assistant",
      content: string,
    ): MessageDetail => ({
      message: {
        id,
        chat_id: "chat",
        agent_run_id: run,
        role,
        content,
        status: "complete",
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
      parts: [],
      agent_id: "agent",
    });
    const oldUser = row("old-user", "old-run", "user", "First task");
    const oldAnswer = row(
      "old-answer",
      "old-run",
      "assistant",
      "Completed work",
    );
    const user = row("user", "new-run", "user", "Next task");
    const answer = row("answer", "new-run", "assistant", "First chunk");
    const cache: import("./message-parsing").ChatBlockCache = new Map();
    const initial = groupChatBlocks(
      [oldUser, oldAnswer, user, answer],
      true,
      false,
      cache,
    );
    const next = groupChatBlocks(
      [
        oldUser,
        oldAnswer,
        user,
        { ...answer, message: { ...answer.message, content: "Second chunk" } },
      ],
      true,
      false,
      cache,
    );
    expect(next[1]).toBe(initial[1]);
    expect(next[3]).not.toBe(initial[3]);
    const changed = groupChatBlocks(
      [
        oldUser,
        {
          ...oldAnswer,
          message: { ...oldAnswer.message, content: "Loaded full details" },
        },
        user,
        answer,
      ],
      true,
      false,
      cache,
    );
    expect(changed[1]).not.toBe(initial[1]);
    const verbose = groupChatBlocks(
      [oldUser, oldAnswer, user, answer],
      true,
      true,
      cache,
    );
    expect(verbose[1]).not.toBe(initial[1]);
    groupChatBlocks([user, answer], false, true, cache);
    expect(cache.has("old-answer")).toBe(false);
  });
});
