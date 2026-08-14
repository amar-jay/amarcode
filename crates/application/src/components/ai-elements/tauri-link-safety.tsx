import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import type { MouseEvent } from "react";
import type { LinkSafetyConfig, LinkSafetyModalProps } from "streamdown";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { notify } from "@/lib/notify";

function localPath(target: string): string | null {
  if (target.startsWith("/")) return target;

  try {
    const url = new URL(target);
    return url.protocol === "file:" ? decodeURIComponent(url.pathname) : null;
  } catch {
    return null;
  }
}

export async function openExternalTarget(target: string) {
  const path = localPath(target);
  if (path) {
    await openPath(path);
    return;
  }
  await openUrl(target);
}

export function handleExternalLinkClick(event: MouseEvent<HTMLAnchorElement>) {
  if (
    event.defaultPrevented ||
    event.button !== 0 ||
    event.metaKey ||
    event.ctrlKey ||
    event.shiftKey ||
    event.altKey
  ) {
    return;
  }

  event.preventDefault();
  const target =
    event.currentTarget.getAttribute("href") ?? event.currentTarget.href;
  void openExternalTarget(target).catch((error: unknown) => {
    console.error("Unable to open external link:", error);
    notify("Unable to open this link with a desktop application.", "error");
  });
}

function TauriLinkSafetyModal({ isOpen, onClose, url }: LinkSafetyModalProps) {
  const openLink = async () => {
    try {
      await openExternalTarget(url);
      onClose();
    } catch (error) {
      console.error("Unable to open external link:", error);
      notify("Unable to open this link with a desktop application.", "error");
    }
  };

  return (
    <Dialog open={isOpen} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="gap-4 sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Open external link?</DialogTitle>
          <DialogDescription>
            You&apos;re about to open this link with a desktop application.
          </DialogDescription>
        </DialogHeader>
        <p className="max-h-36 overflow-y-auto rounded-md bg-muted p-3 font-mono text-xs break-all">
          {url}
        </p>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={() => void openLink()}>Open link</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export const tauriLinkSafety: LinkSafetyConfig = {
  enabled: true,
  renderModal: (props) => <TauriLinkSafetyModal {...props} />,
};
