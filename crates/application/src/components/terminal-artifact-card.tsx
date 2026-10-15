import { useEffect, useState } from "react";
import {
  CheckCircle,
  ChevronDown,
  CircleStop,
  CircleX,
  LoaderCircle,
} from "lucide-react";
import type { TerminalArtifact } from "@/lib/terminal-artifacts";
import {
  Terminal,
  TerminalContent,
  TerminalHeader,
  TerminalTitle,
} from "@/components/ai-elements/terminal";
import { cn } from "@/lib/utils";

export function TerminalArtifactCard({
  terminal,
  onExpand,
}: {
  terminal: TerminalArtifact;
  onExpand: () => Promise<void>;
}) {
  const [userExpanded, setUserExpanded] = useState<boolean>();
  const expanded = userExpanded ?? terminal.status === "running";
  const [loadFailed, setLoadFailed] = useState(false);
  useEffect(() => {
    if (expanded && terminal.deferred) {
      void onExpand().catch(() => setLoadFailed(true));
    }
  }, [expanded, terminal.deferred, onExpand]);
  const toggle = () => {
    setLoadFailed(false);
    setUserExpanded(!expanded);
  };
  const StatusIcon =
    terminal.status === "completed"
      ? CheckCircle
      : terminal.status === "failed"
        ? CircleX
        : terminal.status === "cancelled"
          ? CircleStop
          : LoaderCircle;
  return (
    <Terminal
      output={terminal.output}
      isStreaming={terminal.status === "running"}
    >
      <TerminalHeader
        className={
          cn("bg-secondary/25",
						expanded && terminal.status == "failed" ? "bg-red-500/20" : "",
						expanded && terminal.status == "completed" ? "bg-green-500/20" : "",
						expanded && terminal.status == "cancelled" ? "bg-yellow-500/20" : ""
					)
        }
      >
        <button
          type="button"
          onClick={toggle}
          aria-expanded={expanded}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
        >
          <TerminalTitle className="min-w-0 flex-1 text-zinc-300 [&>svg]:shrink-0">
            <span
              className="min-w-0 truncate font-mono text-xs mr-4"
              title={terminal.command}
            >
              {terminal.command}
            </span>
          </TerminalTitle>
					
          <span
            className="shrink-0 text-zinc-400"
            title={terminal.status}
            aria-label={terminal.status}
          >
            <StatusIcon
              className={cn(
                "size-3.5",
                terminal.status === "running" && "animate-spin",
                terminal.status === "completed" && "text-green-500",
                terminal.status === "failed" && "text-red-500",
                terminal.status === "cancelled" && "text-yellow-500",
              )}
              aria-hidden="true"
            />
          </span>
          <ChevronDown
            className={`size-3 shrink-0 transition-transform ${expanded ? "rotate-180" : ""}`}
          />
        </button>
      </TerminalHeader>
      {expanded && (
        <>
          <div className="border-b border-zinc-800 px-4 py-3">
            <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words font-mono text-xs leading-relaxed [overflow-wrap:anywhere]">
              {terminal.command}
            </pre>
          </div>
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
