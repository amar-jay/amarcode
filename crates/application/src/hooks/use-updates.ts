import { isWeb } from "@/lib/platform";
import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { notify, notifyToast } from "@/lib/notify";
import { Download } from "lucide-react";
import {
  applyUpdatePlan,
  checkUpdatePlan,
  hasUpdates,
  updateError,
  type UpdatePlan,
  type UpdateProgress,
} from "@/lib/updates";
import type { DaemonBootstrapStatus } from "@/api";

export function useUpdates(
  connectionStatus: DaemonBootstrapStatus["status"],
  reconcile: () => Promise<void>,
) {
  const [plan, setPlan] = useState<UpdatePlan | null>(null);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const [checking, setChecking] = useState(false);
  const [updating, setUpdating] = useState(false);
  const busy = useRef(false);

  const checkUpdates = useCallback(async (manual = false) => {
    if (isWeb || busy.current) return;
    busy.current = true;
    setChecking(true);
    toast.dismiss("amarcode-update");
    try {
      const next = await checkUpdatePlan();
      setPlan(next);
      setProgress(null);
      setError(null);
      if (hasUpdates(next)) {
        if (manual) setOpen(true);
        else
          notifyToast("Amarcode update available", {
            id: "amarcode-update",
            duration: 15_000,
            description:
              "Update Amarcode and its connected background service together.",
            action: {
              label: "Review update",
              icon: Download,
              onClick: () => setOpen(true),
            },
          });
      } else if (manual) {
        setOpen(false);
        if (next.issues.length) notify(next.issues.join("\n"), "warning");
        else notify("Amarcode is up to date.", "success");
      }
    } catch (cause) {
      if (manual) notify(updateError(cause), "error");
      else console.info("Update check failed:", cause);
    } finally {
      busy.current = false;
      setChecking(false);
    }
  }, []);

  // Check both components together once startup has either connected or failed.
  useEffect(() => {
    if (isWeb) return;
    if (!["ready", "failed", "installRequired"].includes(connectionStatus))
      return;
    const timer = window.setTimeout(() => void checkUpdates(), 0);
    return () => window.clearTimeout(timer);
  }, [connectionStatus, checkUpdates]);

  const installUpdates = useCallback(async () => {
    if (busy.current || !hasUpdates(plan) || !plan) return;
    busy.current = true;
    setUpdating(true);
    setProgress(null);
    setError(null);
    toast.dismiss("amarcode-update");
    try {
      await applyUpdatePlan(
        plan,
        setProgress,
        () => {
          setPlan((current) =>
            current ? { ...current, daemon: null } : current,
          );
        },
        reconcile,
      );
      setPlan(null);
      notify("Amarcode updated successfully.", "success");
    } catch (cause) {
      setError(updateError(cause));
      // A daemon update may have rolled back. Recover the chat state either way.
      try {
        await reconcile();
      } catch (reconnectError) {
        console.info("Update reconnection failed:", reconnectError);
      }
    } finally {
      busy.current = false;
      setUpdating(false);
    }
  }, [plan, reconcile]);

  return {
    updatePlan: plan,
    updateProgress: progress,
    updateError: error,
    updateOpen: open,
    updateChecking: checking,
    updating,
    updateAvailable: hasUpdates(plan),
    checkUpdates,
    installUpdates,
    closeUpdate: () => {
      if (!busy.current) {
        setOpen(false);
        setProgress(null);
        setError(null);
      }
    },
  };
}
