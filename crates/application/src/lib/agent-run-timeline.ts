import type { AgentRun } from "@/types";

export type TimelineRun = {
  run: AgentRun;
  start: number;
  end: number;
  visualStart: number;
  visualEnd: number;
};

export type TimelineBreak = {
  position: number;
  duration: number;
};

export function buildAgentRunTimeline(runs: AgentRun[], now: number) {
  const parsed = runs
    .map((run) => ({
      run,
      start: Date.parse(run.started_at),
      end: run.finished_at ? Date.parse(run.finished_at) : now,
    }))
    .filter(({ start, end }) => Number.isFinite(start) && Number.isFinite(end))
    .map((item) => ({ ...item, end: Math.max(item.start, item.end) }))
    .sort((left, right) => left.start - right.start);

  if (!parsed.length) return null;

  const windows: Array<{ start: number; end: number }> = [];
  for (const item of parsed) {
    const current = windows.at(-1);
    if (!current || item.start > current.end) {
      windows.push({ start: item.start, end: item.end });
    } else {
      current.end = Math.max(current.end, item.end);
    }
  }

  const activeDuration = windows.reduce(
    (total, window) => total + window.end - window.start,
    0,
  );
  const elapsedDuration = Math.max(
    windows.at(-1)!.end - windows[0].start,
    1,
  );
  // A pause much longer than a typical run is a resumption, not useful scale.
  const averageRunDuration =
    parsed.reduce((total, item) => total + item.end - item.start, 0) /
    parsed.length;
  const breakThreshold = Math.max(60_000, averageRunDuration * 5);
  const compressedGap = Math.max(activeDuration * 0.015, 1);

  let visualCursor = 0;
  const visualWindows = windows.map((window, index) => {
    if (index > 0) {
      const actualGap = window.start - windows[index - 1].end;
      visualCursor +=
        actualGap > breakThreshold ? compressedGap : actualGap;
    }
    const visualStart = visualCursor;
    visualCursor += window.end - window.start;
    return { ...window, visualStart, visualEnd: visualCursor };
  });
  const visualSpan = Math.max(visualCursor, 1);

  const project = (timestamp: number) => {
    const index = visualWindows.findIndex(
      (window) => timestamp >= window.start && timestamp <= window.end,
    );
    const window = visualWindows[Math.max(index, 0)];
    return window.visualStart + timestamp - window.start;
  };

  const timelineRuns: TimelineRun[] = parsed.map((item) => ({
    ...item,
    visualStart: project(item.start),
    visualEnd: project(item.end),
  }));
  const lanes = new Map<string, TimelineRun[]>();
  for (const item of timelineRuns) {
    const lane = lanes.get(item.run.agent_id) ?? [];
    lane.push(item);
    lanes.set(item.run.agent_id, lane);
  }

  const breaks: TimelineBreak[] = windows.slice(1).flatMap((window, index) => {
    const duration = window.start - windows[index].end;
    if (duration <= breakThreshold) return [];
    const position = visualWindows[index].visualEnd + compressedGap / 2;
    return [{ position: (position / visualSpan) * 100, duration }];
  });

  return {
    activeDuration,
    elapsedDuration,
    visualSpan,
    lanes: [...lanes.entries()],
    breaks,
  };
}
