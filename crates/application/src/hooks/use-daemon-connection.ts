import { useCallback, useEffect, useState } from "react";
// import { api } from "@/api";
import type { AgentDefinition, SessionSummary } from "@/types";
import { notify } from "@/lib/notify";

export type DaemonStatus = "connecting" | "connected" | "error";

export function useDaemonConnection(
  onReady: (data: {
    agents: AgentDefinition[];
    sessions: SessionSummary[];
  }) => void,
) {
  const [status, setStatus] = useState<DaemonStatus>("connecting");
  const [error, setError] = useState("");

  const connect = useCallback(async () => {
    setStatus("connecting");
    setError("");
    try {
      // await api.health();
      const [agents, sessions] = await Promise.all([
        // api.agents(),
        // api.sessions(),
      ]);
      onReady({ agents, sessions });
      setStatus("connected");
    } catch (reason) {
      setError(String(reason));
      setStatus("error");
      notify("Could not connect to the AMARCODE daemon", "error");
    }
  }, [onReady]);

  useEffect(() => {
    void connect();
  }, [connect]);

  return { connect, error, status };
}
