import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DaemonConnectionInfo } from "@/api";
vi.mock("@/api", () => ({ daemonApi: { setConnection: vi.fn() } }));
import { daemonApi } from "@/api";
import {
  applyConnection, canReuseToken, connectionError, connectionForm,
  connectionIdentity, emptyConnectionForm,
} from "./daemon-connection-form";
const saved: DaemonConnectionInfo = {
  mode: "websocket", remote: true, url: "wss://lab.example:43821/rpc",
  hasToken: true, scope: "wss://lab.example:43821/rpc", ssh: null,
};
beforeEach(() => vi.resetAllMocks());
describe("daemon connection settings", () => {
  it("loads a remote URL without returning its token", () => {
    expect(connectionForm(saved)).toEqual({ mode: "websocket", url: saved.url, token: "" });
    expect(connectionIdentity(saved)).toBe("lab.example:43821");
    expect(connectionIdentity({ ...saved, mode: "local", remote: false })).toBeNull();
  });
  it("reuses a saved token only for the original remote target", () => {
    const form = connectionForm(saved);
    expect(canReuseToken(form, saved)).toBe(true);
    expect(canReuseToken({ ...form, url: "wss://other.example/rpc" }, saved)).toBe(false);
    expect(canReuseToken(form, { ...saved, hasToken: false })).toBe(false);
    expect(canReuseToken(form, null)).toBe(false);
  });
  it("requires a new remote URL and token when replacing a legacy SSH profile", () => {
    const legacy: DaemonConnectionInfo = {
      ...saved, mode: "ssh", url: "ws://127.0.0.1:12345/rpc", scope: "ssh:profile",
      ssh: { host: "dev-box", user: "dev", port: 2222, identityFile: null, daemonPort: 43721 },
    };
    const form = connectionForm(legacy);
    expect(form).toEqual({ mode: "websocket", url: "", token: "" });
    expect(canReuseToken({ ...form, url: saved.url }, legacy)).toBe(false);
    expect(connectionIdentity(legacy)).toBe("dev@dev-box:2222");
  });
  it("verifies through the remote URL API before returning", async () => {
    vi.mocked(daemonApi.setConnection).mockResolvedValue(saved);
    expect(await applyConnection({ mode: "websocket", url: ` ${saved.url} `, token: " token " })).toBe(saved);
    expect(daemonApi.setConnection).toHaveBeenCalledWith(saved.url, "token");
  });
  it("keeps the current connection on verification failure", async () => {
    vi.mocked(daemonApi.setConnection).mockRejectedValue(new Error("Unauthorized"));
    await expect(applyConnection(connectionForm(saved))).rejects.toThrow("Unauthorized");
  });
  it("requires a remote URL before invoking native IPC", async () => {
    await expect(applyConnection({ ...emptyConnectionForm, mode: "websocket" })).rejects.toThrow("URL");
    expect(daemonApi.setConnection).not.toHaveBeenCalled();
  });
  it("clears remote credentials when choosing local mode", async () => {
    await applyConnection({ ...connectionForm(saved), mode: "local", token: "must not persist" });
    expect(daemonApi.setConnection).toHaveBeenCalledWith("", "");
  });
  it("explains token rejection and preserves other errors", () => {
    expect(connectionError("HTTP error: 401 Unauthorized")).toContain("access token");
    expect(connectionError(new Error("Protocol mismatch"))).toBe("Protocol mismatch");
  });
});
