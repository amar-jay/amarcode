import { useCallback, useEffect, useMemo, useState } from "react";
import {
  FileIcon,
  FileQuestion,
  FolderOpen,
  LoaderCircle,
  RefreshCw,
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
import { toast } from "sonner";

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

export function useWorkspaceFileTree(active: boolean, workspacePath: string) {
  const [files, setFiles] = useState<string[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [selectedPath, setSelectedPath] = useState<string>();
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const validWorkspace = !!workspacePath;

  const refresh = useCallback(async () => {
    if (!workspacePath) {
      setFiles([]);
      return;
    }

    setIsLoading(true);
    try {
      setFiles(await daemonApi.listWorkspaceFiles(workspacePath));
    } catch (cause) {
      toast(cause instanceof Error ? cause.message : "Could not load files.", {
        dismissible: false,
        action: {
          label: "Retry",
          onClick: () => void refresh(),
        },
      });
    } finally {
      setIsLoading(false);
    }
  }, [workspacePath]);

  useEffect(() => {
    if (active) void refresh();
  }, [active, refresh]);

  useEffect(() => {
    setSelectedPath(undefined);
    setExpanded(new Set());
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
  } = treeState;

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
        ) : tree.length === 0 ? (
          <div className="px-2 py-6 text-center text-xs text-muted-foreground">
            No visible files in this workspace.
          </div>
        ) : (
          <FileTree
            className="rounded-md border-0 bg-transparent text-xs pl-0"
            expanded={expanded}
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
