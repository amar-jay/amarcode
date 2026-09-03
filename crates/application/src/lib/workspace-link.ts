export type WorkspaceFileTarget = {
  path: string;
  line?: number;
  column?: number;
};

const EXTERNAL_SCHEME = /^(?:https?|mailto|tel):/i;
const URI_SCHEME = /^[a-z][a-z\d+.-]*:/i;

function decode(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function normalizedPath(value: string): string {
  return value.replaceAll("\\", "/").replace(/\/+$/, "");
}

/** True when a target represents a file/path rather than a web-style link. */
export function looksLikeWorkspaceFileTarget(target: string): boolean {
  const value = target.trim();
  if (!value || value.startsWith("#") || EXTERNAL_SCHEME.test(value)) {
    return false;
  }
  return (
    value.startsWith("/") ||
    value.startsWith("file:") ||
    !URI_SCHEME.test(value)
  );
}

/**
 * Convert an agent-produced absolute, file://, or relative link into a safe
 * workspace-relative path. Lexical containment is followed by a backend file
 * read before navigation, so symlinks/nonexistent paths cannot be opened here.
 */
export function workspaceFileTarget(
  target: string,
  workspaceRoot: string,
): WorkspaceFileTarget | null {
  if (!workspaceRoot || !looksLikeWorkspaceFileTarget(target)) return null;

  let value = target.trim();
  let line: number | undefined;
  let column: number | undefined;

  if (value.startsWith("file:")) {
    try {
      const url = new URL(value);
      value = decode(url.pathname);
      const location = url.hash.match(/^#L(\d+)(?:C(\d+))?$/i);
      if (location) {
        line = Number(location[1]);
        column = location[2] ? Number(location[2]) : undefined;
      }
    } catch {
      return null;
    }
  } else {
    const hashLocation = value.match(/#L(\d+)(?:C(\d+))?$/i);
    if (hashLocation) {
      line = Number(hashLocation[1]);
      column = hashLocation[2] ? Number(hashLocation[2]) : undefined;
      value = value.slice(0, hashLocation.index);
    } else {
      const suffixLocation = value.match(/:(\d+)(?::(\d+))?$/);
      if (suffixLocation) {
        line = Number(suffixLocation[1]);
        column = suffixLocation[2] ? Number(suffixLocation[2]) : undefined;
        value = value.slice(0, suffixLocation.index);
      }
    }
    value = decode(value);
  }

  if (line === undefined) {
    const suffixLocation = value.match(/:(\d+)(?::(\d+))?$/);
    if (suffixLocation) {
      line = Number(suffixLocation[1]);
      column = suffixLocation[2] ? Number(suffixLocation[2]) : undefined;
      value = value.slice(0, suffixLocation.index);
    }
  }

  const root = normalizedPath(workspaceRoot);
  let path = normalizedPath(value);
  if (path.startsWith("/")) {
    if (!path.startsWith(`${root}/`)) return null;
    path = path.slice(root.length + 1);
  } else {
    path = path.replace(/^\.\//, "");
  }

  const parts = path.split("/");
  if (!path || parts.some((part) => !part || part === "." || part === "..")) {
    return null;
  }

  return { path, line, column };
}
