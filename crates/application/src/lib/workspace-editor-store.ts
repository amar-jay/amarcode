import type { EditorState } from "@codemirror/state";
import { daemonApi } from "@/api";

export interface EditorDocument {
  workspace: string;
  path: string;
  original: string;
  content: string;
  state?: EditorState;
  loading: boolean;
  saving: boolean;
  loaded: boolean;
  generation: number;
  error?: string;
}

const message = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

/** In-memory documents survive the sheet unmounting. State includes undo history. */
export class WorkspaceEditorStore {
  private documents = new Map<string, EditorDocument>();
  private snapshot: EditorDocument[] = [];
  private listeners = new Set<() => void>();
  constructor(private api = daemonApi) {}
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  getSnapshot = () => this.snapshot;
  private emit() {
    this.snapshot = [...this.documents.values()];
    this.listeners.forEach((listener) => listener());
  }
  open = async (workspace: string, path: string) => {
    const key = JSON.stringify([workspace, path]);
    let document = this.documents.get(key);
    if (!document) {
      document = {
        workspace,
        path,
        original: "",
        content: "",
        loading: false,
        saving: false,
        loaded: false,
        generation: 0,
      };
      this.documents.set(key, document);
    }
    await this.load(document);
  };
  load = async (document: EditorDocument, reload = false) => {
    if (document.loading || document.saving || (document.loaded && !reload))
      return;
    document.loading = true;
    document.error = undefined;
    this.emit();
    try {
      const content = await this.api.readWorkspaceFile(
        document.workspace,
        document.path,
      );
      document.original = content;
      document.content = content;
      document.state = undefined;
      document.generation += 1;
      document.loaded = true;
    } catch (error) {
      document.error = message(error);
    } finally {
      document.loading = false;
      this.emit();
    }
  };
  update = (document: EditorDocument, state: EditorState) => {
    document.state = state;
    const content = state.sliceDoc();
    if (content !== document.content) {
      document.content = content;
      this.emit();
    }
  };
  save = async (document: EditorDocument) => {
    if (
      !document.loaded ||
      document.loading ||
      document.saving ||
      document.content === document.original
    )
      return false;
    const content = document.content;
    document.saving = true;
    document.error = undefined;
    this.emit();
    try {
      await this.api.saveWorkspaceFile(
        document.workspace,
        document.path,
        document.original,
        content,
      );
      // Edits made while saving stay dirty against the version just written.
      document.original = content;
      return true;
    } catch (error) {
      document.error = message(error);
      return false;
    } finally {
      document.saving = false;
      this.emit();
    }
  };
}
