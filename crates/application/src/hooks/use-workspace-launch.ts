import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { useSetAtom } from "jotai";
import { daemonApi } from "@/api";
import { isWeb } from "@/lib/platform";
import { notify } from "@/lib/notify";
import { openWorkspaceLaunchAtom } from "@/state/navigation";

/** Subscribe before reading the pending request so startup and reloads cannot lose it. */
export function useWorkspaceLaunch(ready: boolean) {
  const openWorkspace = useSetAtom(openWorkspaceLaunchAtom);
  const handled = useRef(0);
  const warned = useRef(0);
  useEffect(() => {
    if (isWeb || !ready) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const apply = async () => {
      try {
        const request = await daemonApi.getWorkspaceLaunch();
        if (disposed || !request || request.id <= handled.current) return;
        const connection = await daemonApi.getConnection();
        if (disposed || request.id <= handled.current) return;
        if (connection.remote) {
          if (warned.current !== request.id) {
            warned.current = request.id;
            notify(
              "A local workspace was requested from the terminal. Switch to Local daemon in Connection settings to open it.",
              "info",
            );
          }
          return;
        }
        handled.current = request.id;
        openWorkspace(request.path);
        await daemonApi.acknowledgeWorkspaceLaunch(request.id);
      } catch (error) {
        if (!disposed)
          notify(
            error instanceof Error
              ? error.message
              : "Could not open the requested workspace.",
            "error",
          );
      }
    };
    void listen("workspace-launch-requested", () => {
      void apply();
    })
      .then((stop) => {
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
        void apply();
      })
      .catch((error: unknown) => {
        if (!disposed)
          notify(
            error instanceof Error
              ? error.message
              : "Could not listen for workspace launches.",
            "error",
          );
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [ready, openWorkspace]);
}
