import { describe, expect, it } from "vitest";
import type { AgentRun } from "@/types";
import { buildAgentRunTimeline } from "./agent-run-timeline";

function run(id: string, start: number, end: number): AgentRun {
  return {
    id,
    chat_id: "chat",
    agent_id: "codex-acp",
    acp_session_id: null,
    status: "completed",
    started_at: new Date(start).toISOString(),
    finished_at: new Date(end).toISOString(),
    error_message: null,
    context_usage: null,
  };
}

describe("agent run timeline", () => {
  it("compresses long inactive gaps without counting them as active time", () => {
    const hour = 60 * 60 * 1_000;
    const timeline = buildAgentRunTimeline(
      [run("first", 0, 10_000), run("resumed", 62 * hour, 62 * hour + 20_000)],
      0,
    )!;

    expect(timeline.activeDuration).toBe(30_000);
    expect(timeline.elapsedDuration).toBe(62 * hour + 20_000);
    expect(timeline.breaks).toHaveLength(1);
    expect(timeline.visualSpan).toBeLessThan(31_000);
    expect(timeline.lanes[0][1][1].visualStart).toBeLessThan(11_000);
  });

  it("preserves short gaps on the scale", () => {
    const timeline = buildAgentRunTimeline(
      [run("first", 0, 10_000), run("next", 20_000, 30_000)],
      0,
    )!;

    expect(timeline.breaks).toHaveLength(0);
    expect(timeline.visualSpan).toBe(30_000);
  });

  it("counts overlapping runs once in active duration", () => {
    const timeline = buildAgentRunTimeline(
      [run("parent", 0, 20_000), run("child", 10_000, 30_000)],
      0,
    )!;

    expect(timeline.activeDuration).toBe(30_000);
  });
});
