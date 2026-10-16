import { describe, expect, it, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import { daemonApi } from "@/api";
import { WorkspaceEditorStore } from "./workspace-editor-store";

function setup() {
  const readWorkspaceFile = vi.fn(async () => "original\r\n");
  const saveWorkspaceFile = vi.fn(async () => {});
  const store = new WorkspaceEditorStore({
    ...daemonApi,
    readWorkspaceFile,
    saveWorkspaceFile,
  });
  return { store, readWorkspaceFile, saveWorkspaceFile };
}
function edit(store: WorkspaceEditorStore, text: string) {
  const document = store.getSnapshot()[0];
  const state = EditorState.create({
    doc: text,
    extensions: EditorState.lineSeparator.of("\r\n"),
  });
  store.update(document, state);
  return document;
}

describe("workspace editor documents", () => {
  it("retains drafts and editor state while switching files and workspaces", async () => {
    const { store, readWorkspaceFile } = setup();
    await store.open("/one", "file.txt");
    const document = edit(store, "draft\r\n");
    const state = document.state;
    await store.open("/two", "file.txt");
    await store.open("/one", "other.txt");
    await store.open("/one", "file.txt");
    expect(readWorkspaceFile).toHaveBeenCalledTimes(3);
    expect(store.getSnapshot()[0]).toBe(document);
    expect(document.content).toBe("draft\r\n");
    expect(document.state).toBe(state);
  });
  it("keeps edits made during an in-flight save dirty and prevents duplicate saves", async () => {
    const { store, saveWorkspaceFile } = setup();
    await store.open("/one", "file.txt");
    const document = edit(store, "first\r\n");
    let finish!: () => void;
    saveWorkspaceFile.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    const saving = store.save(document);
    edit(store, "second\r\n");
    expect(await store.save(document)).toBe(false);
    finish();
    expect(await saving).toBe(true);
    expect(document.original).toBe("first\r\n");
    expect(document.content).toBe("second\r\n");
    expect(saveWorkspaceFile).toHaveBeenCalledExactlyOnceWith(
      "/one",
      "file.txt",
      "original\r\n",
      "first\r\n",
    );
  });
  it("preserves a conflicting draft and clears history only after a successful reload", async () => {
    const { store, readWorkspaceFile, saveWorkspaceFile } = setup();
    await store.open("/one", "file.txt");
    const document = edit(store, "draft");
    saveWorkspaceFile.mockRejectedValueOnce(new Error("changed on disk"));
    expect(await store.save(document)).toBe(false);
    expect(document.error).toBe("changed on disk");
    expect(document.content).toBe("draft");
    readWorkspaceFile.mockRejectedValueOnce(new Error("unavailable"));
    await store.load(document, true);
    expect(document.content).toBe("draft");
    expect(document.state).toBeDefined();
    readWorkspaceFile.mockResolvedValueOnce("agent edit");
    await store.load(document, true);
    expect(document.content).toBe("agent edit");
    expect(document.original).toBe("agent edit");
    expect(document.state).toBeUndefined();
    expect(document.error).toBeUndefined();
  });
});
