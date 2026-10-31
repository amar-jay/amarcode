import { useEffect, useId, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { daemonApi, type DaemonConnectionInfo } from "@/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  NativeSelect,
  NativeSelectOption,
} from "@/components/ui/native-select";
import { useDaemonConnectionStatus } from "@/hooks/use-daemon-connection-status";
import {
  applyConnection,
  canReuseToken,
  connectionError,
  connectionForm,
  emptyConnectionForm,
  type ConnectionForm,
} from "@/lib/daemon-connection-form";

/** Also shown before bootstrap, so a local service is never required to connect. */
export function DaemonConnectionSettings() {
  const id = useId();
  const [form, setForm] = useState<ConnectionForm>(emptyConnectionForm);
  const [saved, setSaved] = useState<DaemonConnectionInfo | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const status = useDaemonConnectionStatus();
  const patch = (value: Partial<ConnectionForm>) =>
    setForm((current) => ({ ...current, ...value }));
  useEffect(() => {
    let disposed = false;
    void daemonApi
      .getConnection()
      .then((value) => {
        if (disposed) return;
        setSaved(value);
        setForm(connectionForm(value));
        setLoaded(true);
      })
      .catch((cause) => {
        if (disposed) return;
        setError(connectionError(cause));
        setLoaded(true); // Local mode can repair corrupt preferences.
      });
    return () => {
      disposed = true;
    };
  }, []);
  const chooseIdentity = async () => {
    try {
      const path = await open({
        directory: false,
        multiple: false,
        title: "Choose an SSH identity file",
      });
      if (typeof path === "string") patch({ identityFile: path });
    } catch (cause) {
      setError(connectionError(cause));
    }
  };
  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const value = await applyConnection(form);
      localStorage.setItem("amarcode-connection-scope", value.scope);
      window.location.reload();
    } catch (cause) {
      setError(connectionError(cause));
      setBusy(false);
    }
  };
  const reuseToken = canReuseToken(form, saved);
  const missingTarget =
    (form.mode === "websocket" && !form.url.trim()) ||
    (form.mode === "ssh" && !form.host.trim());
  return (
    <form
      className="space-y-4 text-left"
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
      aria-busy={busy}
    >
      <div className="space-y-1">
        <h2 className="text-base font-medium">Daemon connection</h2>
        <p className="text-xs text-muted-foreground">
          Choose where your projects and agents run.
        </p>
      </div>
      <fieldset disabled={!loaded || busy} className="min-w-0 space-y-3">
        <legend className="sr-only">Connection details</legend>
        <label className="block space-y-1 text-sm" htmlFor={`${id}-mode`}>
          <span>Connection type</span>
          <NativeSelect
            id={`${id}-mode`}
            className="w-full"
            value={form.mode}
            onChange={(event) => {
              patch({
                mode: event.target.value as ConnectionForm["mode"],
                token: "",
              });
              setError(null);
            }}
          >
            <NativeSelectOption value="local">Local daemon</NativeSelectOption>
            <NativeSelectOption value="websocket">
              Remote WebSocket
            </NativeSelectOption>
            <NativeSelectOption value="ssh">SSH</NativeSelectOption>
          </NativeSelect>
        </label>
        {form.mode === "websocket" && (
          <label className="block space-y-1 text-sm">
            <span>WebSocket URL</span>
            <Input
              value={form.url}
              onChange={(event) => patch({ url: event.target.value })}
              placeholder="wss://daemon.example.com/rpc"
              autoCapitalize="none"
              spellCheck={false}
            />
          </label>
        )}
        {form.mode === "ssh" && (
          <>
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <label className="block space-y-1 text-sm">
                <span>SSH host or alias</span>
                <Input
                  value={form.host}
                  onChange={(event) => patch({ host: event.target.value })}
                  placeholder="dev-box"
                  autoCapitalize="none"
                  spellCheck={false}
                />
              </label>
              <label className="block space-y-1 text-sm">
                <span>
                  Username{" "}
                  <span className="text-muted-foreground">(optional)</span>
                </span>
                <Input
                  value={form.user}
                  onChange={(event) => patch({ user: event.target.value })}
                  placeholder="From SSH config"
                  autoCapitalize="none"
                  spellCheck={false}
                />
              </label>
              <label className="block space-y-1 text-sm">
                <span>
                  SSH port{" "}
                  <span className="text-muted-foreground">(optional)</span>
                </span>
                <Input
                  value={form.port}
                  onChange={(event) => patch({ port: event.target.value })}
                  placeholder="From SSH config, otherwise 22"
                  inputMode="numeric"
                />
              </label>
              <label className="block space-y-1 text-sm">
                <span>Daemon port</span>
                <Input
                  value={form.daemonPort}
                  onChange={(event) =>
                    patch({ daemonPort: event.target.value })
                  }
                  placeholder="43721"
                  inputMode="numeric"
                />
              </label>
            </div>
            <div className="space-y-1 text-sm">
              <label htmlFor={`${id}-identity`}>
                Identity file{" "}
                <span className="text-muted-foreground">(optional)</span>
              </label>
              <div className="flex items-center gap-2">
                <Input
                  id={`${id}-identity`}
                  value={form.identityFile}
                  onChange={(event) =>
                    patch({ identityFile: event.target.value })
                  }
                  placeholder="Use SSH config or agent"
                  autoCapitalize="none"
                  spellCheck={false}
                />
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => void chooseIdentity()}
                >
                  Browse…
                </Button>
              </div>
            </div>
            <p className="text-xs text-muted-foreground">
              Uses your SSH config and agent. Verify the host’s fingerprint with
              your SSH client first; unknown or changed host keys are rejected.
              Unlock encrypted keys in your SSH agent.
            </p>
          </>
        )}
        {form.mode !== "local" && (
          <>
            <label className="block space-y-1 text-sm">
              <span>Daemon access token</span>
              <Input
                type="password"
                value={form.token}
                onChange={(event) => patch({ token: event.target.value })}
                placeholder={
                  reuseToken
                    ? "Leave blank to keep the saved token for this connection"
                    : "Token from the remote daemon"
                }
                autoComplete="off"
              />
            </label>
            <p className="text-xs text-muted-foreground">
              Project folders, Git commands, and agents run on the remote host.{" "}
              {form.mode === "ssh"
                ? "The daemon must already be running on that host’s loopback address."
                : "Enter the remote URL ending in /rpc."}
            </p>
          </>
        )}
      </fieldset>
      {saved?.mode === "ssh" && status && (
        <div
          role="status"
          aria-live="polite"
          className="rounded-md border border-border bg-muted/30 px-3 py-2 text-xs"
        >
          <p className="font-medium">
            Current SSH connection ·{" "}
            {status.status === "connected"
              ? "Connected"
              : status.status === "connecting"
                ? "Connecting…"
                : status.status === "reconnecting"
                  ? "Reconnecting…"
                  : "Waiting…"}
          </p>
          <p className="mt-1 break-words text-muted-foreground">
            {saved.ssh?.host}
            {status.retryInMs !== null
              ? ` · Retry in ${(status.retryInMs / 1_000).toFixed(1)}s`
              : ""}
          </p>
          {status.error && (
            <p className="mt-1 break-words text-destructive">
              {connectionError(status.error)}
            </p>
          )}
        </div>
      )}
      <p className="text-xs text-muted-foreground">
        Save your editor drafts before switching. Applying a connection reloads
        the app; existing agent runs continue on their host.
      </p>
      {error && (
        <p role="alert" className="break-words text-sm text-destructive">
          {error}
        </p>
      )}
      <Button type="submit" disabled={!loaded || busy || missingTarget}>
        {busy ? "Verifying connection…" : "Verify and apply"}
      </Button>
    </form>
  );
}
