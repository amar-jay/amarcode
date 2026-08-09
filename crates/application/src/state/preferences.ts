import { atom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { parseSessionMode, type SessionMode } from "./session-mode";

export type Theme = "light" | "dark" | "system";
export type Palette = "monochrome" | "ember";

function readTheme(): Theme {
  const stored = localStorage.getItem("amarcode-theme");
  return stored === "light" || stored === "dark" || stored === "system" ? stored : "system";
}

export function readPalette(): Palette {
  const stored = localStorage.getItem("amarcode-palette");
  return stored === "ember" || stored === "monochrome" ? stored : "monochrome";
}

/** User-facing chrome preference. Synced to `document.documentElement` by `useAppBootstrap`. */
export const themeAtom = atomWithStorage<Theme>("amarcode-theme", readTheme(), undefined, {
  getOnInit: true,
});

/** Color style token (`data-style` on `<html>`). */
export const paletteAtom = atomWithStorage<Palette>("amarcode-palette", readPalette(), undefined, {
  getOnInit: true,
});

/** Default agent for new chats / home composer. */
export const defaultAgentIdAtom = atomWithStorage<string>(
  "amarcode-default-agent",
  "codex-acp",
  undefined,
  { getOnInit: true },
);

/** Default plan/build/ask mode for new chats. */
export const defaultSessionModeAtom = atomWithStorage<SessionMode>(
  "amarcode-default-session-mode",
  parseSessionMode(localStorage.getItem("amarcode-default-session-mode")),
  undefined,
  { getOnInit: true },
);

/**
 * When true, reasoning chain shows full tool titles / shell commands and
 * untruncated thought text. Default is compact.
 */
export const verboseReasoningAtom = atomWithStorage<boolean>(
  "amarcode-verbose-reasoning",
  false,
  undefined,
  { getOnInit: true },
);
