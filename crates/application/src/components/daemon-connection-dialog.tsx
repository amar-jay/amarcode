import { Button, buttonVariants } from "@/components/ui/button";
import { X } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { DaemonBootstrapStatus } from "@/api";

type DaemonConnectionDialogProps = {
  status: Exclude<DaemonBootstrapStatus, { status: "ready" }>;
  onRetry: () => void;
  onInstall: () => void;
  onCloseApplication: () => void;
};

export function DaemonConnectionDialog({
  status,
  onRetry,
  onInstall,
  onCloseApplication,
}: DaemonConnectionDialogProps) {
  const isError = status.status === "failed";
  const requiresInstall = status.status === "installRequired";
  const illustration = connectionIllustration(status);
  const title = requiresInstall
    ? "Background service required"
    : status.status === "downloading"
      ? "Downloading daemon..."
      : status.status === "verifying"
        ? "Verifying daemon..."
        : status.status === "installing"
          ? "Installing daemon..."
          : status.status === "starting"
            ? "Starting daemon..."
            : isError
              ? "Connection failed"
              : "Checking daemon...";
  const description = requiresInstall
    ? `${status.reason} It runs for your user account, starts at login, and remains available when this window closes.`
    : status.status === "downloading"
      ? status.total > 0
        ? Math.round((status.received / status.total) * 100) + "% downloaded"
        : "Downloading the daemon for this platform."
      : isError
        ? status.error
        : "Preparing the local daemon service.";

  return (
    <Dialog open>
      <DialogContent
        showCloseButton={false}
        className="max-w-xs gap-6 p-7 text-center"
        onEscapeKeyDown={(event) => event.preventDefault()}
      >
        <button
          type="button"
          aria-label="Close application"
          onClick={onCloseApplication}
          className="absolute top-3 right-3 grid size-8 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
        >
          <X className="size-4" />
        </button>
        <div className="mx-auto flex size-36 items-center justify-center">
          <img
            key={illustration}
            src={illustration}
            alt=""
            className={
              !isError && !requiresInstall
                ? "size-full object-contain animate-[pulse_1.8s_ease-in-out_infinite] dark:invert"
                : "size-full object-contain dark:invert"
            }
          />
        </div>
        <DialogHeader className="items-center gap-2">
          <DialogTitle className="text-base">{title}</DialogTitle>
          <DialogDescription className="max-w-[18rem]">
            {description}
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-2">
          {requiresInstall ? (
            <Button onClick={onInstall}>Install and start service</Button>
          ) : (
            <Button
              className={buttonVariants({
                variant: isError ? "default" : "outline",
              })}
              onClick={onRetry}
              disabled={!isError}
            >
              {!isError ? "Please wait..." : "Try again"}
            </Button>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function connectionIllustration(
  status: Exclude<DaemonBootstrapStatus, { status: "ready" }>,
): string {
  switch (status.status) {
    case "checking":
      return "/illustrations/daemon-checking.png";
    case "installRequired":
    case "verifying":
      return "/illustrations/daemon-verifying.png";
    case "downloading":
      return "/illustrations/daemon-downloading.png";
    case "installing":
      return "/illustrations/daemon-installing.png";
    case "starting":
      return "/illustrations/daemon-starting.png";
    case "failed":
      return "/illustrations/daemon-error.png";
  }
}
