import { useCallback, useMemo, useState } from "react";
// import { api } from "@/api";
import type { AgentDefinition } from "@/types";
import { notify } from "@/lib/notify";

export type AgentForm = { name: string; command: string; arguments: string };
const emptyAgentForm: AgentForm = { name: "", command: "", arguments: "" };

export function useAgentCatalog() {
  const [agents, setAgents] = useState<AgentDefinition[]>([]);
  const [selectedAgentId, setSelectedAgentId] = useState("");
  const [agentForm, setAgentForm] = useState<AgentForm>(emptyAgentForm);
  const [showAgentForm, setShowAgentForm] = useState(false);
  const [error, setError] = useState("");

  const selectedAgent = useMemo(
    () => agents.find((agent) => agent.id === selectedAgentId),
    [agents, selectedAgentId],
  );

  const loadAgents = useCallback((loadedAgents: AgentDefinition[]) => {
    setAgents(loadedAgents);
    setSelectedAgentId((current) =>
      loadedAgents.some((agent) => agent.id === current)
        ? current
        : (loadedAgents[0]?.id ?? ""),
    );
  }, []);

  async function addAgent(event: React.FormEvent) {
    event.preventDefault();
    const created: AgentDefinition = {
      id: crypto.randomUUID(),
      name: agentForm.name,
      command: agentForm.command,
      arguments: agentForm.arguments.split(" ").filter(Boolean),
      environment: [],
      isPreset: false,
    };
    try {
      // await api.saveAgent(created);
      setAgents((current) => [...current, created]);
      setSelectedAgentId(created.id);
      setShowAgentForm(false);
      setAgentForm(emptyAgentForm);
      setError("");
      notify(`${created.name} was added`, "success");
    } catch (reason) {
      setError(String(reason));
      notify("Could not add that agent", "error");
    }
  }

  return {
    addAgent,
    agentForm,
    agents,
    error,
    loadAgents,
    selectedAgent,
    selectedAgentId,
    setAgentForm,
    setSelectedAgentId,
    setShowAgentForm,
    showAgentForm,
  };
}
