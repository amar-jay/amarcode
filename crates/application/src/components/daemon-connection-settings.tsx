import { useEffect, useState } from "react";
import { daemonApi } from "@/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

/** Also shown before bootstrap, so a local service is never required to connect. */
export function DaemonConnectionSettings() {
  const [remote, setRemote] = useState(false);
  const [url, setUrl] = useState("");
  const [token, setToken] = useState("");
  const [hasToken, setHasToken] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    void daemonApi
      .getConnection()
      .then((value) => {
        setRemote(value.remote);
        setUrl(value.url);
        setHasToken(value.hasToken);
        setLoaded(true);
      })
      .catch((cause: unknown) => {
        setError(String(cause));
        setLoaded(true); // Allow switching to local to recover corrupt preferences.
      });
  }, []);
  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const value = await daemonApi.setConnection(
        remote ? url : "",
        remote ? token : "",
      );
      // Only the endpoint is exposed to JS; the credential remains in native storage.
      localStorage.setItem("amarcode-connection-scope", value.url);
      window.location.reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      setBusy(false);
    }
  };
  return (
    <section className="space-y-3 text-left">
      <h2 className="text-base font-medium">Daemon connection</h2>
      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={remote}
          onChange={(event) => setRemote(event.target.checked)}
          disabled={!loaded || busy}
        />
        Connect to a remote daemon
      </label>
      {remote && (
        <>
          <label className="block space-y-1 text-sm">
            <span>WebSocket URL</span>
            <Input
              value={url}
              onChange={(event) => setUrl(event.target.value)}
              placeholder="wss://daemon.example.com/rpc"
              autoCapitalize="none"
              spellCheck={false}
              disabled={busy}
            />
          </label>
          <label className="block space-y-1 text-sm">
            <span>Access token</span>
            <Input
              type="password"
              value={token}
              onChange={(event) => setToken(event.target.value)}
              placeholder={
                hasToken
                  ? "Leave blank to keep the saved token for this URL"
                  : "Token from the remote host"
              }
              autoComplete="off"
              disabled={busy}
            />
          </label>
          <p className="text-xs text-muted-foreground">
            Use the tunnel’s hostname with /rpc. Project folders, Git commands,
            and agents run on the remote host.
          </p>
        </>
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
      <Button
        onClick={() => void save()}
        disabled={!loaded || busy || (remote && !url.trim())}
      >
        {busy ? "Checking connection…" : "Apply and reload"}
      </Button>
    </section>
  );
}
