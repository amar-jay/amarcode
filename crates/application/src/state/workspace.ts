import { atom } from "jotai";
import { defaultWorkspacePathAtom } from "./preferences";

/** Project folder the home composer / new chats target. */
export const workspacePathAtom = atom("");

/** Launch-time suggestion from the connected daemon; never persisted as a user preference. */
export const daemonWorkspacePathAtom = atom("");
export const effectiveDefaultWorkspacePathAtom = atom(
  (get) => get(defaultWorkspacePathAtom) || get(daemonWorkspacePathAtom),
);
