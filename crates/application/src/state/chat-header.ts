import { atom } from "jotai";

/** Portal host for conversation controls in the shared window title bar. */
export const chatHeaderHostAtom = atom<HTMLElement | null>(null);

/**
 * Whether the chat find-in-chat UI currently occupies the title bar.
 * The centered run-status pill lives in the same band, so readers hide it
 * while search is open instead of painting over the controls.
 */
export const chatSearchVisibleAtom = atom(false);
