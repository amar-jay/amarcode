import { Button, buttonVariants } from "@/components/ui/button";
import { X } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

type DaemonConnectionDialogProps = {
  status: "connecting" | "error";
  onRetry: () => void;
  onCloseApplication: () => void;
};

export function DaemonConnectionDialog({
  status,
  onRetry,
  onCloseApplication,
}: DaemonConnectionDialogProps) {
  const isConnecting = status === "connecting";

  return (
    <Dialog open
		 >
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
              isConnecting
                ? "size-full animate-[pulse_1.8s_ease-in-out_infinite] dark:invert"
                : "size-full dark:invert"
            }
          />
        </div>
        <DialogHeader className="items-center gap-2">
          <DialogTitle className="text-base">
            {isConnecting ? "Connecting..." : "Connection failed"}
          </DialogTitle>
          <DialogDescription className="max-w-[18rem]">
            {isConnecting
              ? "Looking for the local daemon service."
              : "The local daemon service is unavailable."}
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-2">
	          <Button className={buttonVariants({variant: !isConnecting ? "default" : "outline"})} onClick={onRetry} disabled={isConnecting}>
	          {isConnecting ? 
								"Connecting..." : "Try again"
						}
	          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
