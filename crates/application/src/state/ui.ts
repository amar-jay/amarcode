import { atom } from "jotai";

/** Settings dialog open state (shell chrome, not chat-local). */
export const settingsOpenAtom = atom(false);
export const sidePanelOpenAtom = atom(false);

export type WorkspaceFileOpenRequest = {
  path: string;
  line?: number;
};

/** One-shot navigation request from a chat file link to the workspace viewer. */
export const workspaceFileOpenRequestAtom =
  atom<WorkspaceFileOpenRequest | null>(null);
