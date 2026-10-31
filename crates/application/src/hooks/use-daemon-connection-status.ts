import { useEffect, useState } from "react";
import { daemonApi, type DaemonConnectionStatus } from "@/api";

/** Native SSH lifecycle, also available while normal daemon bootstrap has failed. */
export function useDaemonConnectionStatus() {
  const [status, setStatus] = useState<DaemonConnectionStatus | null>(null);
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const value = await daemonApi.getConnectionStatus();
        if (!disposed) setStatus(value);
      } catch {
        // Bootstrap/settings report IPC failures; this status is supplementary.
      } finally {
        if (!disposed) timer = setTimeout(() => void poll(), 1_000);
      }
    };
    void poll();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, []);
  return status;
}
