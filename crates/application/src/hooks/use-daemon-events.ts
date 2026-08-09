import { useEffect, useState } from "react";
import { daemonApi } from "@/api";
import type { EditorEvent, TurnStatus } from "@/types";

type EventListener = (event: EditorEvent) => void;

const listeners = new Set<EventListener>();
let streamStarted = false;

/** Latest turnUpdated payload per chat — survives remount / last-event overwrite. */
const latestTurnByChat = new Map<
  string,
  {
    run_id: string;
    user_message_id: string;
    status: TurnStatus;
    stop_reason: string | null;
    error_message: string | null;
  }
>();

function rememberEvent(event: EditorEvent) {
  if (event.type !== "turnUpdated") return;
  latestTurnByChat.set(event.payload.chat_id, {
    run_id: event.payload.run_id,
    user_message_id: event.payload.user_message_id,
    status: event.payload.status,
    stop_reason: event.payload.stop_reason,
    error_message: event.payload.error_message,
  });
}

function startEventStream() {
  if (streamStarted) return;
  streamStarted = true;
  void daemonApi
    .subscribeEvents({}, (event) => {
      rememberEvent(event);
      listeners.forEach((listener) => listener(event));
    })
    .catch((error: unknown) => {
      // A future reconnect policy can replace this; do not create duplicate
      // subscriptions while React Strict Mode replays effects.
      console.error("daemon event stream disconnected", error);
    });
}

/** Snapshot of the last turnUpdated for a chat, if any were observed this session. */
export function getLatestTurnForChat(chatId: string) {
  return latestTurnByChat.get(chatId) ?? null;
}

/** One unfiltered daemon stream, shared by every mounted UI consumer. */
export function useDaemonEvents(): EditorEvent | null {
  const [event, setEvent] = useState<EditorEvent | null>(null);

  useEffect(() => {
    listeners.add(setEvent);
    startEventStream();
    return () => {
      listeners.delete(setEvent);
    };
  }, []);

  return event;
}
