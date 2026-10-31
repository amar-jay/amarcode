import { createStore } from "jotai";
import { beforeEach, describe, expect, it, vi } from "vitest";
vi.mock("@/api", () => ({
  daemonApi: { listAgents: vi.fn(), installAgent: vi.fn() },
}));
vi.mock("./preferences", async () => ({
  defaultAgentIdAtom: (await import("jotai")).atom(""),
}));
import { daemonApi } from "@/api";
import type { AgentInfo } from "@/types";
import {
  agentsAtom,
  installingAgentIdsAtom,
  installAgentAtom,
  loadAgentsAtom,
  reloadAgentsAtom,
} from "./agents";
const builtin: AgentInfo = {
  id: "amarcode-acp",
  name: "Amarcode ACP",
  is_builtin: true,
  available: false,
  icon: null,
  command: "",
  arguments: [],
  environment: [],
  created_at: "",
  updated_at: "",
  resolved_command: null,
  unavailable_reason: "Not downloaded",
};
const installed: AgentInfo = {
  ...builtin,
  available: true,
  command: "/managed/amarcode-acp",
  resolved_command: "/managed/amarcode-acp",
  unavailable_reason: null,
};
const result = {
  agent: installed,
  runtime_status: "ready" as const,
  runtime_message: null,
};
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(daemonApi.listAgents).mockResolvedValue([builtin]);
  vi.mocked(daemonApi.installAgent).mockResolvedValue(result);
});
describe("default built-in agent installation", () => {
  it("loads the catalog without waiting for download and shares a manual install request", async () => {
    let finish!: (value: typeof result) => void;
    vi.mocked(daemonApi.installAgent).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const store = createStore();
    expect(await store.set(loadAgentsAtom)).toEqual([builtin]);
    expect(store.get(agentsAtom)).toEqual([builtin]);
    expect(store.get(installingAgentIdsAtom)).toEqual([builtin.id]);
    const manual = store.set(installAgentAtom, builtin.id);
    expect(daemonApi.installAgent).toHaveBeenCalledTimes(1);
    finish(result);
    await manual;
    expect(store.get(agentsAtom)).toEqual([installed]);
    expect(store.get(installingAgentIdsAtom)).toEqual([]);
  });
  it("automatically marks ACP installed and selectable without provider settings", async () => {
    const store = createStore();
    await store.set(loadAgentsAtom);
    await vi.waitFor(() => expect(store.get(agentsAtom)).toEqual([installed]));
    expect(daemonApi.installAgent).toHaveBeenCalledWith("amarcode-acp");
  });
  it("does not download an existing installation again", async () => {
    vi.mocked(daemonApi.listAgents).mockResolvedValue([installed]);
    const store = createStore();
    await store.set(loadAgentsAtom);
    await store.set(reloadAgentsAtom);
    expect(daemonApi.installAgent).not.toHaveBeenCalled();
  });
  it("keeps startup usable offline and allows a manual retry", async () => {
    vi.spyOn(console, "info").mockImplementation(() => undefined);
    vi.mocked(daemonApi.installAgent).mockRejectedValueOnce(
      new Error("Offline"),
    );
    const store = createStore();
    expect(await store.set(loadAgentsAtom)).toEqual([builtin]);
    await vi.waitFor(() =>
      expect(store.get(installingAgentIdsAtom)).toEqual([]),
    );
    expect(store.get(agentsAtom)).toEqual([builtin]);
    await store.set(installAgentAtom, builtin.id);
    expect(daemonApi.installAgent).toHaveBeenCalledTimes(2);
    expect(store.get(agentsAtom)).toEqual([installed]);
    vi.mocked(console.info).mockRestore();
  });
  it("retries a failed automatic download on an explicit catalog refresh", async () => {
    vi.spyOn(console, "info").mockImplementation(() => undefined);
    vi.mocked(daemonApi.installAgent).mockRejectedValueOnce(
      new Error("Offline"),
    );
    const store = createStore();
    await store.set(loadAgentsAtom);
    await vi.waitFor(() =>
      expect(store.get(installingAgentIdsAtom)).toEqual([]),
    );
    await store.set(reloadAgentsAtom);
    await vi.waitFor(() => expect(store.get(agentsAtom)).toEqual([installed]));
    expect(daemonApi.installAgent).toHaveBeenCalledTimes(2);
    vi.mocked(console.info).mockRestore();
  });
  it("does not automatically install other catalog agents", async () => {
    vi.mocked(daemonApi.listAgents).mockResolvedValue([
      { ...builtin, id: "external-agent", is_builtin: false },
    ]);
    await createStore().set(loadAgentsAtom);
    expect(daemonApi.installAgent).not.toHaveBeenCalled();
  });
  it("does not share in-flight installations across independent application stores", async () => {
    vi.mocked(daemonApi.installAgent).mockReturnValue(
      new Promise(() => undefined),
    );
    const first = createStore();
    const second = createStore();
    await first.set(loadAgentsAtom);
    await second.set(loadAgentsAtom);
    expect(daemonApi.installAgent).toHaveBeenCalledTimes(2);
  });
});
