/**
 * @deprecated Prefer `lastDaemonEventAtom` / `ensureDaemonEventStream` from `@/state`.
 */
import { useAtomValue } from "jotai";
import { useEffect } from "react";
import {
  ensureDaemonEventStream,
  getLatestTurnForChat,
  lastDaemonEventAtom,
} from "@/state/daemon-events";

export { getLatestTurnForChat };

export function useDaemonEvents() {
  useEffect(() => {
    ensureDaemonEventStream();
  }, []);
  return useAtomValue(lastDaemonEventAtom);
}
