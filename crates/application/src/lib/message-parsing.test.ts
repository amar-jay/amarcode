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
        }),
      ],
    };

    const [block] = groupChatBlocks([item], false, false);
    expect(block.kind).toBe("assistant");
    if (block.kind !== "assistant") return;

    expect(block.timeline.map((step) => step.label)).toEqual([
      "Read `/workspace/src/app.ts`",
    ]);
    expect(block.diffs).toHaveLength(1);
    expect(block.diffs[0]).toMatchObject({
      toolCallId: "edit-1",
      title: "File changes",
      changes: [{ path: "/workspace/src/app.ts", operation: "modify" }],
    });
    expect(block.diffs[0].patch).toContain("-const value = 1;");
    expect(block.diffs[0].patch).toContain("+const value = 2;");
  });
});
