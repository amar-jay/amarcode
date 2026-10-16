import { useEffect, useState, useSyncExternalStore } from "react";
import { WorkspaceEditorStore } from "@/lib/workspace-editor-store";
export type { EditorDocument } from "@/lib/workspace-editor-store";

/** Owned above the sheet so drafts and undo history outlive its contents. */
export function useWorkspaceEditor(workspace: string, path?: string) {
  const [store] = useState(() => new WorkspaceEditorStore());
  const documents = useSyncExternalStore(store.subscribe, store.getSnapshot);
  useEffect(() => {
    if (workspace && path) void store.open(workspace, path);
  }, [store, workspace, path]);
  const dirtyDocuments = documents.filter(
    (document) => document.content !== document.original,
  );
  useEffect(() => {
    if (!dirtyDocuments.length) return;
    const warn = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [dirtyDocuments.length]);
  return {
    document: documents.find(
      (document) => document.workspace === workspace && document.path === path,
    ),
    dirtyDocuments,
    load: store.load,
    update: store.update,
    save: store.save,
  };
}
