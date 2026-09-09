import { atom } from "jotai";
import type { AgentInfo, Chat } from "@/types";
import { chatsAtom } from "./chats";
import { selectedAgentAtom, selectAgentAtom } from "./agents";
import { defaultWorkspacePathAtom } from "./preferences";
import { workspacePathAtom } from "./workspace";

/**
 * Which surface the main pane shows.
 * `null` → home composer; otherwise the live chat for `chat`.
 */
export type ActiveSession = {
  chat: Chat;
  agent?: AgentInfo;
  /** Run to attach on first paint (usually null; events fill it in). */
  initialRunId: string | null;
  /** Home composer just kicked off a prompt for this chat. */
  initialTurnActive?: boolean;
};

export const activeSessionAtom = atom<ActiveSession | null>(null);

/** Open an existing chat from the sidebar. */
export const selectChatAtom = atom(null, (get, set, chatId: string) => {
  const chat = get(chatsAtom).find((item) => item.id === chatId);
  if (!chat) return;
  set(activeSessionAtom, {
    chat,
    agent: get(selectedAgentAtom),
    initialRunId: null,
    initialTurnActive: false,
  });
  if (chat.workspace_path) set(workspacePathAtom, chat.workspace_path);
});

/** Return to the home composer. */
export const startNewChatAtom = atom(null, (get, set) => {
  set(activeSessionAtom, null);
  set(workspacePathAtom, get(defaultWorkspacePathAtom));
});

/** After home composer creates a chat + fires a prompt. */
export const openStartedChatAtom = atom(
  null,
  (
    _get,
    set,
    payload: {
      chat: Chat;
      agent: AgentInfo;
    },
  ) => {
    set(selectAgentAtom, payload.agent);
    set(activeSessionAtom, {
      chat: payload.chat,
      agent: payload.agent,
      initialRunId: null,
      initialTurnActive: true,
    });
  },
);

/** Change workspace from home; clears the open chat so folders don't mix. */
export const setWorkspacePathAtom = atom(null, (_get, set, path: string) => {
  set(workspacePathAtom, path);
  set(activeSessionAtom, null);
});

/** Keep the open session's agent in sync when the composer agent changes. */
export const bindSessionAgentAtom = atom(null, (get, set, agent: AgentInfo) => {
  set(selectAgentAtom, agent);
  const session = get(activeSessionAtom);
  if (session) set(activeSessionAtom, { ...session, agent });
});
