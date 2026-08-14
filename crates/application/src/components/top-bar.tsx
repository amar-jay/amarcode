import { getCurrentWindow } from "@tauri-apps/api/window";
import { useAtom, useAtomValue } from "jotai";
import { Minus, PanelRightOpen, Square, X } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { daemonApi } from "@/api";
import { sidePanelOpenAtom, workspacePathAtom } from "@/state";

interface TopBarProps {}

export function TopBar({}: TopBarProps) {
  const [, setSheetOpen] = useAtom(sidePanelOpenAtom);
  const workspacePath = useAtomValue(workspacePathAtom);
  const [isOpeningSheet, setIsOpeningSheet] = useState(false);

  const openWorkspacePanel = async () => {
    if (!workspacePath) {
      toast.error("Choose a Git workspace before opening the file panel.");
      return;
    }

    setIsOpeningSheet(true);
    try {
      const workspace = await daemonApi.getWorkspaceInfo(workspacePath);
      if (!workspace.isGitRepository) {
        toast.error("The selected folder is not inside a Git workspace.");
        return;
      }
      setSheetOpen(true);
    } catch (error) {
      toast.error(
        error instanceof Error
          ? error.message
          : "Could not verify the workspace.",
      );
    } finally {
      setIsOpeningSheet(false);
    }
  };
  return (
    <>
      <div className="z-60 flex h-9 shrink-0 items-center border-b border-border bg-sidebar">
        <div
          data-tauri-drag-region
          className="flex h-full min-w-0 flex-1 items-center gap-2 px-3 select-none"
        >
          <img src="/acp-mark.svg" alt="" className="size-4" />
          <span className="text-xs font-medium">AMARCODE</span>
          <span className="border-l border-border pl-2 text-xs text-muted-foreground">
            workspace
          </span>
        </div>
        <div className="flex h-full">
          <button
            type="button"
            className="grid h-9 w-10 place-items-center text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            disabled={isOpeningSheet}
            onClick={() => void openWorkspacePanel()}
            aria-label="Open workspace panel"
            title="Workspace"
          >
            <PanelRightOpen className="size-4" />
          </button>
          <button
            type="button"
            className="grid h-9 w-12 place-items-center text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            onClick={() => void getCurrentWindow().minimize()}
            aria-label="Minimize"
          >
            <Minus className="size-4" />
          </button>
          <button
            type="button"
            className="grid h-9 w-12 place-items-center text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            onClick={() => void getCurrentWindow().toggleMaximize()}
            aria-label="Maximize"
          >
            <Square className="size-3.5" />
          </button>
          <button
            type="button"
            className="grid h-9 w-12 place-items-center text-muted-foreground transition-colors hover:bg-destructive hover:text-background"
            onClick={() => void getCurrentWindow().close()}
            aria-label="Close"
          >
            <X className="size-4" />
          </button>
        </div>
      </div>
    </>
  );
}
