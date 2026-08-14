import { useAtom } from "jotai";
import { FileDiff, RefreshCw, XIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { daemonApi, WorkspaceDiff, type WorkspaceChange } from "@/api";
import { sidePanelOpenAtom } from "@/state";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "./ui/sheet";
import { useWorkspaceFileTree, WorkspaceFileTree } from "./workspace-file-tree";
import { WorkspaceDiffViewer } from "./workspace-diff-viewer";
import { Button } from "./ui/button";
import { cn } from "@/lib/utils";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "./ui/resizable";

interface AppSidePanelProps {
  workspacePath: string;
}
function AppSidePanel({ workspacePath }: AppSidePanelProps) {
  const [sheetOpen, setSheetOpen] = useAtom(sidePanelOpenAtom);
  const [dirName, setDirName] = useState("No workspace selected");
  const [changes, setChanges] = useState<WorkspaceChange[]>([]);
  useEffect(() => {
    if (!workspacePath) {
      setDirName("No workspace selected");
      return;
    }

    void daemonApi
      .getWorkspaceInfo(workspacePath)
      .then((workspace) => setDirName(workspace.displayName))
      .catch(() => setDirName("Workspace"));
  }, [workspacePath]);
  const workspaceFileTree = useWorkspaceFileTree(sheetOpen, workspacePath);
  const refreshChanges = useCallback(async () => {
    if (!workspacePath) {
      setChanges([]);
      return;
    }
    try {
      setChanges(await daemonApi.listWorkspaceChanges(workspacePath));
    } catch {
      setChanges([]);
    }
  }, [workspacePath]);

  useEffect(() => {
    if (sheetOpen) void refreshChanges();
  }, [refreshChanges, sheetOpen]);

  const changesByPath = useMemo(
    () => new Map(changes.map((change) => [change.path, change])),
    [changes],
  );

  const [diff, setDiff] = useState<WorkspaceDiff>();

  return (
    <Sheet open={sheetOpen} onOpenChange={setSheetOpen}>
      <SheetContent
        side="right"
        resizable
        className={cn(
          workspaceFileTree.selectedPath
            ? "w-280 min-w-[min(100vw-50rem,70rem)] max-w-[80vw]!"
            : "w-40 min-w-[min(100vw-30rem,10rem)] max-w-[33vw]!",
        )}
        showCloseButton={false}
      >
        <SheetHeader className="px-3 py-1 flex-row items-baseline">
          <SheetTitle className="text-xs font-bold" title={workspacePath || ""}>
            {dirName || ""}{" "}
            {changes.length > 0 && (
              <span className="rounded bg-muted px-1.5 ml-3 py-0.5 text-[0.6rem]">
                ~ {changes.length}
              </span>
            )}
          </SheetTitle>
          <SheetDescription className="text-xs text-muted-foreground flex flex-row items-center gap-2 ml-auto">
            {workspaceFileTree.selectedPath && (
              <>
                <FileDiff
                  className={cn(
                    "size-4 text-muted-foreground",
                    diff?.status === "M" && diff?.comparison === "Unstaged"
                      ? "text-orange-500"
                      : diff?.status === "M" && diff?.comparison === "Staged"
                        ? "text-green-500"
                        : "",
                  )}
                />
                <span
                  className="truncate font-mono text-xs"
                  title={workspaceFileTree.selectedPath || ""}
                >
                  {workspaceFileTree.selectedPath || ""}
                </span>
              </>
            )}
          </SheetDescription>

          <Button
            disabled={
              !workspaceFileTree.validWorkspace || workspaceFileTree.isLoading
            }
            onClick={() => {
              void workspaceFileTree.refresh();
              void refreshChanges();
            }}
            className="ml-auto"
            size="icon-sm"
            variant="ghost"
          >
            <RefreshCw
              className={workspaceFileTree.isLoading ? "animate-spin" : ""}
            />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => setSheetOpen(false)}
          >
            <XIcon />
            <span className="sr-only">Close</span>
          </Button>
        </SheetHeader>

        <ResizablePanelGroup
          orientation="horizontal"
          className="min-h-0 flex-1 border-t"
        >
          <ResizablePanel
            defaultSize="256px"
            minSize="180px"
            maxSize="450px"
            groupResizeBehavior="preserve-pixel-size"
          >
            <div className="flex size-full min-h-0 flex-col">
              <WorkspaceFileTree
                {...workspaceFileTree}
                changes={changesByPath}
              />
            </div>
          </ResizablePanel>
          <ResizableHandle withHandle />
          {!!workspaceFileTree.selectedPath && (
            <ResizablePanel minSize="0px">
              <WorkspaceDiffViewer
                selectedPath={workspaceFileTree.selectedPath}
                workspacePath={workspacePath}
                setDiff={setDiff}
                diff={diff}
              />
            </ResizablePanel>
          )}
        </ResizablePanelGroup>
      </SheetContent>
    </Sheet>
  );
}

export default AppSidePanel;
