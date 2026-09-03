import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  FileIcon,
  GitCommitHorizontal,
  LoaderCircle,
  Minus,
  Plus,
} from "lucide-react";
import { daemonApi, type WorkspaceChange } from "@/api";
import {
  FileTree,
  FileTreeFile,
  FileTreeFolder,
  FileTreeIcon,
  FileTreeName,
} from "@/components/ai-elements/file-tree";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { toast } from "sonner";
import { Tooltip, TooltipContent, TooltipTrigger } from "./ui/tooltip";

type FileNode = {
  kind: "file";
  name: string;
  path: string;
};

type FolderNode = {
  kind: "folder";
  name: string;
  path: string;
  children: TreeNode[];
};

type TreeNode = FileNode | FolderNode;

const compareNodes = (left: TreeNode, right: TreeNode) => {
  if (left.kind !== right.kind) return left.kind === "folder" ? -1 : 1;
  return left.name.localeCompare(right.name, undefined, { numeric: true });
};

function makeTree(paths: string[]): TreeNode[] {
  const root: FolderNode = { children: [], kind: "folder", name: "", path: "" };

  for (const path of paths) {
    const parts = path.replaceAll("\\", "/").split("/").filter(Boolean);
    if (!parts.length) continue;

    let parent = root;
    for (const [index, name] of parts.entries()) {
      const nodePath = parts.slice(0, index + 1).join("/");
      const isFile = index === parts.length - 1;
      let node = parent.children.find((child) => child.path === nodePath);

      if (!node) {
        node = isFile
          ? { kind: "file", name, path: nodePath }
          : { children: [], kind: "folder", name, path: nodePath };
        parent.children.push(node);
      }

      if (!isFile && node.kind === "folder") parent = node;
    }
  }

  const sortTree = (nodes: TreeNode[]): TreeNode[] =>
    nodes
      .sort(compareNodes)
      .map((node) =>
        node.kind === "folder"
          ? { ...node, children: sortTree(node.children) }
          : node,
      );

  return sortTree(root.children);
}

function collectFolderPaths(nodes: TreeNode[], paths = new Set<string>()) {
  for (const node of nodes) {
    if (node.kind === "folder") {
      paths.add(node.path);
      collectFolderPaths(node.children, paths);
    }
  }
  return paths;
}

function TreeNodes({
  changes,
  nodes,
}: {
  changes: Map<string, WorkspaceChange>;
  nodes: TreeNode[];
}) {
  return nodes.map((node) =>
    node.kind === "folder" ? (
      <FileTreeFolder key={node.path} name={node.name} path={node.path}>
        <TreeNodes changes={changes} nodes={node.children} />
      </FileTreeFolder>
    ) : (
      <FileTreeFile key={node.path} name={node.name} path={node.path}>
        <span className="size-4 shrink-0" />
        <FileTreeIcon>
          <FileIcon className="size-4 text-muted-foreground" />
        </FileTreeIcon>
        <FileTreeName>{node.name}</FileTreeName>
        {changes.get(node.path) && (
          <span className="ml-auto rounded bg-muted px-1 font-mono text-[0.6rem] font-semibold text-muted-foreground">
            {changes.get(node.path)?.status}
          </span>
        )}
      </FileTreeFile>
    ),
  );
}

