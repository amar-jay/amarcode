import { useAtom } from "jotai";
import {
  FileDiff,
  GitBranch,
  ListFilter,
  RefreshCw,
  XIcon,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { daemonApi, WorkspaceDiff, type WorkspaceChange } from "@/api";
import { sidePanelOpenAtom, workspaceFileOpenRequestAtom } from "@/state";
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
} from "./ui/sheet";
import {
  useWorkspaceFileTree,
  WorkspaceChangedFiles,
  WorkspaceFileTree,
} from "./workspace-file-tree";
import { WorkspaceDiffViewer } from "./workspace-diff-viewer";
import { Button } from "./ui/button";
import { cn } from "@/lib/utils";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "./ui/resizable";
import { WorkspaceFileSearch } from "./workspace-file-search";

interface AppSidePanelProps {
  workspacePath: string;
}
function AppSidePanel({ workspacePath }: AppSidePanelProps) {
  const [sheetOpen, setSheetOpen] = useAtom(sidePanelOpenAtom);
  const [fileOpenRequest, setFileOpenRequest] = useAtom(
    workspaceFileOpenRequestAtom,
  );
  const [dirName, setDirName] = useState("No workspace selected");
  const [branchName, setBranchName] = useState<string | null>(null);
  const [changes, setChanges] = useState<WorkspaceChange[]>([]);
  const [changesLoading, setChangesLoading] = useState(false);
  const [changingStagePath, setChangingStagePath] = useState<string>();
  const [stagingAll, setStagingAll] = useState(false);
  const [commitMessage, setCommitMessage] = useState("");
  const [committing, setCommitting] = useState(false);
  const [navigatorView, setNavigatorView] = useState<"files" | "changes">(
    "files",
  );
  const [fileSearchQuery, setFileSearchQuery] = useState("");
  const [selectedLine, setSelectedLine] = useState<number>();
  useEffect(() => {
    if (!workspacePath) {
      queueMicrotask(() => {
        setDirName("No workspace selected");
        setBranchName(null);
      });
      return;
    }

    void daemonApi
      .getWorkspaceInfo(workspacePath)
      .then((workspace) => {
        setDirName(workspace.displayName);
        setBranchName(workspace.branchName);
      })
      .catch(() => {
        setDirName("Workspace");
        setBranchName(null);
      });
  }, [workspacePath]);

  useEffect(() => {
    queueMicrotask(() => setFileSearchQuery(""));
  }, [workspacePath]);

  const workspaceFileTree = useWorkspaceFileTree(
    sheetOpen,
    workspacePath,
    fileSearchQuery,
  );

  useEffect(() => {
    if (!fileOpenRequest || !workspacePath) return;
    queueMicrotask(() => {
      workspaceFileTree.setSelectedPath(fileOpenRequest.path);
      workspaceFileTree.setExpanded(
        new Set(
          fileOpenRequest.path
            .split("/")
            .slice(0, -1)
            .map((_, index, parts) => parts.slice(0, index + 1).join("/")),
        ),
      );
      setSelectedLine(fileOpenRequest.line);
      setNavigatorView("files");
      setFileSearchQuery("");
      setFileOpenRequest(null);
    });
  }, [fileOpenRequest, setFileOpenRequest, workspacePath]);

  const selectPath: typeof workspaceFileTree.setSelectedPath = useCallback(
    (path) => {
      setSelectedLine(undefined);
      workspaceFileTree.setSelectedPath(path);
    },
    [workspaceFileTree.setSelectedPath],
  );
  const refreshChanges = useCallback(async () => {
    if (!workspacePath) {
      setChanges([]);
      return;
    }
    setChangesLoading(true);
    try {
      setChanges(await daemonApi.listWorkspaceChanges(workspacePath));
    } catch {
      setChanges([]);
    } finally {
      setChangesLoading(false);
    }
  }, [workspacePath]);

  useEffect(() => {
    if (sheetOpen) queueMicrotask(() => void refreshChanges());
  }, [refreshChanges, sheetOpen]);

  const changesByPath = useMemo(
    () => new Map(changes.map((change) => [change.path, change])),
    [changes],
  );

  const [diff, setDiff] = useState<WorkspaceDiff>();

  const stageFile = useCallback(
    async (path: string) => {
      setChangingStagePath(path);
      try {
        await daemonApi.stageWorkspaceFile(workspacePath, path);
        await refreshChanges();
        if (workspaceFileTree.selectedPath === path) {
          setDiff(await daemonApi.getWorkspaceFileDiff(workspacePath, path));
        }
      } catch (cause) {
        toast.error(
          cause instanceof Error ? cause.message : "Could not stage file.",
        );
      } finally {
        setChangingStagePath(undefined);
      }
    },
    [refreshChanges, workspaceFileTree.selectedPath, workspacePath],
  );

  const unstageFile = useCallback(
    async (path: string) => {
      setChangingStagePath(path);
      try {
        await daemonApi.unstageWorkspaceFile(workspacePath, path);
        await refreshChanges();
        if (workspaceFileTree.selectedPath === path) {
          setDiff(await daemonApi.getWorkspaceFileDiff(workspacePath, path));
        }
      } catch (cause) {
        toast.error(
          cause instanceof Error ? cause.message : "Could not unstage file.",
        );
      } finally {
        setChangingStagePath(undefined);
      }
    },
    [refreshChanges, workspaceFileTree.selectedPath, workspacePath],
  );

  const stageAllFiles = useCallback(async () => {
    setStagingAll(true);
    try {
      await daemonApi.stageAllWorkspaceFiles(workspacePath);
      await refreshChanges();
      if (workspaceFileTree.selectedPath) {
        setDiff(
          await daemonApi.getWorkspaceFileDiff(
            workspacePath,
            workspaceFileTree.selectedPath,
          ),
        );
      }
    } catch (cause) {
      toast.error(
        cause instanceof Error ? cause.message : "Could not stage files.",
      );
    } finally {
      setStagingAll(false);
    }
  }, [refreshChanges, workspaceFileTree.selectedPath, workspacePath]);

  const commitChanges = useCallback(async () => {
    const message = commitMessage.trim();
    if (!message) return;
    setCommitting(true);
    try {
      await daemonApi.commitWorkspaceChanges(workspacePath, message);
      setCommitMessage("");
      const nextChanges = await daemonApi.listWorkspaceChanges(workspacePath);
      setChanges(nextChanges);
      const selectedPath = workspaceFileTree.selectedPath;
      if (
        selectedPath &&
        nextChanges.some((change) => change.path === selectedPath)
      ) {
        setDiff(
          await daemonApi.getWorkspaceFileDiff(workspacePath, selectedPath),
        );
      } else {
        workspaceFileTree.setSelectedPath(undefined);
        setDiff(undefined);
      }
      toast.success("Changes committed.");
    } catch (cause) {
      toast.error(
        cause instanceof Error ? cause.message : "Could not commit changes.",
      );
    } finally {
      setCommitting(false);
    }
  }, [commitMessage, workspaceFileTree, workspacePath]);

  return (
    <Sheet open={sheetOpen} onOpenChange={setSheetOpen} modal={false}>
      {/* Keep resize drags from being treated as outside clicks by the sheet. */}
      <SheetContent
        side="right"
        resizable
        onPointerDownOutside={(event) => event.preventDefault()}
        className={cn(
          workspaceFileTree.selectedPath
            ? "w-280 min-w-[min(100vw-50rem,70rem)] max-w-[80vw]!"
            : "w-40 min-w-[min(100vw-30rem,10rem)] max-w-[33vw]!",
        )}
        showCloseButton={false}
      >
        <SheetHeader className="relative flex-row items-center gap-2 px-3 py-1 select-none">
          <SheetTitle className="flex min-w-0 flex-1 flex-row items-center gap-1 overflow-hidden text-xs font-bold">
            <span className="truncate">{dirName || ""}</span>
            {branchName && (
              <span
                className="inline-flex min-w-0 max-w-24 shrink items-center gap-1 rounded bg-muted px-1.5 py-[3.5px] font-mono text-[0.65rem] font-normal text-muted-foreground"
                title={`Git branch: ${branchName}`}
              >
                <GitBranch className="size-3 shrink-0" />
                <span className="truncate">{branchName}</span>
              </span>
            )}
          </SheetTitle>
          {workspaceFileTree.selectedPath && (
            <div className="flex min-w-0 flex-1 items-center gap-1">
              <FileDiff
                className={cn(
                  "size-4 shrink-0 text-muted-foreground",
                  diff?.status === "M" && diff?.comparison === "Unstaged"
                    ? "text-orange-500"
                    : diff?.status === "M" && diff?.comparison === "Staged"
                      ? "text-green-500"
                      : "",
                )}
              />
              <span className="min-w-0 truncate font-mono text-xs">
                {workspaceFileTree.selectedPath || ""}
              </span>
            </div>
          )}
          <div className="ml-auto flex shrink-0 items-center">
            <Button
              className="gap-1 px-2"
              variant={navigatorView === "changes" ? "secondary" : "ghost"}
              size="sm"
              onClick={() =>
                setNavigatorView((view) =>
                  view === "changes" ? "files" : "changes",
                )
              }
              aria-pressed={navigatorView === "changes"}
              aria-label={
                navigatorView === "changes"
                  ? "Show all workspace files"
                  : "Show changed files"
              }
              title={
                navigatorView === "changes"
                  ? "Show all files"
                  : "Show changed files"
              }
            >
              <ListFilter className="size-3.5" />
              <span className="font-mono text-[0.65rem] tabular-nums">
                {changes.length}
              </span>
            </Button>
            <WorkspaceFileSearch
              active={sheetOpen}
              disabled={!workspaceFileTree.validWorkspace}
              value={fileSearchQuery}
              onValueChange={setFileSearchQuery}
            />
            <Button
              disabled={
                !workspaceFileTree.validWorkspace || workspaceFileTree.isLoading
              }
              onClick={() => {
                void workspaceFileTree.refresh();
                void refreshChanges();
              }}
              size="icon-sm"
              variant="ghost"
            >
              <RefreshCw
                className={cn(
                  "size-4",
                  workspaceFileTree.isLoading && "animate-spin",
                )}
              />
            </Button>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => setSheetOpen(false)}
            >
              <XIcon className="size-4" />
              <span className="sr-only">Close</span>
            </Button>
          </div>
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
              {navigatorView === "files" ? (
                <WorkspaceFileTree
                  {...workspaceFileTree}
                  changes={changesByPath}
                  searchQuery={fileSearchQuery}
                  setSelectedPath={selectPath}
                />
              ) : (
                <WorkspaceChangedFiles
                  changes={changes}
                  isLoading={changesLoading}
                  searchQuery={fileSearchQuery}
                  selectedPath={workspaceFileTree.selectedPath}
                  setSelectedPath={selectPath}
                  onStage={stageFile}
                  onUnstage={unstageFile}
                  changingStagePath={changingStagePath}
                  commitMessage={commitMessage}
                  onCommitMessageChange={setCommitMessage}
                  onStageAll={stageAllFiles}
                  stagingAll={stagingAll}
                  onCommit={commitChanges}
                  committing={committing}
                  validWorkspace={workspaceFileTree.validWorkspace}
                />
              )}
            </div>
          </ResizablePanel>
          <ResizableHandle withHandle />
          {!!workspaceFileTree.selectedPath && (
            <ResizablePanel minSize="0px" className="min-h-0 overflow-hidden">
              <WorkspaceDiffViewer
                selectedPath={workspaceFileTree.selectedPath}
                selectedLine={selectedLine}
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
