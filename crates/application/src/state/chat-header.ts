import { atom } from "jotai";

/** Portal host for conversation controls in the shared window title bar. */
export const chatHeaderHostAtom = atom<HTMLElement | null>(null);
