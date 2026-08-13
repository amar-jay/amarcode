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
    return agents;
  } catch (error) {
    set(agentsLoadStateAtom, "error");
    throw error;
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
  return (
    agents.find((agent) => agent.id === preferred && agent.available) ??
    agents.find((agent) => agent.available)
  );
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
