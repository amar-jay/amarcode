import { useState } from "react";
import { ChevronRight } from "lucide-react";
import type { MessagePart } from "@/types";

type DiffChange = {
  operation: "create" | "modify" | "delete";
  path: string;
};

function asRecord(value: unknown): Record<string, unknown> | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  return value as Record<string, unknown>;
}

function pathsFromPatch(patch: string): string[] {
  const paths = new Set<string>();
  for (const line of patch.split("\n")) {
    const match = /^diff --git a\/(.+) b\/(.+)$/.exec(line);
    if (match) {
      paths.add(match[2]);
      continue;
    }
    const fileMarker = /^\+\+\+ (?:b\/)?(.+)$/.exec(line);
    if (fileMarker && fileMarker[1] !== "/dev/null") paths.add(fileMarker[1]);
  }
  return [...paths];
}

/**
 * Minimal line LCS → unified-diff body. Good enough for chat previews;
 * clients like Zed own full editor diffs.
 */
function unifiedFromTexts(path: string, oldText: string | null | undefined, newText: string): string {
  const fileName = path.split(/[/\\]/).pop() || path;
  const oldLines = oldText == null ? [] : oldText.split("\n");
  const newLines = newText.split("\n");

  // Drop a single trailing empty segment so split("a\n") behaves like editors.
  if (oldLines.length > 0 && oldLines[oldLines.length - 1] === "") oldLines.pop();
  if (newLines.length > 0 && newLines[newLines.length - 1] === "") newLines.pop();

  const header = [
    `diff --git a/${fileName} b/${fileName}`,
    oldText == null ? "--- /dev/null" : `--- a/${fileName}`,
    `+++ b/${fileName}`,
  ];

  if (oldText == null) {
    return [
      ...header,
      `@@ -0,0 +1,${newLines.length || 1} @@`,
      ...newLines.map((line) => `+${line}`),
    ].join("\n");
  }

  if (newText === "" && oldLines.length > 0) {
    return [
      ...header.slice(0, 1),
      `--- a/${fileName}`,
      "+++ /dev/null",
      `@@ -1,${oldLines.length} +0,0 @@`,
      ...oldLines.map((line) => `-${line}`),
    ].join("\n");
  }

  // LCS table
  const m = oldLines.length;
  const n = newLines.length;
  const dp: number[][] = Array.from({ length: m + 1 }, () => Array(n + 1).fill(0));
  for (let i = m - 1; i >= 0; i--) {
    for (let j = n - 1; j >= 0; j--) {
      dp[i][j] =
        oldLines[i] === newLines[j]
          ? dp[i + 1][j + 1] + 1
          : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }

  type HunkLine = { kind: " " | "+" | "-"; text: string };
  const body: HunkLine[] = [];
  let i = 0;
  let j = 0;
  while (i < m && j < n) {
    if (oldLines[i] === newLines[j]) {
      body.push({ kind: " ", text: oldLines[i] });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      body.push({ kind: "-", text: oldLines[i] });
      i++;
    } else {
      body.push({ kind: "+", text: newLines[j] });
      j++;
    }
  }
  while (i < m) {
    body.push({ kind: "-", text: oldLines[i++] });
  }
  while (j < n) {
    body.push({ kind: "+", text: newLines[j++] });
  }

  const oldCount = body.filter((line) => line.kind !== "+").length;
  const newCount = body.filter((line) => line.kind !== "-").length;
  return [
    ...header,
    `@@ -1,${oldCount || 1} +1,${newCount || 1} @@`,
    ...body.map((line) => `${line.kind}${line.text}`),
  ].join("\n");
}

function operationFor(oldText: unknown, newText: unknown): DiffChange["operation"] {
  if (oldText == null || oldText === "") return "create";
  if (typeof newText === "string" && newText === "") return "delete";
  return "modify";
}

function extractPatch(diff: Record<string, unknown>): string | null {
  // Official ACP: path + oldText + newText (client synthesizes the display).
  if (typeof diff.newText === "string") {
    const oldText =
      diff.oldText == null
        ? null
        : typeof diff.oldText === "string"
          ? diff.oldText
          : null;
    const path = typeof diff.path === "string" ? diff.path : "file";
    return unifiedFromTexts(path, oldText, diff.newText);
  }

  // Agent extension: prebuilt git patch as string or { text }.
  if (typeof diff.patch === "string" && diff.patch.trim()) return diff.patch;
  const patchRecord = asRecord(diff.patch);
  if (patchRecord && typeof patchRecord.text === "string" && patchRecord.text.trim()) {
    return patchRecord.text;
  }
  return null;
}

function extractChanges(diff: Record<string, unknown>): DiffChange[] {
  if (Array.isArray(diff.changes)) {
    const fromList = diff.changes.flatMap((change): DiffChange[] => {
      const value = asRecord(change);
      if (!value || typeof value.path !== "string") return [];
      const operation =
        value.operation === "create" || value.operation === "delete" || value.operation === "modify"
          ? value.operation
          : operationFor(value.oldText, value.newText);
      return [{ path: value.path, operation }];
    });
    if (fromList.length > 0) return fromList;
  }

  if (typeof diff.path === "string") {
    return [{ path: diff.path, operation: operationFor(diff.oldText, diff.newText) }];
  }
  return [];
}

export type DiffArtifact = {
  key: string;
  title: string;
  changes: DiffChange[];
  patch: string | null;
};

/**
 * ACP delivers edit previews as `content: [{ type: "diff", path, oldText, newText }]`
 * on a tool_call / tool_call_update. The daemon stores the raw update payload in a
 * `tool_call` part — decode at the presentation boundary.
 *
 * Also tolerates agent extensions: `changes[]`, string/`{text}` git patches.
 */
export function diffArtifacts(parts: MessagePart[]): DiffArtifact[] {
  // tool_call + tool_call_update each append a part; keep the latest snapshot
  // per (toolCallId, content index) so the card doesn't stack mid-stream.
  const byKey = new Map<string, DiffArtifact>();

  const toolParts = parts
    .filter((part) => part.kind === "tool_call")
    .slice()
    .sort((a, b) => a.ordinal - b.ordinal);

  for (const part of toolParts) {
    try {
      const payload: unknown = JSON.parse(part.content_json);
      const tool = asRecord(payload);
      if (!tool) continue;

      const content = Array.isArray(tool.content) ? tool.content : [];
      const toolCallId =
        typeof tool.toolCallId === "string"
          ? tool.toolCallId
          : typeof tool.tool_call_id === "string"
            ? tool.tool_call_id
            : `part-${part.ordinal}`;
      const title =
        typeof tool.title === "string" && tool.title.trim()
          ? tool.title
          : "File changes";

      content.forEach((item, index) => {
        const diff = asRecord(item);
        if (!diff || diff.type !== "diff") return;

        let changes = extractChanges(diff);
        const patch = extractPatch(diff);
        if (changes.length === 0 && patch) {
          changes = pathsFromPatch(patch).map((path) => ({
            path,
            operation: "modify" as const,
          }));
        }

        // Nothing useful to show.
        if (changes.length === 0 && !patch) return;

        const key = `${toolCallId}:${index}`;
        byKey.set(key, { key, title, changes, patch });
      });
    } catch {
      // Malformed tool payload must never break the conversation.
    }
  }

  return [...byKey.values()];
}

export function DiffArtifactCard({ artifact }: { artifact: DiffArtifact }) {
  const [expanded, setExpanded] = useState(false);
  const changedFiles = artifact.changes.length;
  const additionCount =
    artifact.patch?.split("\n").filter((line) => line.startsWith("+") && !line.startsWith("+++"))
      .length ?? 0;
  const deletionCount =
    artifact.patch?.split("\n").filter((line) => line.startsWith("-") && !line.startsWith("---"))
      .length ?? 0;
  const summary =
    changedFiles > 0
      ? `${changedFiles} ${changedFiles === 1 ? "file" : "files"} changed`
      : artifact.patch
        ? "Patch preview"
        : "File changes";

  return (
    <section className="mt-3 overflow-hidden rounded-lg border border-border/80 bg-muted/25">
      <button
        type="button"
        className="flex w-full flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-left text-xs transition-colors hover:bg-muted/50"
        aria-expanded={expanded}
        aria-label={`${expanded ? "Collapse" : "Expand"} ${artifact.title.toLowerCase()}`}
        onClick={() => setExpanded((value) => !value)}
      >
        <ChevronRight
          className={`size-3.5 shrink-0 transition-transform ${expanded ? "rotate-90" : ""}`}
          aria-hidden
        />
        <span className="font-medium">{artifact.title}</span>
        <span className="text-muted-foreground">{summary}</span>
        {artifact.patch && (
          <span className="font-mono text-muted-foreground">
            <span className="text-emerald-600 dark:text-emerald-400">+{additionCount}</span>{" "}
            <span className="text-red-600 dark:text-red-400">−{deletionCount}</span>
          </span>
        )}
      </button>
      {expanded && artifact.changes.length > 0 && (
        <div className="border-t border-border/60 px-3 py-2 text-xs text-muted-foreground">
          {artifact.changes.map((change) => (
            <div key={`${change.operation}-${change.path}`} className="font-mono">
              <span className="text-muted-foreground/80">{change.operation}</span> {change.path}
            </div>
          ))}
        </div>
      )}
      {expanded && artifact.patch && (
        <pre className="max-h-80 overflow-auto border-t border-border/60 bg-background/60 p-3 font-mono text-xs leading-relaxed">
          {artifact.patch.split("\n").map((line, index) => (
            <span
              key={index}
              className={
                line.startsWith("+") && !line.startsWith("+++")
                  ? "block bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
                  : line.startsWith("-") && !line.startsWith("---")
                    ? "block bg-red-500/10 text-red-700 dark:text-red-300"
                    : "block text-muted-foreground"
              }
            >
              {line || " "}
            </span>
          ))}
        </pre>
      )}
    </section>
  );
}
