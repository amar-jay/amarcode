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
import { connectionIdentity } from "@/lib/daemon-connection-form";
import {
  hasUpdates,
  type UpdatePlan,
  type UpdateProgress,
} from "@/lib/updates";

type Props = {
  open: boolean;
  plan: UpdatePlan | null;
  progress: UpdateProgress | null;
  error: string | null;
  updating: boolean;
  onConfirm: () => void;
  onClose: () => void;
};
export function UpdateDialog({
  open,
  plan,
  progress,
  error,
  updating,
  onConfirm,
  onClose,
}: Props) {
  const complete = progress?.component === "complete" && !error;
  const target = plan?.connection?.remote
    ? (connectionIdentity(plan.connection) ?? "remote host")
    : "this computer";
  const stage =
    progress && progress.component !== "complete"
      ? progress.status.status
      : null;
  const illustration = error
    ? "error"
    : stage === "rollingBack"
      ? "rollback"
      : stage === "verifying"
        ? "verifying"
        : stage === "downloading"
          ? "downloading"
          : stage === "installing"
            ? "installing"
            : stage === "restarting" || stage === "ready" || complete
              ? "starting"
              : "update";
  return (
    <AlertDialog
      open={open}
      onOpenChange={(value) => {
        if (!value && !updating) onClose();
      }}
    >
      <AlertDialogContent>
        <div
          aria-hidden
          className="mx-auto flex size-28 items-center justify-center"
        >
          <img
            src={`/illustrations/daemon-${illustration}.png`}
            alt=""
            className={`size-full object-contain dark:invert${updating ? " animate-[pulse_1.8s_ease-in-out_infinite]" : ""}`}
          />
        </div>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {error
              ? "Update interrupted"
              : complete
                ? "Amarcode updated"
                : updating
                  ? "Updating Amarcode…"
                  : "Update Amarcode?"}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {error ??
              progressDescription(progress) ??
              "Install available updates for Amarcode and its connected background service."}
          </AlertDialogDescription>
        </AlertDialogHeader>
        {!complete && (
          <div className="space-y-3 text-xs text-muted-foreground">
            <div className="space-y-1">
              {plan?.app && (
                <p>
                  Application: {plan.app.currentVersion} → {plan.app.version}
                </p>
              )}
              {plan?.daemon && (
                <p>
                  Background service on {target}: {plan.daemon.currentVersion} →{" "}
                  {plan.daemon.version}
                </p>
              )}
            </div>
            {!updating && !error && (
              <p>
                {plan?.daemon &&
                  `The background service on ${target} will restart, interrupting active agent turns. Saved chats remain available. `}
                {plan?.app && "Amarcode will restart after installation."}
              </p>
            )}
            {plan?.issues.map((issue) => (
              <p key={issue}>{issue}</p>
            ))}
            {plan?.app?.notes && !updating && (
              <p className="max-h-32 overflow-y-auto whitespace-pre-wrap">
                {plan.app.notes}
              </p>
            )}
          </div>
        )}
        <AlertDialogFooter>
          {updating ? (
            <Button disabled>Please wait…</Button>
          ) : complete ? (
            <Button onClick={onClose}>Done</Button>
          ) : error ? (
            <>
              <Button variant="outline" onClick={onClose}>
                Close
              </Button>
              {hasUpdates(plan) && (
                <Button onClick={onConfirm}>Retry update</Button>
              )}
            </>
          ) : (
            <>
              <AlertDialogCancel>Later</AlertDialogCancel>
              <AlertDialogAction
                onClick={(event) => {
                  event.preventDefault();
                  onConfirm();
                }}
              >
                Update
              </AlertDialogAction>
            </>
          )}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

function progressDescription(progress: UpdateProgress | null): string | null {
  if (!progress) return null;
  if (progress.component === "complete")
    return "The available updates have been installed.";
  const name =
    progress.component === "application" ? "application" : "background service";
  const status = progress.status;
  switch (status.status) {
    case "downloading":
      return status.total > 0
        ? `Downloading ${name}… ${Math.round((status.received / status.total) * 100)}%`
        : `Downloading ${name}…`;
    case "verifying":
      return "Verifying the background service release…";
    case "installing":
      return `Installing ${name}…`;
    case "restarting":
      return `Restarting ${name}…`;
    case "rollingBack":
      return "Restoring the previous background service version…";
    case "ready":
      return "Background service updated. Reconnecting…";
    case "failed":
      return status.error;
  }
}
