import { useAtomValue, useSetAtom } from "jotai";
import { agentSetupResultsAtom, agentSetupTargetAtom } from "@/state/agents";

export function AgentSetupButton({
  agent,
  onOpen,
}: {
  agent: { id: string; name: string };
  onOpen: () => void;
}) {
  const result = useAtomValue(agentSetupResultsAtom)[agent.id];
  const openSetup = useSetAtom(agentSetupTargetAtom);
  const label =
    result?.status === "ready"
      ? "Ready"
      : result?.status === "auth_required" &&
          result.methods.some((m) => m.supported)
        ? "Sign in"
        : result?.status === "signing_in"
          ? "Signing in…"
          : result?.status === "checking"
            ? "Checking…"
            : "Check setup";
  return (
    <button
      type="button"
      className="ml-auto shrink-0 rounded px-2 py-1 text-[11px] text-primary hover:bg-foreground/10 focus-visible:outline-2 focus-visible:outline-ring"
      aria-label={`${label}: ${agent.name}`}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") event.stopPropagation();
      }}
      onClick={(event) => {
        event.preventDefault();
        event.stopPropagation();
        onOpen();
        openSetup({ id: agent.id, name: agent.name });
      }}
    >
      {label}
    </button>
  );
}
