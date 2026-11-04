import { useEffect, useRef, useState } from "react";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import { LoaderCircle } from "lucide-react";
import {
  agentSignInRequiredAtom,
  daemonConnectionGenerationAtom,
} from "@/state/daemon-events";
import { daemonApi } from "@/api";
import { agentSetupResultsAtom, agentSetupTargetAtom } from "@/state/agents";
import type { AgentSetupAction, AgentSetupResult } from "@/generated/protocol";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { openExternalTarget } from "@/components/ai-elements/tauri-link-safety";
import { Button } from "@/components/ui/button";

export function AgentSetupDialog() {
  const [target, setTarget] = useAtom(agentSetupTargetAtom);
  const generation = useAtomValue(daemonConnectionGenerationAtom);
  const previousGeneration = useRef(generation);
  const [results, saveResults] = useAtom(agentSetupResultsAtom);
  const backgroundIds = Object.entries(results)
    .filter(
      ([id, result]) =>
        id !== target?.id &&
        (result.status === "checking" || result.status === "signing_in"),
    )
    .map(([id]) => id)
    .sort()
    .join("\n");
  useEffect(() => {
    if (!backgroundIds) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      await Promise.all(
        backgroundIds.split("\n").map(async (id) => {
          try {
            const result = await daemonApi.agentSetup(id, "status");
            if (!stopped)
              saveResults((previous) => ({ ...previous, [id]: result }));
          } catch {
            if (!stopped)
              saveResults((previous) => {
                const next = { ...previous };
                delete next[id];
                return next;
              });
          }
        }),
      );
      if (!stopped) timer = setTimeout(poll, 1000);
    };
    timer = setTimeout(poll, 1000);
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [backgroundIds, generation, saveResults]);
  useEffect(() => {
    if (previousGeneration.current === generation) return;
    previousGeneration.current = generation;
    saveResults({});
    setTarget(null);
  }, [generation, saveResults, setTarget]);
  return target ? (
    <Setup key={target.id} target={target} onClose={() => setTarget(null)} />
  ) : null;
}

