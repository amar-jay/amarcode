import { useEffect, useState } from "react";
import { ChevronDown, LoaderCircle } from "lucide-react";
import type { TerminalArtifact } from "@/lib/terminal-artifacts";
import {
  Terminal,
  TerminalContent,
  TerminalCopyButton,
  TerminalHeader,
  TerminalTitle,
} from "@/components/ai-elements/terminal";

export function TerminalArtifactCard({
  terminal,
  onExpand,
}: {
  terminal: TerminalArtifact;
  onExpand: () => Promise<void>;
}) {
  const [expanded, setExpanded] = useState(terminal.status === "running");
  const [loadFailed, setLoadFailed] = useState(false);
  useEffect(() => {
    if (expanded && terminal.deferred) {
      void onExpand().catch(() => setLoadFailed(true));
    }
  }, [expanded, terminal.deferred, onExpand]);
  const toggle = () => {
    setLoadFailed(false);
    setExpanded(!expanded);
  };
  return (
    <Terminal
      output={terminal.output}
      isStreaming={terminal.status === "running"}
    >
      <TerminalHeader>
        <button
          type="button"
          onClick={toggle}
          aria-expanded={expanded}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
        >
          <TerminalTitle className="min-w-0">
            <span
              className="truncate font-mono text-xs"
              title={terminal.command}
            >
              {terminal.command}
            </span>
          </TerminalTitle>
          <span className="shrink-0 text-xs text-zinc-400">
            {terminal.exitCode !== undefined
              ? `Exit ${terminal.exitCode}`
              : terminal.signal
                ? `Signal ${terminal.signal}`
                : terminal.status}
          </span>
          <ChevronDown
            className={`size-3 shrink-0 transition-transform ${expanded ? "rotate-180" : ""}`}
          />
        </button>
        <TerminalCopyButton disabled={terminal.deferred || !terminal.output} />
      </TerminalHeader>
      {expanded && (
        <>
          {terminal.cwd && (
            <div className="px-4 pt-2 font-mono text-[10px] text-zinc-500">
              {terminal.cwd}
            </div>
          )}
          {loadFailed ? (
            <div className="p-4 text-xs text-zinc-400">
              Could not load output. Collapse and expand to retry.
            </div>
          ) : terminal.deferred ? (
            <div className="flex items-center gap-2 p-4 text-xs text-zinc-400">
              <LoaderCircle className="size-3 animate-spin" />
              Loading output…
            </div>
          ) : terminal.output ? (
            <TerminalContent />
          ) : (
            <div className="p-4 text-xs text-zinc-400">
              {terminal.status === "running"
                ? "Waiting for output…"
                : "No output."}
            </div>
          )}
          {terminal.truncated && (
            <div className="px-4 pb-2 text-xs text-zinc-400">
              Earlier output was truncated.
            </div>
          )}
        </>
      )}
    </Terminal>
  );
}
