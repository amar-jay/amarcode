import { atom, getDefaultStore } from "jotai";
import { daemonApi } from "@/api";
import type { ChatSessionSnapshot, EditorEvent, TurnStatus } from "@/types";

type JotaiStore = ReturnType<typeof getDefaultStore>;
type EventListener = (event: EditorEvent) => void;

/**
 * Latest daemon event (debug / simple subscribers). Prefer
 * `subscribeDaemonEvents` for anything that must see *every* event,
 * high-frequency updates can overwrite this atom between React renders.
 */
export const lastDaemonEventAtom = atom<EditorEvent | null>(null);

export type TurnSnapshot = {
  run_id: string;
  user_message_id: string;
  status: TurnStatus;
  stop_reason: string | null;
  error_message: string | null;
};

/** Latest turnUpdated payload per chat, survives remount / last-event overwrite. */
export const latestTurnByChatAtom = atom<Record<string, TurnSnapshot>>({});

export type DaemonEventStreamState = {
  status: "idle" | "connecting" | "connected" | "reconnecting";
  error: string | null;
  reconnectAttempt: number;
  retryInMs: number | null;
};

/** Connection lifecycle exposed to React for banners/debugging/telemetry. */
export const daemonEventStreamStateAtom = atom<DaemonEventStreamState>({
  status: "idle",
  error: null,
  reconnectAttempt: 0,
  retryInMs: null,
});

/** Changes on each subscription attempt and disconnect; invalidates old RPC responses. */
export const daemonConnectionGenerationAtom = atom(0);
export const sessionSnapshotsAtom = atom<Record<string, ChatSessionSnapshot>>(
  {},
);

export const rememberSessionSnapshotAtom = atom(
  null,
  (get, set, snapshot: ChatSessionSnapshot) => {
    const previous = get(sessionSnapshotsAtom);
    const known = Object.values(previous)[0];
    const sameInstance =
      !known || known.daemon_instance_id === snapshot.daemon_instance_id;
    const existing = sameInstance ? previous[snapshot.chat_id] : undefined;
    if (existing && existing.revision > snapshot.revision) return false;
    if (!sameInstance) set(latestTurnByChatAtom, {});
    set(sessionSnapshotsAtom, {
      ...(sameInstance ? previous : {}),
      [snapshot.chat_id]: snapshot,
    });
    const turns = { ...get(latestTurnByChatAtom) };
    if (snapshot.turn && snapshot.run_id) {
      turns[snapshot.chat_id] = { ...snapshot.turn, run_id: snapshot.run_id };
    } else delete turns[snapshot.chat_id];
    set(latestTurnByChatAtom, turns);
    return true;
  },
);

let streamStarted = false;
let reconnectAttempt = 0;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
const INITIAL_RECONNECT_DELAY_MS = 250;
const MAX_RECONNECT_DELAY_MS = 5_000;
/** Store that owns the React tree (Provider). Must not be getDefaultStore() when a Provider is used. */
let boundStore: JotaiStore | null = null;
const listeners = new Set<EventListener>();

function activeStore(fallback?: JotaiStore): JotaiStore {
  return boundStore ?? fallback ?? getDefaultStore();
}

/**
 * Start the shared daemon event stream once, writing into the **same** jotai
 * store React uses (pass `useStore()` from under `JotaiProvider`).
 *
 * Using `getDefaultStore()` while the app mounts `<Provider>` silently
 * orphans events, the UI stays stuck on "Thinking…" forever.
 */
export function ensureDaemonEventStream(store: JotaiStore) {
  boundStore = store;
  if (streamStarted || reconnectTimer) return;
  const generation = store.get(daemonConnectionGenerationAtom) + 1;
  store.set(daemonConnectionGenerationAtom, generation);
  streamStarted = true;
  store.set(daemonEventStreamStateAtom, {
    status: reconnectAttempt > 0 ? "reconnecting" : "connecting",
    error: store.get(daemonEventStreamStateAtom).error,
    reconnectAttempt,
    retryInMs: null,
  });

  void daemonApi
    .subscribeEvents(
      {},
      (event) => {
        const s = activeStore(store);
        if (s.get(daemonConnectionGenerationAtom) !== generation) return;
        if (event.type === "sessionStateUpdated")
          s.set(rememberSessionSnapshotAtom, event.payload.snapshot);
        s.set(lastDaemonEventAtom, event);
        for (const listener of listeners) {
          try {
            listener(event);
          } catch (error) {
            console.error("daemon event listener failed", error);
          }
        }
      },
      (status) => {
        const s = activeStore(store);
        if (s.get(daemonConnectionGenerationAtom) !== generation) return;
        if (status.status === "connected") {
          reconnectAttempt = 0;
          s.set(daemonEventStreamStateAtom, {
            status: "connected",
            error: null,
            reconnectAttempt: 0,
            retryInMs: null,
          });
        } else {
          s.set(daemonConnectionGenerationAtom, generation + 1);
          s.set(daemonEventStreamStateAtom, {
            status: "reconnecting",
            error: status.error,
            reconnectAttempt: reconnectAttempt + 1,
            retryInMs: null,
          });
        }
      },
    )
    .catch((error: unknown) => {
      streamStarted = false;
      activeStore(store).set(daemonConnectionGenerationAtom, generation + 1);
      const message = error instanceof Error ? error.message : String(error);
      reconnectAttempt += 1;
      const delay = Math.min(
        INITIAL_RECONNECT_DELAY_MS * 2 ** (reconnectAttempt - 1),
        MAX_RECONNECT_DELAY_MS,
      );
      const s = activeStore(store);
      s.set(daemonEventStreamStateAtom, {
        status: "reconnecting",
        error: message,
        reconnectAttempt,
        retryInMs: delay,
      });
      console.warn(
        `daemon event stream disconnected; retrying in ${delay}ms`,
        error,
      );

      if (!reconnectTimer) {
        reconnectTimer = setTimeout(() => {
          reconnectTimer = null;
          ensureDaemonEventStream(activeStore(store));
        }, delay);
      }
    });
}

/**
 * Subscribe to every daemon event (not just the latest atom value).
 * Returns an unsubscribe function.
 */
export function subscribeDaemonEvents(listener: EventListener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Snapshot of the last turnUpdated for a chat, if any were observed this session. */
export function getLatestTurnForChat(chatId: string): TurnSnapshot | null {
  return activeStore().get(latestTurnByChatAtom)[chatId] ?? null;
}
