import {
  daemonApi,
  type DaemonConnectionInfo,
  type SshConnectionConfig,
} from "@/api";

export type ConnectionForm = {
  mode: DaemonConnectionInfo["mode"];
  url: string;
  host: string;
  user: string;
  port: string;
  identityFile: string;
  daemonPort: string;
  token: string;
};

export const emptyConnectionForm: ConnectionForm = {
  mode: "local",
  url: "",
  host: "",
  user: "",
  port: "",
  identityFile: "",
  daemonPort: "43721",
  token: "",
};

export function connectionForm(info: DaemonConnectionInfo): ConnectionForm {
  return {
    ...emptyConnectionForm,
    mode: info.mode,
    url: info.url,
    host: info.ssh?.host ?? "",
    user: info.ssh?.user ?? "",
    port: info.ssh?.port?.toString() ?? "",
    identityFile: info.ssh?.identityFile ?? "",
    daemonPort: info.ssh?.daemonPort.toString() ?? "43721",
  };
}

function parsePort(
  value: string,
  name: string,
  optional = false,
): number | null {
  const text = value.trim();
  if (!text && optional) return null;
  if (!/^\d+$/.test(text) || Number(text) < 1 || Number(text) > 65535)
    throw new Error(`${name} must be a whole number between 1 and 65535.`);
  return Number(text);
}

export function sshConfig(form: ConnectionForm): SshConnectionConfig {
  if (!form.host.trim()) throw new Error("Enter an SSH host or config alias.");
  return {
    host: form.host.trim(),
    user: form.user.trim() || null,
    port: parsePort(form.port, "SSH port", true),
    identityFile: form.identityFile.trim() || null,
    daemonPort: parsePort(form.daemonPort, "Daemon port")!,
  };
}

export function canReuseToken(
  form: ConnectionForm,
  saved: DaemonConnectionInfo | null,
) {
  if (!saved?.hasToken || saved.mode !== form.mode || form.mode === "local")
    return false;
  if (form.mode === "websocket") return saved.url === form.url.trim();
  try {
    const config = sshConfig(form);
    return (
      saved.ssh !== null &&
      (Object.keys(config) as (keyof SshConnectionConfig)[]).every(
        (key) => config[key] === saved.ssh?.[key],
      )
    );
  } catch {
    return false;
  }
}

/** Native verification and persistence finish before the caller reloads the app. */
export async function applyConnection(form: ConnectionForm) {
  if (form.mode === "ssh")
    return daemonApi.setSshConnection(sshConfig(form), form.token.trim());
  if (form.mode === "websocket" && !form.url.trim())
    throw new Error("Enter the remote WebSocket URL.");
  return daemonApi.setConnection(
    form.mode === "local" ? "" : form.url.trim(),
    form.mode === "local" ? "" : form.token.trim(),
  );
}

export function connectionError(cause: unknown): string {
  const message = cause instanceof Error ? cause.message : String(cause);
  if (
    /host key verification failed|REMOTE HOST IDENTIFICATION HAS CHANGED/i.test(
      message,
    )
  )
    return "SSH could not verify this host’s key. Verify its fingerprint with your SSH client before connecting from Amarcode.";
  if (/Cannot start OpenSSH/i.test(message))
    return "OpenSSH could not start. Install the SSH client and make sure ssh is available on your PATH.";
  if (/Permission denied/i.test(message))
    return "SSH authentication failed. Check the username and identity file, or unlock your key in the SSH agent.";
  if (/401|Unauthorized/i.test(message))
    return "The daemon rejected the access token. Check the token configured on the remote host.";
  return message;
}