export function useWorkspaceFileTree(
  active: boolean,
  workspacePath: string,
  searchQuery: string,
) {
  const [files, setFiles] = useState<string[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [selectedPath, setSelectedPath] = useState<string>();
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const requestId = useRef(0);
  const validWorkspace = !!workspacePath;
  const normalizedSearchQuery = searchQuery.trim();

  const refresh = useCallback(async () => {
    const currentRequestId = ++requestId.current;
    if (!workspacePath) {
      setFiles([]);
      setIsLoading(false);
      return;
    }

    setIsLoading(true);
    try {
      const files = await daemonApi.listWorkspaceFiles(
        workspacePath,
        normalizedSearchQuery || undefined,
      );
      if (currentRequestId === requestId.current) setFiles(files);
    } catch (cause) {
      if (currentRequestId !== requestId.current) return;
      toast(cause instanceof Error ? cause.message : "Could not load files.", {
        dismissible: false,
        action: {
          label: "Retry",
          onClick: () => void refresh(),
        },
      });
    } finally {
      if (currentRequestId === requestId.current) setIsLoading(false);
    }
  }, [normalizedSearchQuery, workspacePath]);

  useEffect(() => {
    if (!active) return;
    const timeout = window.setTimeout(
      () => void refresh(),
      normalizedSearchQuery ? 150 : 0,
    );
    return () => {
      window.clearTimeout(timeout);
      requestId.current += 1;
    };
  }, [active, normalizedSearchQuery, refresh]);

  useEffect(() => {
    queueMicrotask(() => {
      setSelectedPath(undefined);
      setExpanded(new Set());
    });
  }, [workspacePath]);

  const tree = useMemo(() => makeTree(files), [files]);
  return {
    files,
    isLoading,
    selectedPath,
    setSelectedPath,
    expanded,
    setExpanded,
    refresh,
    tree,
    validWorkspace,
    searchQuery,
  };
}

export function WorkspaceFileTree({
  changes = new Map(),
  ...treeState
}: ReturnType<typeof useWorkspaceFileTree> & {
  changes?: Map<string, WorkspaceChange>;
}) {
  const {
    files,
    isLoading,
    selectedPath,
    setSelectedPath,
    expanded,
    setExpanded,
    tree,
    validWorkspace,
    searchQuery,
  } = treeState;
  const hasSearchQuery = !!searchQuery.trim();
  const visibleExpanded = useMemo(
    () => (hasSearchQuery ? collectFolderPaths(tree) : expanded),
    [expanded, hasSearchQuery, tree],
  );

  return (
    <section
      className="flex min-h-0 flex-1 flex-col"
      aria-label="Workspace files"
    >
      <div className="min-h-0 flex-1 overflow-y-auto pb-5">
        {!validWorkspace ? (
          <p className="px-2 py-6 text-center text-xs text-muted-foreground">
            Choose a workspace to browse its files.
          </p>
        ) : isLoading && files.length === 0 ? (
          <div className="flex items-center justify-center gap-2 px-2 py-6 text-xs text-muted-foreground">
            <LoaderCircle className="size-4 animate-spin" /> Loading files
          </div>
        ) : tree.length === 0 && hasSearchQuery ? (
          <div className="px-2 py-6 text-center text-xs text-muted-foreground">
            No matching files.
          </div>
        ) : tree.length === 0 ? (
          <div className="px-2 py-6 text-center text-xs text-muted-foreground">
            No visible files in this workspace.
          </div>
        ) : (
          <FileTree
            className="rounded-md border-0 bg-transparent text-xs pl-0"
            expanded={visibleExpanded}
            onExpandedChange={setExpanded}
            onSelect={setSelectedPath}
            selectedPath={selectedPath}
          >
            <TreeNodes changes={changes} nodes={tree} />
          </FileTree>
        )}
      </div>
      {/* {selectedPath && (
        <div className="flex items-center gap-2 border-t px-2 py-2 text-xs text-muted-foreground">
          <FolderOpen className="size-3.5 shrink-0" />
          <span className="truncate font-mono" title={selectedPath}>
            {selectedPath}
          </span>
        </div>
      )} */}
    </section>
  );
}

const changeLabel = (change: WorkspaceChange) => {
  if (change.staged && change.unstaged) return "staged + unstaged";
  if (change.staged) return "staged";
  return "unstaged";
};

export function WorkspaceChangedFiles({
  changes,
  isLoading,
  searchQuery,
  selectedPath,
  setSelectedPath,
  onStage,
  onUnstage,
  changingStagePath,
  commitMessage,
  onCommitMessageChange,
  onStageAll,
  stagingAll,
  onCommit,
  committing,
  validWorkspace,
}: {
  changes: WorkspaceChange[];
  isLoading: boolean;
  searchQuery: string;
  selectedPath?: string;
  setSelectedPath: (path: string) => void;
  onStage: (path: string) => Promise<void>;
  onUnstage: (path: string) => Promise<void>;
  changingStagePath?: string;
  commitMessage: string;
  onCommitMessageChange: (message: string) => void;
  onStageAll: () => Promise<void>;
  stagingAll: boolean;
  onCommit: () => Promise<void>;
  committing: boolean;
  validWorkspace: boolean;
}) {
  const normalizedQuery = searchQuery.trim().toLocaleLowerCase();
  const visibleChanges = useMemo(
    () =>
      normalizedQuery
        ? changes.filter((change) =>
            change.path.toLocaleLowerCase().includes(normalizedQuery),
          )
        : changes,
    [changes, normalizedQuery],
  );
  const stagedCount = changes.filter((change) => change.staged).length;
  const unstagedCount = changes.filter((change) => change.unstaged).length;

  return (
    <section
      className="flex min-h-0 flex-1 flex-col"
      aria-label="Changed files"
    >
      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-3 pt-1">
        {!validWorkspace ? (
          <p className="px-2 py-6 text-center text-xs text-muted-foreground">
            Choose a workspace to see changed files.
          </p>
        ) : isLoading && changes.length === 0 ? (
          <div className="flex items-center justify-center gap-2 px-2 py-6 text-xs text-muted-foreground">
            <LoaderCircle className="size-4 animate-spin" /> Loading changes
          </div>
        ) : visibleChanges.length === 0 && normalizedQuery ? (
          <p className="px-2 py-6 text-center text-xs text-muted-foreground">
            No changed files match your search.
          </p>
        ) : visibleChanges.length === 0 ? (
          <p className="px-2 py-6 text-center text-xs text-muted-foreground">
            No uncommitted changes.
          </p>
        ) : (
          <div className="flex flex-col gap-0.5" role="tree">
            {visibleChanges.map((change) => {
              const separator = change.path.lastIndexOf("/");
              const name = change.path.slice(separator + 1);
              const directory =
                separator === -1
                  ? "Project root"
                  : change.path.slice(0, separator);
              const selected = selectedPath === change.path;

              return (
                <div
                  key={change.path}
                  role="treeitem"
                  aria-selected={selected}
                  className={cn(
                    "group flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
                    selected &&
                      "bg-accent text-accent-foreground hover:bg-accent",
                  )}
                  title={change.path}
                >
                  <button
                    type="button"
                    className="flex min-w-0 flex-1 items-center gap-2 text-left outline-none"
                    onClick={() => setSelectedPath(change.path)}
                  >
                    <span
                      className={cn(
                        "flex size-6 shrink-0 items-center justify-center rounded font-mono text-xs font-bold",
                        change.status === "D"
                          ? "text-red-500"
                          : change.status === "A"
                            ? "text-emerald-500"
                            : "text-amber-500",
                      )}
                      aria-label={`Status ${change.status}`}
                    >
                      {change.status}
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-mono text-xs text-foreground">
                        {name}
                      </span>
                      <span className="block truncate font-mono text-[0.65rem] text-muted-foreground">
                        {directory}
                      </span>
                    </span>
                  </button>
                  {change.unstaged || change.staged ? (
                    <button
                      type="button"
                      className="flex size-6 shrink-0 items-center justify-center rounded text-muted-foreground opacity-70 outline-none transition-colors hover:bg-background hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring group-hover:opacity-100"
                      disabled={changingStagePath === change.path}
                      aria-label={`${change.unstaged ? "Stage" : "Unstage"} ${change.path}`}
                      title={change.unstaged ? "Stage file" : "Unstage file"}
                      onClick={() =>
                        void (change.unstaged
                          ? onStage(change.path)
                          : onUnstage(change.path))
                      }
                    >
                      {changingStagePath === change.path ? (
                        <LoaderCircle className="size-3.5 animate-spin" />
                      ) : change.unstaged ? (
                        <Plus className="size-4" />
                      ) : (
                        <Minus className="size-4" />
                      )}
                    </button>
                  ) : (
                    <span className="shrink-0 text-[0.6rem] text-muted-foreground opacity-70 group-hover:opacity-100">
                      {changeLabel(change)}
                    </span>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
      {validWorkspace && changes.length > 0 && (
        <div className="shrink-0 space-y-2 border-t bg-muted/20 p-2.5">
          <div className="flex items-center justify-between font-mono text-[0.65rem] text-muted-foreground">
            <span>
              {stagedCount} staged · {unstagedCount} unstaged
            </span>
            {/* {unstagedCount > 0 && (
              <button
                type="button"
                className="inline-flex items-center gap-1 rounded px-1 py-0.5 text-foreground/75 outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
                disabled={stagingAll || committing}
                onClick={() => void onStageAll()}
              >
                {stagingAll ? (
                  <LoaderCircle className="size-3 animate-spin" />
                ) : (
                  <Plus className="size-3" />
                )}
                Stage all
              </button>
            )} */}
          </div>
          <form
            className="flex gap-1.5"
            onSubmit={(event) => {
              event.preventDefault();
              void onCommit();
            }}
          >
            <input
              value={commitMessage}
              onChange={(event) => onCommitMessageChange(event.target.value)}
              placeholder="Commit message"
              aria-label="Commit message"
              className="h-8 min-w-0 flex-1 rounded-md border bg-background px-2.5 text-xs outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
            />
            {/* </Button> */}
						<Tooltip>
							<TooltipTrigger asChild>
  					<Button
						variant={"secondary"}
              type="submit"
              size="sm"
							name="Commit"
              className="h-8 gap-1.5 px-2.5"
              onClick={() => void onStageAll()}
              disabled={
								unstagedCount == 0 || stagingAll || committing
              }
            >

                {stagingAll ? (
                  <LoaderCircle className="size-3.5 animate-spin" />
                ) : (
                  <Plus className="size-3.5" />
                )}
            </Button>
							</TooltipTrigger>
							<TooltipContent align="end" side="top">
								Stage all changes
							</TooltipContent>
						</Tooltip>

						<Tooltip>
							<TooltipTrigger asChild>
            <Button
              type="submit"
              size="sm"
							name="Commit"
              className="h-8 gap-1.5 px-2.5"
              disabled={
                stagedCount === 0 || !commitMessage.trim() || committing
              }
            >
              {committing ? (
                <LoaderCircle className="size-3.5 animate-spin" />
              ) : (
                <GitCommitHorizontal className="size-3.5" />
              )}
            </Button>
							</TooltipTrigger>
							<TooltipContent align="end" side="right">
								Commit changes
							</TooltipContent>
						</Tooltip>
          </form>
        </div>
      )}
    </section>
  );
}
