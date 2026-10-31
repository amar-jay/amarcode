import { daemonApi, type DaemonConnectionInfo } from "@/api";

export type ConnectionForm = {
  mode: "local" | "websocket";
  url: string;
  token: string;
};

export const emptyConnectionForm: ConnectionForm = {
  mode: "local",
  url: "",
  token: "",
};

/** Human-readable host identity for a remote daemon session. */
export function connectionIdentity(info: DaemonConnectionInfo): string | null {
  if (!info.remote) return null;
  if (info.mode === "ssh" && info.ssh) {
    const user = info.ssh.user ? `${info.ssh.user}@` : "";
    const port = info.ssh.port ? `:${info.ssh.port}` : "";
    return `${user}${info.ssh.host}${port}`;
  }
  try {
    return new URL(info.url).host || info.url;
  } catch {
    return info.url.split("/")[2] ?? info.url;
  }
}

export function connectionForm(info: DaemonConnectionInfo): ConnectionForm {
  return {
    ...emptyConnectionForm,
    mode: info.mode === "local" ? "local" : "websocket",
    // A legacy SSH tunnel's local forwarding URL is not a public daemon URL.
    url: info.mode === "websocket" ? info.url : "",
  };
}

export function canReuseToken(
  form: ConnectionForm,
  saved: DaemonConnectionInfo | null,
) {
  return (
    !!saved?.hasToken &&
    form.mode === "websocket" &&
    saved.mode === "websocket" &&
    saved.url === form.url.trim()
  );
}

/** Native verification and persistence finish before the caller reloads the app. */
export async function applyConnection(form: ConnectionForm) {
  if (form.mode === "websocket" && !form.url.trim())
    throw new Error("Enter the remote WebSocket URL.");
  return daemonApi.setConnection(
    form.mode === "local" ? "" : form.url.trim(),
    form.mode === "local" ? "" : form.token.trim(),
  );
}

export function connectionError(cause: unknown): string {
  const message = cause instanceof Error ? cause.message : String(cause);
  if (/401|Unauthorized/i.test(message))
    return "The daemon rejected the access token. Check the token configured on the remote host.";
  return message;
}
