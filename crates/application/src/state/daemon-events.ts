import { atom, getDefaultStore } from "jotai";
import { daemonApi } from "@/api";
import type { EditorEvent, TurnStatus } from "@/types";

/**
 * Latest daemon event (unfiltered stream). Components that care about a
 * specific chat should filter by id; don't fork a second subscription.
 */
export const lastDaemonEventAtom = atom<EditorEvent | null>(null);

export type TurnSnapshot = {
  run_id: string;
  user_message_id: string;
  status: TurnStatus;
  stop_reason: string | null;
  error_message: string | null;
};

/** Latest turnUpdated payload per chat — survives remount / last-event overwrite. */
export const latestTurnByChatAtom = atom<Record<string, TurnSnapshot>>({});

let streamStarted = false;

function rememberTurn(store: ReturnType<typeof getDefaultStore>, event: EditorEvent) {
  if (event.type !== "turnUpdated") return;
  const prev = store.get(latestTurnByChatAtom);
  store.set(latestTurnByChatAtom, {
    ...prev,
    [event.payload.chat_id]: {
      run_id: event.payload.run_id,
      user_message_id: event.payload.user_message_id,
      status: event.payload.status,
      stop_reason: event.payload.stop_reason,
      error_message: event.payload.error_message,
    },
  });
}

/**
 * Start the shared daemon event stream once. Safe under React Strict Mode —
 * only the first call opens the subscription; later calls no-op.
 */
export function ensureDaemonEventStream() {
  if (streamStarted) return;
  streamStarted = true;
  const store = getDefaultStore();
  void daemonApi
    .subscribeEvents({}, (event) => {
      rememberTurn(store, event);
      store.set(lastDaemonEventAtom, event);
    })
    .catch((error: unknown) => {
      console.error("daemon event stream disconnected", error);
    });
}

/** Snapshot of the last turnUpdated for a chat, if any were observed this session. */
export function getLatestTurnForChat(chatId: string): TurnSnapshot | null {
  return getDefaultStore().get(latestTurnByChatAtom)[chatId] ?? null;
}
