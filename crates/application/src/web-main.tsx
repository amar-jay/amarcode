import { StrictMode, Suspense, lazy, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  connectBrowserDaemon,
  disconnectBrowserDaemon,
} from "@/lib/browser-daemon";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import "./index.css";

const WebApp = lazy(() => import("./web-app"));
document.documentElement.dataset.style = "ember";
function RemoteWeb() {
  const [connected, setConnected] = useState(false);
  const [url, setUrl] = useState(
    () => localStorage.getItem("amarcode-web-url") ?? "",
  );
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (connected)
    return (
      <Suspense
        fallback={
          <div role="status" className="m-auto p-8">
            Opening workspace…
          </div>
        }
      >
        <WebApp />
      </Suspense>
    );
  return (
    <main className="flex min-h-dvh w-full items-center justify-center bg-background p-6 text-foreground">
      <form
        className="w-full max-w-sm space-y-5"
        onSubmit={async (event) => {
          event.preventDefault();
          if (busy) return;
          setBusy(true);
          setError(null);
          try {
            await connectBrowserDaemon(url, token);
            setToken("");
            setConnected(true);
          } catch (cause) {
            setError(
              cause instanceof Error ? cause.message : "Could not connect.",
            );
          } finally {
            setBusy(false);
          }
        }}
      >
        <div className="space-y-2">
          <img src="/acp-mark.svg" alt="" className="size-8" />
          <h1 className="text-xl font-medium">Amarcode remote</h1>
          <p className="text-sm text-muted-foreground">
            Connect to the daemon running on your remote machine.
          </p>
        </div>
        <label className="block space-y-2 text-sm">
          <span>Daemon URL</span>
          <Input
            type="url"
            autoComplete="url"
            value={url}
            onChange={(event) => setUrl(event.target.value)}
            placeholder="https://updates.amarcode.amarjay.com/r/…"
            required
            disabled={busy}
          />
        </label>
        <label className="block space-y-2 text-sm">
          <span>Access token</span>
          <Input
            type="password"
            autoComplete="off"
            value={token}
            onChange={(event) => setToken(event.target.value)}
            required
            disabled={busy}
          />
        </label>
        <p className="text-xs text-muted-foreground">
          Your token stays in this tab’s memory. Reloading or disconnecting
          requires signing in again.
        </p>
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        <Button type="submit" className="w-full" disabled={busy}>
          {busy ? "Connecting…" : "Connect"}
        </Button>
      </form>
    </main>
  );
}
window.addEventListener("pagehide", disconnectBrowserDaemon);
window.addEventListener("pageshow", (event) => {
  if (event.persisted) window.location.reload();
});
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <RemoteWeb />
  </StrictMode>,
);
