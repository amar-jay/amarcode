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
import type { AppUpdateStatus } from "@/api";

type AppUpdateDialogProps = {
  version: string | null;
  notes: string | null;
  status: AppUpdateStatus | null;
  onConfirm: () => void;
  onClose: () => void;
};

export function AppUpdateDialog({
  version,
  notes,
  status,
  onConfirm,
  onClose,
}: AppUpdateDialogProps) {
  const updating = status !== null && status.status !== "failed";
  const failed = status?.status === "failed";

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
            src={appUpdateIllustration(status)}
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
              ? "Application update failed"
              : updating
                ? `Updating Amarcode…`
                : `Update Amarcode to latest version?`}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {appUpdateDescription(status) ??
              notes ??
              "The signed update will be downloaded. Amarcode will restart after installation."}
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

function appUpdateIllustration(status: AppUpdateStatus | null): string {
  switch (status?.status) {
    case "downloading":
      return "/illustrations/daemon-downloading.png";
    case "installing":
      return "/illustrations/daemon-installing.png";
    case "restarting":
      return "/illustrations/daemon-starting.png";
    case "failed":
      return "/illustrations/daemon-error.png";
    default:
      return "/illustrations/daemon-update.png";
  }
}

function appUpdateDescription(status: AppUpdateStatus | null): string | null {
  if (!status) return null;
  switch (status.status) {
    case "downloading":
      return status.total > 0
        ? `Downloading signed update… ${Math.round((status.received / status.total) * 100)}%`
        : "Downloading signed update…";
    case "installing":
      return "The signature is valid. Installing the update…";
    case "restarting":
      return "Update installed. Restarting Amarcode…";
    case "failed":
      return status.error;
  }
}
