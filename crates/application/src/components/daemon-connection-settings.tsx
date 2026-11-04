import { isWeb } from "@/lib/platform";
import { useEffect, useId, useState } from "react";
import { daemonApi, type DaemonConnectionInfo } from "@/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  NativeSelect,
  NativeSelectOption,
} from "@/components/ui/native-select";
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
  if (isWeb)
    return (
      <div className="space-y-3 text-sm">
        <p>Connected directly to your remote daemon.</p>
        <p className="break-all text-xs text-muted-foreground">{saved?.url}</p>
        <Button variant="outline" onClick={() => window.location.reload()}>
          Disconnect / switch connection
        </Button>
      </div>
    );
  const reuseToken = canReuseToken(form, saved);
  const missingTarget = form.mode === "websocket" && !form.url.trim();
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
              Remote daemon
            </NativeSelectOption>
          </NativeSelect>
        </label>
        {form.mode === "websocket" && (
          <label className="block space-y-1 text-sm">
            <span>WebSocket URL</span>
            <Input
              value={form.url}
              onChange={(event) => patch({ url: event.target.value })}
              placeholder="https://updates.amarcode.amarjay.com/r/…"
              autoCapitalize="none"
              spellCheck={false}
            />
          </label>
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
              Paste the Remote URL printed by the daemon (short link or
              wss://…/rpc).
            </p>
          </>
        )}
      </fieldset>
      {saved?.mode === "ssh" && (
        <p className="text-xs text-muted-foreground">
          This installation has a saved SSH tunnel connection. Enter the
          daemon’s remote URL and token to replace it. For an interactive remote
          terminal, run <code>amarcoded ssh</code> in your terminal.
        </p>
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
