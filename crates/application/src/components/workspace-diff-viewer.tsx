import { useEffect, useMemo, useRef, useState } from "react";
import { EditorState, type Extension } from "@codemirror/state";
import {
  defaultHighlightStyle,
  syntaxHighlighting,
} from "@codemirror/language";
import { unifiedMergeView } from "@codemirror/merge";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { html } from "@codemirror/lang-html";
import { css } from "@codemirror/lang-css";
import { markdown } from "@codemirror/lang-markdown";
import { rust } from "@codemirror/lang-rust";
import { python } from "@codemirror/lang-python";
import { yaml } from "@codemirror/lang-yaml";
import { sql } from "@codemirror/lang-sql";
import { lineNumbers, EditorView } from "@codemirror/view";
import { FileDiff, LoaderCircle } from "lucide-react";
import { daemonApi, type WorkspaceDiff } from "@/api";

function languageForPath(path: string): Extension {
  const extension = path.split(".").pop()?.toLowerCase();
  switch (extension) {
    case "js":
    case "jsx":
    case "ts":
    case "tsx":
      return javascript({ jsx: true, typescript: extension.startsWith("ts") });
    case "json":
    case "jsonc":
      return json();
    case "html":
    case "htm":
      return html();
    case "css":
    case "scss":
      return css();
    case "md":
    case "mdx":
      return markdown();
    case "rs":
      return rust();
    case "py":
      return python();
    case "yaml":
    case "yml":
      return yaml();
    case "sql":
      return sql();
    default:
      return [];
  }
}

const readOnlyExtensions = (path: string): Extension[] => [
  lineNumbers(),
  EditorState.readOnly.of(true),
  EditorView.editable.of(false),
  EditorView.lineWrapping,
  syntaxHighlighting(defaultHighlightStyle),
  languageForPath(path),
  EditorView.theme(
    {
      "&": {
        backgroundColor: "transparent",
        color: "var(--foreground)",
        fontSize: "12px",
      },
      ".cm-content": {
        fontFamily: "var(--font-mono)",
        caretColor: "transparent",
      },
      ".cm-gutters": {
        backgroundColor: "transparent",
        borderRight: "1px solid var(--border)",
        color: "var(--muted-foreground)",
      },
      ".cm-activeLineGutter": { backgroundColor: "transparent" },
    },
    { dark: true },
  ),
];

function DiffCanvas({ diff }: { diff: WorkspaceDiff }) {
  const host = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!host.current) return;
    const editor = new EditorView({
      state: EditorState.create({
        doc: diff.after,
        extensions: [
          ...readOnlyExtensions(diff.path),
          ...(diff.before === diff.after
            ? []
            : unifiedMergeView({
                allowInlineDiffs: true,
                collapseUnchanged: { margin: 3, minSize: 8 },
                gutter: true,
                highlightChanges: true,
                mergeControls: false,
                original: diff.before,
                syntaxHighlightDeletions: true,
              })),
        ],
      }),
      parent: host.current,
    });
    return () => editor.destroy();
  }, [diff]);

  return <div className="min-h-0 flex-1 overflow-auto" ref={host} />;
}

export function WorkspaceDiffViewer({
  selectedPath,
  workspacePath,
  diff,
  setDiff,
}: {
  selectedPath?: string;
  workspacePath: string;
  diff?: WorkspaceDiff;
  setDiff: (diff?: WorkspaceDiff) => void;
}) {
  const [error, setError] = useState<string>();
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    setDiff(undefined);
    setError(undefined);
    if (!workspacePath || !selectedPath) return;

    let cancelled = false;
    setLoading(true);
    void daemonApi
      .getWorkspaceFileDiff(workspacePath, selectedPath)
      .then((nextDiff) => {
        if (!cancelled) setDiff(nextDiff);
      })
      .catch((cause) => {
        if (!cancelled) {
          setError(
            cause instanceof Error
              ? cause.message
              : "Could not load this diff.",
          );
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedPath, workspacePath]);

  const title = useMemo(() => selectedPath ?? "Select a file", [selectedPath]);

  return (
    <section
      className="flex min-w-0 flex-1 flex-col border-l"
      aria-label="Uncommitted file diff"
    >
      {/* <header className="flex h-10 shrink-0 items-center gap-2 border-b px-3">
        <FileDiff className="size-4 text-muted-foreground" />
        <span className="truncate font-mono text-xs" title={title}>
          {title}
        </span>
        {diff && (
          <span className="ml-auto text-[0.65rem] font-medium tracking-wide text-muted-foreground uppercase">
            {diff.comparison} · {diff.status}
          </span>
        )}
      </header> */}
      {loading ? (
        <div className="flex flex-1 items-center justify-center gap-2 text-xs text-muted-foreground">
          <LoaderCircle className="size-4 animate-spin" /> Loading diff
        </div>
      ) : diff ? (
        <DiffCanvas diff={diff} />
      ) : (
        <div className="flex flex-1 items-center justify-center px-8 text-center text-xs text-muted-foreground">
          {error ?? "Select a file to preview it here."}
        </div>
      )}
    </section>
  );
}
