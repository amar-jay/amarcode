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
  onCloseApplication: () => void;
};

export function DaemonConnectionDialog({
  status,
  onRetry,
  onCloseApplication,
}: DaemonConnectionDialogProps) {
  const isError = status.status === "failed";
  const title =
    status.status === "downloading"
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
  const description =
    status.status === "downloading"
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
            src="/connection.svg"
            alt=""
            className={
              !isError
                ? "size-full animate-[pulse_1.8s_ease-in-out_infinite] dark:invert"
                : "size-full dark:invert"
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
          <Button
            className={buttonVariants({
              variant: isError ? "default" : "outline",
            })}
            onClick={onRetry}
            disabled={!isError}
          >
            {!isError ? "Please wait..." : "Try again"}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
