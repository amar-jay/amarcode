import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { WorkspaceToolbarButton } from "./workspace-toolbar-button";
import { Compartment, EditorState } from "@codemirror/state";
import {
  EditorView,
  drawSelection,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
  rectangularSelection,
} from "@codemirror/view";
import {
  bracketMatching,
  defaultHighlightStyle,
  foldGutter,
  foldKeymap,
  indentOnInput,
  syntaxHighlighting,
} from "@codemirror/language";
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentWithTab,
} from "@codemirror/commands";
import {
  autocompletion,
  closeBrackets,
  closeBracketsKeymap,
  completionKeymap,
  completeAnyWord,
} from "@codemirror/autocomplete";
import {
  highlightSelectionMatches,
  openSearchPanel,
  searchKeymap,
} from "@codemirror/search";
import { LoaderCircle, RotateCcw, Save, Search } from "lucide-react";
import { languageForPath } from "@/lib/editor-language";
import type {
  EditorDocument,
  useWorkspaceEditor,
} from "@/hooks/use-workspace-editor";
import { Button } from "./ui/button";

const integration = new Compartment();

type Controller = ReturnType<typeof useWorkspaceEditor>;

export function WorkspaceEditor({
  controller,
  selectedLine,
  onSaved,
  toolbarHost,
}: {
  controller: Controller;
  selectedLine?: number;
  onSaved: () => void;
  toolbarHost: HTMLElement | null;
}) {
  const { document, update, save, load } = controller;
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const [confirmReload, setConfirmReload] = useState(false);
  // Key bindings use the latest callbacks without rebuilding the editor.
  const actions = useRef({ save, onSaved });
  useEffect(() => {
    actions.current = { save, onSaved };
  }, [save, onSaved]);

  useEffect(() => {
    if (!host.current || !document?.loaded || document.loading) return;
    const current: EditorDocument = document;
    const generation = current.generation;
    const editor = new EditorView({
      parent: host.current,
      state:
        current.state ??
        EditorState.create({
          doc: current.content,
          extensions: [
            EditorState.lineSeparator.of(
              current.content.includes("\r\n") ? "\r\n" : "\n",
            ),
            EditorState.allowMultipleSelections.of(true),
            EditorView.contentAttributes.of({
              "aria-label": `Edit ${current.path}`,
              spellcheck: "false",
            }),
            lineNumbers(),
            foldGutter(),
            history(),
            drawSelection(),
            rectangularSelection(),
            indentOnInput(),
            bracketMatching(),
            closeBrackets(),
            autocompletion(),
            EditorState.languageData.of(() => [
              { autocomplete: completeAnyWord },
            ]),
            highlightActiveLine(),
            highlightActiveLineGutter(),
            highlightSelectionMatches(),
            syntaxHighlighting(defaultHighlightStyle),
            languageForPath(current.path),
            keymap.of([
              ...closeBracketsKeymap,
              ...defaultKeymap,
              ...searchKeymap,
              ...historyKeymap,
              ...foldKeymap,
              ...completionKeymap,
              indentWithTab,
            ]),
            integration.of([]),
            EditorView.theme({
              "&": {
                height: "100%",
                backgroundColor: "transparent",
                color: "var(--foreground)",
                fontSize: "12px",
              },
              ".cm-scroller": {
                overflow: "auto",
                fontFamily: "var(--font-mono)",
              },
              ".cm-content": { caretColor: "var(--foreground)" },
              ".cm-gutters": {
                backgroundColor: "var(--popover)",
                color: "var(--muted-foreground)",
                borderColor: "var(--border)",
              },
              ".cm-activeLine, .cm-activeLineGutter": {
                backgroundColor:
                  "color-mix(in oklab, var(--muted) 55%, transparent)",
              },
              ".cm-cursor": { borderLeftColor: "var(--foreground)" },
              "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection":
                {
                  backgroundColor:
                    "color-mix(in oklab, var(--primary) 25%, transparent)",
                },
              ".cm-tooltip, .cm-panels": {
                backgroundColor: "var(--popover)",
                color: "var(--foreground)",
                borderColor: "var(--border)",
              },
              ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
                backgroundColor: "var(--accent)",
                color: "var(--accent-foreground)",
              },
              ".cm-textfield, .cm-button": {
                background: "var(--background)",
                color: "var(--foreground)",
                border: "1px solid var(--border)",
              },
              "&.cm-focused": { outline: "none" },
            }),
          ],
        }),
    });
    editor.dispatch({
      effects: integration.reconfigure([
        keymap.of([
          {
            key: "Mod-s",
            run: () => {
              void actions.current.save(current).then((saved) => {
                if (saved) actions.current.onSaved();
              });
              return true;
            },
          },
        ]),
        EditorView.updateListener.of((transaction) =>
          update(current, transaction.state),
        ),
      ]),
    });
    view.current = editor;
    return () => {
      if (current.generation === generation) current.state = editor.state;
      editor.destroy();
      view.current = null;
    };
  }, [
    document,
    document?.loaded,
    document?.loading,
    document?.generation,
    update,
  ]);

  useEffect(() => {
    const editor = view.current;
    if (!editor || !selectedLine) return;
    const position = editor.state.doc.line(
      Math.min(Math.max(selectedLine, 1), editor.state.doc.lines),
    ).from;
    editor.dispatch({
      selection: { anchor: position },
      effects: EditorView.scrollIntoView(position, { y: "center" }),
    });
  }, [
    document,
    document?.loading,
    document?.loaded,
    document?.generation,
    selectedLine,
  ]);

  const dirty = !!document && document.content !== document.original;
  return (
    <section
      className="flex h-full min-h-0 flex-col border-l"
      aria-label="File editor"
    >
      {toolbarHost &&
        createPortal(
          <>
            <WorkspaceToolbarButton
              label="Find and replace"
              disabled={!document?.loaded || document.loading}
              onClick={() => {
                if (view.current) openSearchPanel(view.current);
              }}
            >
              <Search className="size-3.5" />
            </WorkspaceToolbarButton>
            <WorkspaceToolbarButton
              label="Reload file from disk"
              disabled={!document || document.loading || document.saving}
              onClick={() => {
                if (!document) return;
                if (dirty) setConfirmReload(true);
                else void load(document, true);
              }}
            >
              <RotateCcw className="size-3.5" />
            </WorkspaceToolbarButton>
            <WorkspaceToolbarButton
              label={document?.saving ? "Saving…" : "Save file (Ctrl/Cmd+S)"}
              disabled={!dirty || document?.saving || document?.loading}
              onClick={() => {
                if (document)
                  void save(document).then((saved) => {
                    if (saved) onSaved();
                  });
              }}
            >
              {document?.saving ? (
                <LoaderCircle className="size-3.5 animate-spin" />
              ) : (
                <Save className="size-3.5" />
              )}
            </WorkspaceToolbarButton>
          </>,
          toolbarHost,
        )}
      {confirmReload && (
        <div
          className="flex flex-wrap items-center gap-2 border-b px-3 py-2 text-xs"
          role="alert"
        >
          <span className="mr-auto">
            Discard this draft and reload the file?
          </span>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => setConfirmReload(false)}
          >
            Keep editing
          </Button>
          <Button
            size="sm"
            variant="destructive"
            onClick={() => {
              setConfirmReload(false);
              if (document) void load(document, true);
            }}
          >
            Discard and reload
          </Button>
        </div>
      )}
      {document?.error && (
        <div
          role="alert"
          className="border-b px-3 py-2 text-xs text-destructive"
        >
          {document.error}
        </div>
      )}
      {!document || document.loading ? (
        <div className="flex flex-1 items-center justify-center gap-2 text-xs text-muted-foreground">
          <LoaderCircle className="size-4 animate-spin" />
          Loading file
        </div>
      ) : !document.loaded ? (
        <div className="flex flex-1 items-center justify-center">
          <Button size="sm" variant="ghost" onClick={() => void load(document)}>
            Retry
          </Button>
        </div>
      ) : null}
      <div ref={host} className="min-h-0 flex-1 overflow-hidden" />
    </section>
  );
}
