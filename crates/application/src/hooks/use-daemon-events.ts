/**
 * @deprecated Prefer `subscribeDaemonEvents` / atoms from `@/state`.
 */
import { useAtomValue, useStore } from "jotai";
import { useEffect, useState } from "react";
import type { EditorEvent } from "@/types";
import {
  ensureDaemonEventStream,
  getLatestTurnForChat,
  lastDaemonEventAtom,
  subscribeDaemonEvents,
} from "@/state/daemon-events";

export { getLatestTurnForChat };

export function useDaemonEvents(): EditorEvent | null {
  const store = useStore();
  const latest = useAtomValue(lastDaemonEventAtom);
  const [event, setEvent] = useState<EditorEvent | null>(latest);

  useEffect(() => {
    ensureDaemonEventStream(store);
    return subscribeDaemonEvents(setEvent);
  }, [store]);

  return event ?? latest;
}