function Setup({
  target,
  onClose,
}: {
  target: { id: string; name: string; onReady?: () => void };
  onClose: () => void;
}) {
  const [result, setResult] = useState<AgentSetupResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [requesting, setRequesting] = useState(false);
  const [methodId, setMethodId] = useState("");
  const saveResults = useSetAtom(agentSetupResultsAtom);
  const setAuthRequired = useSetAtom(agentSignInRequiredAtom);
  const alive = useRef(true);
  const requestSequence = useRef(0);
  const accept = (next: AgentSetupResult) => {
    setResult(next);
    if (next.status === "ready") {
      setAuthRequired((previous) => ({ ...previous, [target.id]: false }));
      target.onReady?.();
    }
    saveResults((previous) => ({ ...previous, [target.id]: next }));
    setMethodId((current) =>
      next.methods.some((m) => m.id === current && m.supported)
        ? current
        : (next.methods.find((m) => m.supported)?.id ?? ""),
    );
  };
  const request = async (action: AgentSetupAction) => {
    const sequence = ++requestSequence.current;
    setRequesting(true);
    setError(null);
    try {
      const next = await daemonApi.agentSetup(
        target.id,
        action,
        methodId || undefined,
      );
      if (alive.current && sequence === requestSequence.current) accept(next);
    } catch (cause) {
      if (alive.current && sequence === requestSequence.current)
        setError(String(cause));
    } finally {
      if (alive.current && sequence === requestSequence.current)
        setRequesting(false);
    }
  };
  useEffect(() => {
    alive.current = true;
    const timer = setTimeout(() => void request("check"), 0);
    return () => {
      clearTimeout(timer);
      alive.current = false;
    };
    // Each target mounts its own job observer.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target.id]);
  const busy = result?.status === "checking" || result?.status === "signing_in";
  useEffect(() => {
    if (!busy || error) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      const sequence = requestSequence.current;
      try {
        const next = await daemonApi.agentSetup(target.id, "status");
        if (stopped || !alive.current || sequence !== requestSequence.current)
          return;
        accept(next);
        if (next.status === "checking" || next.status === "signing_in")
          timer = setTimeout(poll, 700);
      } catch (cause) {
        if (!stopped && alive.current) setError(String(cause));
      }
    };
    timer = setTimeout(poll, 700);
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [busy, error, target.id]);

  const loginLinks =
    busy && result?.status === "signing_in"
      ? [
          ...new Set(
            `${result.message ?? ""} ${result.output ?? ""}`
              .split(String.fromCharCode(27))
              .join(" ")
              .match(/https?:\/\/[^\s<>"']+/g) ?? [],
          ),
        ]
          .slice(0, 5)
          .flatMap((value) => {
            try {
              const url = new URL(value);
              return [{ url: url.href, host: url.hostname }];
            } catch {
              return [];
            }
          })
      : [];
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            Sign in to {target.name.replace(/\s*\bACP\s*$/i, "")}
          </DialogTitle>
          <DialogDescription>
            This agent requires authentication. Use its supported login method.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3 text-sm" aria-live="polite">
          {(busy || requesting) && (
            <p className="flex items-center gap-2">
              <LoaderCircle className="size-4 animate-spin" />
              {result?.status === "signing_in"
                ? "Signing in… Follow the agent’s browser login or instructions below."
                : "Checking setup…"}
            </p>
          )}
          {result?.message && (
            <p className="whitespace-pre-wrap break-words">{result.message}</p>
          )}
          {result?.output && (
            <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-all rounded border p-2 text-xs">
              {result.output}
            </pre>
          )}
          {loginLinks.map(({ url, host }) => (
            <Button
              key={url}
              variant="outline"
              className="w-full justify-start truncate"
              onClick={() =>
                void openExternalTarget(url).catch((cause: unknown) =>
                  setError(String(cause)),
                )
              }
              title={url}
            >
              Open sign-in link: {host}
            </Button>
          ))}
          {error && (
            <p role="alert" className="text-destructive">
              {error}
            </p>
          )}
          {!busy && result && result.status !== "ready" && (
            <>
              {result.methods.some((m) => m.supported) ? (
                <div className="space-y-2">
                  <label
                    htmlFor="agent-auth-method"
                    className="block text-xs text-muted-foreground"
                  >
                    Sign-in method
                  </label>
                  <select
                    id="agent-auth-method"
                    className="w-full rounded-md border bg-background p-2"
                    value={methodId}
                    onChange={(e) => setMethodId(e.target.value)}
                  >
                    {result.methods
                      .filter((m) => m.supported)
                      .map((m) => (
                        <option key={m.id} value={m.id}>
                          {m.name}
                        </option>
                      ))}
                  </select>
                  {result.methods.find((m) => m.id === methodId)
                    ?.description && (
                    <p className="text-muted-foreground">
                      {
                        result.methods.find((m) => m.id === methodId)
                          ?.description
                      }
                    </p>
                  )}
                </div>
              ) : (
                <p className="text-muted-foreground">
                  This agent does not advertise an in-app sign-in method.
                  Complete its CLI login or provider API-key setup on the
                  machine running the daemon, then check again.
                </p>
              )}
              {result.methods
                .filter((m) => !m.supported)
                .map((m) => (
                  <p key={m.id} className="text-xs text-muted-foreground">
                    {m.name}: {m.description ?? "Requires external setup."}
                  </p>
                ))}
            </>
          )}
          <p className="text-xs text-muted-foreground">
            Credentials are managed by the agent on the daemon’s machine.
            Closing this dialog keeps setup running.
          </p>
        </div>
        <div className="flex justify-end gap-2">
          {busy ? (
            <Button
              variant="outline"
              disabled={requesting}
              onClick={() => void request("cancel")}
            >
              Cancel setup
            </Button>
          ) : (
            <>
              <Button
                variant="outline"
                disabled={requesting}
                onClick={() => void request("check")}
              >
                Check again
              </Button>
              {methodId && result?.status !== "ready" && (
                <Button
                  disabled={requesting}
                  onClick={() => void request("sign_in")}
                >
                  Sign in
                </Button>
              )}
            </>
          )}
          <Button variant="ghost" onClick={onClose}>
            Close
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
