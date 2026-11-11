import { chatHeaderHostAtom, chatSearchVisibleAtom } from "@/state/chat-header";
import { SidebarTrigger, useSidebar } from "@/components/ui/sidebar";
import { isWeb } from "@/lib/platform";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import {
  Minus,
  PanelRightClose,
  PanelRightOpen,
  Square,
  X,
} from "lucide-react";
import { toast } from "sonner";
import { shortcutHint } from "@/lib/app-shortcuts";
import { ChatStateIndicators } from "@/components/chat-state-indicators";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  liveChatAtom,
  liveChatIsWorkingAtom,
  sidePanelOpenAtom,
  sidePanelMaximizedAtom,
  sidePanelPinnedAtom,
  workspacePathAtom,
} from "@/state";

export function TopBar() {
  const setHeaderHost = useSetAtom(chatHeaderHostAtom);
  const { openMobile } = useSidebar();
  const [sheetOpen, setSheetOpen] = useAtom(sidePanelOpenAtom);
  const [workspaceMaximized, setWorkspaceMaximized] = useAtom(
    sidePanelMaximizedAtom,
  );
  const workspacePinned = useAtomValue(sidePanelPinnedAtom);
  const workspacePath = useAtomValue(workspacePathAtom);
  const live = useAtomValue(liveChatAtom);
  const isWorking = useAtomValue(liveChatIsWorkingAtom);
  const searchVisible = useAtomValue(chatSearchVisibleAtom);

  const toggleWorkspacePanel = () => {
    if (sheetOpen) {
      setSheetOpen(false);
      return;
    }

    if (!workspacePath) {
      toast.error("Choose a workspace before opening the file panel.");
      return;
    }

    setSheetOpen(true);
  };
  return (
    <>
      <div className="relative z-60 flex h-9 shrink-0 items-center border-b border-border bg-sidebar">
        <SidebarTrigger
          className="h-9 w-10 shrink-0 rounded-none md:hidden"
          title={openMobile ? "Close chat sidebar" : "Open chat sidebar"}
          aria-label={openMobile ? "Close chat sidebar" : "Open chat sidebar"}
          aria-expanded={openMobile}
        />
        <div
          data-tauri-drag-region
          className="flex h-full shrink-0 items-center gap-2 px-3 select-none"
        >
          <img src="/acp-mark.svg" alt="" className="size-4" />
          <span className="hidden text-xs font-medium border-r pr-2 sm:block">
            AMARCODE
          </span>
        </div>
        <div
          ref={setHeaderHost}
          data-tauri-drag-region
          className="flex h-full min-w-0 flex-1 items-center"
        />
        {live && !searchVisible && (
          <div className="pointer-events-none absolute inset-y-0 left-1/2 flex max-w-[30vw] -translate-x-1/2 items-center justify-center bg-sidebar px-2 select-none">
            <ChatStateIndicators
              loading={live.loading}
              isWorking={isWorking}
              contextRestoration={live.contextRestoration}
              runStatus={live.runStatus}
            />
          </div>
        )}
        <div className="flex h-full">
          {workspaceMaximized && (
            <button
              type="button"
              className="flex h-9 items-center gap-2 px-3 text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
              onClick={() => {
                setWorkspaceMaximized(false);
                if (!workspacePinned) setSheetOpen(false);
              }}
            >
              {live?.pendingRequest && (
                <span
                  className="size-2 rounded-full bg-orange-500"
                  aria-hidden="true"
                />
              )}
              {live?.pendingRequest
                ? "Agent needs attention · Return to chat"
                : "Return to chat"}
            </button>
          )}
          {isWeb && (
            <button
              type="button"
              className="px-3 text-xs text-muted-foreground hover:text-foreground"
              onClick={() => window.location.reload()}
            >
              Disconnect
            </button>
          )}
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                className="grid h-9 w-10 place-items-center text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                onClick={() => void toggleWorkspacePanel()}
                aria-label={
                  sheetOpen ? "Close workspace panel" : "Open workspace panel"
                }
                aria-expanded={sheetOpen}
              >
                {sheetOpen ? (
                  <PanelRightClose className="size-4" />
                ) : (
                  <PanelRightOpen className="size-4" />
                )}
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom">
              {`${sheetOpen ? "Close" : "Open"} workspace (${shortcutHint("workspace")})`}
            </TooltipContent>
          </Tooltip>
          {!isWeb && (
            <>
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
            </>
          )}
        </div>
      </div>
    </>
  );
}
