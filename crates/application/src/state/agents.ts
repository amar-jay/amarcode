import { getPreferredAgent } from "@/lib/agent";
import { atom } from "jotai";
import { daemonApi } from "@/api";
import type { AgentInfo } from "@/types";
import { defaultAgentIdAtom } from "./preferences";

/** Full agent catalog from the daemon (shared, loaded once). */
export const agentsAtom = atom<AgentInfo[]>([]);

const agentsLoadStateAtom = atom<"idle" | "loading" | "ready" | "error">(
  "idle",
);

/** Load agents if not already loaded / in-flight. Retries after a prior error. */
export const loadAgentsAtom = atom(null, async (get, set) => {
  const status = get(agentsLoadStateAtom);
  if (status === "loading" || status === "ready") {
    return get(agentsAtom);
  }
  set(agentsLoadStateAtom, "loading");
  try {
    const agents = await daemonApi.listAgents();
    set(agentsAtom, agents);
    set(agentsLoadStateAtom, "ready");
    void set(ensureBuiltinAgentAtom);
    return agents;
  } catch (error) {
    set(agentsLoadStateAtom, "error");
    throw error;
  }
});

/** Force-refresh the agent catalog from the daemon. */
export const reloadAgentsAtom = atom(null, async (_get, set) => {
  set(agentsLoadStateAtom, "loading");
  try {
    const agents = await daemonApi.listAgents();
    set(agentsAtom, agents);
    set(agentsLoadStateAtom, "ready");
    void set(ensureBuiltinAgentAtom);
    return agents;
  } catch (error) {
    set(agentsLoadStateAtom, "error");
    throw error;
  }
});

type InstallResult = Awaited<ReturnType<typeof daemonApi.installAgent>>;
const installRequestsAtom = atom<Record<string, Promise<InstallResult>>>({});
export const installingAgentIdsAtom = atom((get) =>
  Object.keys(get(installRequestsAtom)),
);

/** Share background/manual installations and merge completion into the catalog. */
export const installAgentAtom = atom(null, (get, set, agentId: string) => {
  const existing = get(installRequestsAtom)[agentId];
  if (existing) return existing;
  const request = Promise.resolve()
    .then(() => daemonApi.installAgent(agentId))
    .then((result) => {
      const agents = get(agentsAtom);
      const next = agents.some((candidate) => candidate.id === result.agent.id)
        ? agents.map((candidate) =>
            candidate.id === result.agent.id ? result.agent : candidate,
          )
        : [...agents, result.agent];
      set(agentsAtom, next);
      return result;
    })
    .finally(() => {
      const pending = { ...get(installRequestsAtom) };
      delete pending[agentId];
      set(installRequestsAtom, pending);
    });
  set(installRequestsAtom, { ...get(installRequestsAtom), [agentId]: request });
  return request;
});

/** First-run installation is best-effort and never holds up startup/catalogs. */
const ensureBuiltinAgentAtom = atom(null, async (get, set) => {
  const builtin = get(agentsAtom).find(
    (agent) => agent.id === "amarcode-acp" && agent.is_builtin,
  );
  if (!builtin || builtin.available) return;
  try {
    await set(installAgentAtom, builtin.id);
  } catch (error) {
    // Keep the catalog usable offline and leave the Install button as a retry.
    console.info(
      "Automatic Amarcode ACP installation failed; manual retry is available:",
      error,
    );
  }
});

/**
 * Agent currently selected in the home/chat composer.
 * Falls back to the default agent id when unset.
 */
export const selectedAgentIdAtom = atom<string | null>(null);

/** Resolved agent definition for the current selection. */
export const selectedAgentAtom = atom((get) => {
  const agents = get(agentsAtom);
  if (!agents.length) return undefined;
  const preferred = get(selectedAgentIdAtom) ?? get(defaultAgentIdAtom);
  return getPreferredAgent(agents, preferred);
});

/** Set selection by agent id (no-op if unknown). */
export const selectAgentByIdAtom = atom(null, (get, set, agentId: string) => {
  const agent = get(agentsAtom).find(
    (candidate) => candidate.id === agentId && candidate.available,
  );
  if (agent) set(selectedAgentIdAtom, agent.id);
});

/** Set selection from a full agent object. */
export const selectAgentAtom = atom(null, (_get, set, agent: AgentInfo) => {
  if (agent.available) set(selectedAgentIdAtom, agent.id);
});

/** Session-scoped readiness is deliberately separate from executable availability. */
export const agentSetupResultsAtom = atom<
  Record<string, import("@/generated/protocol").AgentSetupResult>
>({});
export const agentSetupTargetAtom = atom<{
  id: string;
  name: string;
  onReady?: () => void;
} | null>(null);
