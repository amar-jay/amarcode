import { useEffect, useState } from "react";
import { daemonApi } from "@/api";
import type { AgentDefinition } from "@/types";

let cachedAgents: AgentDefinition[] | undefined;
let pendingAgents: Promise<AgentDefinition[]> | undefined;

function loadAgents(): Promise<AgentDefinition[]> {
  if (cachedAgents) return Promise.resolve(cachedAgents);
  if (!pendingAgents) {
    pendingAgents = daemonApi.listAgents()
      .then((agents) => (cachedAgents = agents))
      .finally(() => { pendingAgents = undefined; });
  }
  return pendingAgents;
}

/** Shares the initial catalog request between Strict Mode effect replays. */
export function useAgentCatalog(): AgentDefinition[] {
  const [agents, setAgents] = useState<AgentDefinition[]>(cachedAgents ?? []);

  useEffect(() => {
    let mounted = true;
    void loadAgents().then((next) => { if (mounted) setAgents(next); });
    return () => { mounted = false; };
  }, []);

  return agents;
}
