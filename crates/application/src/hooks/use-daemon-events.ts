import { useEffect, useState } from "react";
import { daemonApi } from "@/api";
import type { EditorEvent } from "@/types";

type EventListener = (event: EditorEvent) => void;

const listeners = new Set<EventListener>();
let streamStarted = false;

function startEventStream() {
  if (streamStarted) return;
  streamStarted = true;
  void daemonApi.subscribeEvents({}, (event) => {
    listeners.forEach((listener) => listener(event));
  }).catch((error: unknown) => {
    // A future reconnect policy can replace this; do not create duplicate
    // subscriptions while React Strict Mode replays effects.
    console.error("daemon event stream disconnected", error);
  });
}

/** One unfiltered daemon stream, shared by every mounted UI consumer. */
export function useDaemonEvents(): EditorEvent | null {
  const [event, setEvent] = useState<EditorEvent | null>(null);

  useEffect(() => {
    listeners.add(setEvent);
    startEventStream();
    return () => { listeners.delete(setEvent); };
  }, []);

  return event;
}
