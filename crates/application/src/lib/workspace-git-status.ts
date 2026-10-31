import type { WorkspaceChange } from "@/types";

export function gitStatusClass(status?: string): string | undefined {
  if (!status) return undefined;
  if (status === "A" || status === "?" || status === "C")
    return "text-emerald-600 dark:text-emerald-400";
  if (status === "D" || status === "U") return "text-red-600 dark:text-red-400";
  if (status === "R") return "text-blue-600 dark:text-blue-400";
  return "text-amber-700 dark:text-amber-400";
}

/** Include every ancestor, even when the changed file is not in the visible tree. */
export function changedDirectoryStatuses(changes: Iterable<WorkspaceChange>) {
  const directories = new Map<string, string>();
  for (const change of changes) {
    const parts = change.path.replaceAll("\\", "/").split("/");
    for (let index = 1; index < parts.length; index++) {
      const directory = parts.slice(0, index).join("/");
      const previous = directories.get(directory);
      // Mixed statuses use the general changed color, independent of input order.
      directories.set(
        directory,
        previous && previous !== change.status ? "M" : change.status,
      );
    }
  }
  return directories;
}

/** Git may report a whole untracked directory instead of individual files. */
export function fileGitStatus(
  path: string,
  changes: Map<string, WorkspaceChange>,
) {
  const direct = changes.get(path)?.status;
  if (direct) return direct;
  const parts = path.split("/");
  for (let index = parts.length - 1; index > 0; index--) {
    const directory = changes.get(`${parts.slice(0, index).join("/")}/`);
    if (directory) return directory.status;
  }
  return undefined;
}
