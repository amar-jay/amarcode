import { atom } from "jotai";

/** Settings dialog open state (shell chrome, not chat-local). */
export const settingsOpenAtom = atom(false);
export const sidePanelMaximizedAtom = atom(false);
const sidePanelVisibleAtom = atom(false);
/** Closing the workspace restores chat and clears temporary focus mode. */
export const sidePanelOpenAtom = atom(
  (get) => get(sidePanelVisibleAtom),
  (_get, set, open: boolean) => {
    set(sidePanelVisibleAtom, open);
    if (!open) set(sidePanelMaximizedAtom, false);
  },
);
export const sidePanelPinnedAtom = atom(false);

export type WorkspaceFileOpenRequest = {
  path: string;
  line?: number;
};

/** One-shot navigation request from a chat file link to the workspace viewer. */
export const workspaceFileOpenRequestAtom =
  atom<WorkspaceFileOpenRequest | null>(null);
