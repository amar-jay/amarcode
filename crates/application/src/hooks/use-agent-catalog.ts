/**
 * @deprecated Prefer `agentsAtom` / `loadAgentsAtom` from `@/state`.
 */
import { useSetAtom, useAtomValue } from "jotai";
import { useEffect } from "react";
import { agentsAtom, loadAgentsAtom } from "@/state/agents";

export function useAgentCatalog() {
  const agents = useAtomValue(agentsAtom);
  const loadAgents = useSetAtom(loadAgentsAtom);

  useEffect(() => {
    void loadAgents().catch((error: unknown) => {
      console.error("Failed to load agents:", error);
    });
  }, [loadAgents]);

  return agents;
}
