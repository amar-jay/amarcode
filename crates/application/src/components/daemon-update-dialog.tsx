import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import type { DaemonUpdateStatus } from "@/api";

type DaemonUpdateDialogProps = {
  version: string | null;
  status: DaemonUpdateStatus | null;
  onConfirm: () => void;
  onClose: () => void;
};

export function DaemonUpdateDialog({
  version,
  status,
  onConfirm,
  onClose,
}: DaemonUpdateDialogProps) {
  const updating = status !== null && status.status !== "failed";
  const failed = status?.status === "failed";
  const description = updateDescription(status);
  const illustration = updateIllustration(status);

  return (
    <AlertDialog
      open={version !== null}
      onOpenChange={(open) => {
        if (!open && !updating) onClose();
      }}
    >
      <AlertDialogContent>
        <div
          aria-hidden
          className="mx-auto flex size-28 items-center justify-center"
        >
          <img
            key={illustration}
            src={illustration}
            alt=""
            className={
              updating
                ? "size-full object-contain animate-[pulse_1.8s_ease-in-out_infinite] dark:invert"
                : "size-full object-contain dark:invert"
            }
          />
        </div>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {failed
              ? "Daemon update failed"
              : updating
                ? `Updating daemon to ${version}…`
                : `Update daemon to ${version}?`}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {description ??
              "The background service will restart. Any active agent turns will be interrupted, but chats and completed messages remain saved."}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          {failed ? (
            <Button onClick={onClose}>Close</Button>
          ) : updating ? (
            <Button disabled>Please wait…</Button>
          ) : (
            <>
              <AlertDialogCancel>Later</AlertDialogCancel>
              <AlertDialogAction
                onClick={(event) => {
                  event.preventDefault();
                  onConfirm();
                }}
              >
                Update and restart
              </AlertDialogAction>
            </>
          )}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

function updateIllustration(status: DaemonUpdateStatus | null): string {
  switch (status?.status) {
    case "downloading":
      return "/illustrations/daemon-downloading.png";
    case "verifying":
      return "/illustrations/daemon-verifying.png";
    case "installing":
      return "/illustrations/daemon-installing.png";
    case "restarting":
    case "ready":
      return "/illustrations/daemon-starting.png";
    case "rollingBack":
      return "/illustrations/daemon-rollback.png";
    case "failed":
      return "/illustrations/daemon-error.png";
    default:
      return "/illustrations/daemon-update.png";
  }
}

function updateDescription(status: DaemonUpdateStatus | null): string | null {
  if (!status) return null;
  switch (status.status) {
    case "downloading":
      return status.total > 0
        ? `Downloading verified release… ${Math.round((status.received / status.total) * 100)}%`
        : "Downloading verified release…";
    case "verifying":
      return "Verifying the release signature, checksum, and service commands…";
    case "installing":
      return "Registering the new version with the native service manager…";
    case "restarting":
      return "Restarting the background service and checking its version…";
    case "rollingBack":
      return "The new daemon did not start correctly. Restoring the previous version…";
    case "ready":
      return `Daemon ${status.version} is ready.`;
    case "failed":
      return status.error;
  }
}
