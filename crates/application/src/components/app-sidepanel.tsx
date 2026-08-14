import { useAtom } from "jotai";
import { RefreshCw, XIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { daemonApi } from "@/api";
import { sidePanelOpenAtom } from "@/state";
import { Sheet, SheetContent, SheetHeader, SheetTitle } from "./ui/sheet";
import { useWorkspaceFileTree, WorkspaceFileTree } from "./workspace-file-tree";
import { Button } from "./ui/button";

interface AppSidePanelProps {
  workspacePath: string;
}
function AppSidePanel({ workspacePath }: AppSidePanelProps) {
  const [sheetOpen, setSheetOpen] = useAtom(sidePanelOpenAtom);
  const [dirName, setDirName] = useState("No workspace selected");
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

  return (
    <Sheet open={sheetOpen} onOpenChange={setSheetOpen}>
      <SheetContent
        side="right"
        resizable
        className="w-full sm:max-w-md"
        showCloseButton={false}
      >
        <SheetHeader className="px-3 py-1 flex-row items-baseline">
          <SheetTitle className="text-xs font-bold">{dirName || ""}</SheetTitle>

        <Button
          disabled={!workspaceFileTree.validWorkspace || workspaceFileTree.isLoading}
          onClick={() => void workspaceFileTree.refresh()}
          className="ml-auto"
          size="icon-sm"
          variant="ghost"
        >
          <RefreshCw className={workspaceFileTree.isLoading ? "animate-spin" : ""} />
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

        <WorkspaceFileTree {...workspaceFileTree} />
      </SheetContent>
    </Sheet>
  );
}

export default AppSidePanel;
