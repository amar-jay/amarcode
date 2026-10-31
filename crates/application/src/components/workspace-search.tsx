import { useEffect, useRef, useState } from "react";
import { daemonApi } from "@/api";
import type { WorkspaceSearchResult } from "@/types";
import { Button } from "./ui/button";

export function WorkspaceSearch({
  workspacePath,
  active,
  onSelect,
  mode,
  onModeChange,
  refreshRevision,
  onFindInFile,
}: {
  workspacePath: string;
  active: boolean;
  refreshRevision: number;
  onFindInFile?: () => void;
  onSelect: (path: string, line?: number) => void;
  mode: "files" | "contents";
  onModeChange: (mode: "files" | "contents") => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (active) inputRef.current?.focus();
  }, [active, mode]);
  const [query, setQuery] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [regex, setRegex] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const [response, setResponse] = useState<{
    key: string;
    result?: WorkspaceSearchResult;
    error?: string;
  }>();
  const key = JSON.stringify([
    workspacePath,
    mode,
    query,
    caseSensitive,
    regex,
    refresh,
    refreshRevision,
  ]);
  const current = response?.key === key ? response : undefined;
  const searching = Boolean(query) && !current;

  useEffect(() => {
    if (!active || !workspacePath || !query) return;
    let stale = false;
    const timer = setTimeout(() => {
      const request =
        mode === "contents"
          ? daemonApi.searchWorkspaceContent(
              workspacePath,
              query,
              caseSensitive,
              regex,
            )
          : daemonApi
              .listWorkspaceFiles(workspacePath, query)
              .then((paths): WorkspaceSearchResult => ({
                matches: paths.map((path) => ({ path, line: 0, preview: "" })),
                truncated: false,
                skipped_files: 0,
              }));
      void request
        .then((result) => {
          if (!stale) setResponse({ key, result });
        })
        .catch((error: unknown) => {
          if (!stale)
            setResponse({
              key,
              error: error instanceof Error ? error.message : String(error),
            });
        });
    }, 300);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
  }, [active, workspacePath, mode, query, caseSensitive, regex, key]);

  const groups = new Map<string, WorkspaceSearchResult["matches"]>();
  for (const match of current?.result?.matches ?? []) {
    const matches = groups.get(match.path) ?? [];
    matches.push(match);
    groups.set(match.path, matches);
  }

  return (
    <section
      aria-label="Search workspace"
      className="flex min-h-0 flex-1 flex-col"
    >
      <div className="space-y-1 border-b p-1.5">
        <div className="flex items-center gap-0.5">
          <div
            className="flex min-w-0 items-center"
            role="group"
            aria-label="Workspace search mode"
          >
            <Button
              size="xs"
              className="px-1.5"
              variant={mode === "files" ? "secondary" : "ghost"}
              aria-pressed={mode === "files"}
              onClick={() => onModeChange("files")}
            >
              File names
            </Button>
            <Button
              size="xs"
              className="px-1.5"
              variant={mode === "contents" ? "secondary" : "ghost"}
              aria-pressed={mode === "contents"}
              onClick={() => onModeChange("contents")}
            >
              Contents
            </Button>
            {onFindInFile && (
              <Button
                size="xs"
                className="px-1.5"
                variant="ghost"
                onClick={onFindInFile}
                title="Find and replace in open file (Ctrl/Cmd+F)"
                aria-label="Find and replace in open file"
              >
                In file
              </Button>
            )}
          </div>
          <div className="ml-auto flex shrink-0 items-center gap-0.5">
            {mode === "contents" && (
              <>
                <Button
                  size="icon-xs"
                  variant={caseSensitive ? "secondary" : "ghost"}
                  aria-pressed={caseSensitive}
                  onClick={() => setCaseSensitive((value) => !value)}
                  title="Match case"
                  aria-label="Match case"
                >
                  Aa
                </Button>
                <Button
                  size="icon-xs"
                  variant={regex ? "secondary" : "ghost"}
                  aria-pressed={regex}
                  onClick={() => setRegex((value) => !value)}
                  title="Use regular expression"
                  aria-label="Use regular expression"
                >
                  .*
                </Button>
              </>
            )}
          </div>
        </div>
        <input
          ref={inputRef}
          data-workspace-search
          type="search"
          aria-label={
            mode === "files"
              ? "Search workspace file names"
              : "Search text across workspace"
          }
          placeholder={mode === "files" ? "Find files…" : "Search in files…"}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") setRefresh((value) => value + 1);
          }}
          className="h-6 w-full min-w-0 rounded border bg-background px-2 text-xs outline-none focus-visible:ring-1 focus-visible:ring-ring"
        />
      </div>
      <div
        role="status"
        aria-live="polite"
        className="px-3 py-2 text-xs text-muted-foreground"
      >
        {!query
          ? mode === "files"
            ? "Find files by name or path."
            : "Search text across workspace files."
          : searching
            ? "Searching…"
            : current?.error
              ? current.error
              : mode === "files"
                ? `${groups.size} matching files`
                : `${current?.result?.matches.length ?? 0} matching lines in ${groups.size} files`}
        {current?.result?.truncated && (
          <p className="mt-1">
            Search limit reached. Narrow your query to see more results.
          </p>
        )}
        {Boolean(current?.result?.skipped_files) && (
          <p className="mt-1">
            {current?.result?.skipped_files} files or entries skipped (large,
            binary, non-UTF-8, or unreadable).
          </p>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        {[...groups].map(([path, matches]) => (
          <div key={path} className="mb-2">
            {mode === "files" ? (
              <button
                className="block w-full truncate px-3 py-1 text-left font-mono text-xs hover:bg-muted focus-visible:bg-muted focus-visible:outline-none"
                title={path}
                onClick={() => onSelect(path)}
              >
                {path}
              </button>
            ) : (
              <div
                className="sticky top-0 truncate bg-popover px-3 py-1 font-mono text-xs font-medium"
                title={path}
              >
                {path}
              </div>
            )}
            {mode === "contents" &&
              matches.map((match) => (
                <button
                  key={match.line}
                  className="flex w-full items-baseline gap-2 px-3 py-1 text-left hover:bg-muted focus-visible:bg-muted focus-visible:outline-none"
                  onClick={() => onSelect(path, match.line)}
                  title={`${path}:${match.line}`}
                >
                  <span className="shrink-0 font-mono text-[10px] tabular-nums text-muted-foreground">
                    {match.line}
                  </span>
                  <span className="truncate whitespace-pre font-mono text-xs">
                    {match.preview}
                  </span>
                </button>
              ))}
          </div>
        ))}
      </div>
    </section>
  );
}
