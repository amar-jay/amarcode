import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DaemonConnectionInfo } from "@/api";
vi.mock("@/api", () => ({
  daemonApi: { setConnection: vi.fn(), setSshConnection: vi.fn() },
}));
import { daemonApi } from "@/api";
import {
  applyConnection,
  canReuseToken,
  connectionError,
  connectionForm,
  emptyConnectionForm,
  sshConfig,
} from "./daemon-connection-form";

const saved: DaemonConnectionInfo = {
  mode: "ssh",
  remote: true,
  url: "",
  hasToken: true,
  scope: "ssh:profile",
  ssh: {
    host: "dev-box",
    user: null,
    port: null,
    identityFile: null,
    daemonPort: 43721,
  },
};
beforeEach(() => {
  vi.clearAllMocks();
});

describe("SSH connection settings", () => {
  it("loads a saved profile without putting its token into the form", () => {
    const form = connectionForm(saved);
    expect(form).toMatchObject({
      mode: "ssh",
      host: "dev-box",
      port: "",
      daemonPort: "43721",
      token: "",
    });
    expect(sshConfig(form)).toEqual(saved.ssh);
    expect(canReuseToken(form, saved)).toBe(true);
  });

  it.each(["host", "user", "port", "identityFile", "daemonPort"] as const)(
    "requires a token when %s changes",
    (field) => {
      const form = connectionForm(saved);
      const values = {
        host: "another-host",
        user: "dev",
        port: "2222",
        identityFile: "/keys/id_ed25519",
        daemonPort: "43722",
      };
      form[field] = values[field];
      expect(canReuseToken(form, saved)).toBe(false);
    },
  );

  it.each(["0", "65536", "1.5", "2e2", "abc", "-22"])(
    "rejects invalid SSH ports before invoking native IPC: %s",
    async (port) => {
      await expect(
        applyConnection({ ...connectionForm(saved), port }),
      ).rejects.toThrow("SSH port");
      expect(daemonApi.setSshConnection).not.toHaveBeenCalled();
    },
  );

  it("requires a host and valid daemon port", () => {
    expect(() => sshConfig({ ...connectionForm(saved), host: " " })).toThrow(
      "host",
    );
    expect(() =>
      sshConfig({ ...connectionForm(saved), daemonPort: "" }),
    ).toThrow("Daemon port");
  });

  it("normalizes SSH fields and sends the token only to the native SSH command", async () => {
    vi.mocked(daemonApi.setSshConnection).mockResolvedValue(saved);
    const result = await applyConnection({
      ...connectionForm(saved),
      host: " dev-box ",
      user: " developer ",
      port: "2222",
      identityFile: " /keys/private key ",
      token: " token ",
    });
    expect(result).toBe(saved);
    expect(daemonApi.setSshConnection).toHaveBeenCalledWith(
      {
        host: "dev-box",
        user: "developer",
        port: 2222,
        identityFile: "/keys/private key",
        daemonPort: 43721,
      },
      "token",
    );
    expect(daemonApi.setConnection).not.toHaveBeenCalled();
  });

  it("propagates verification failure so the caller keeps the current connection", async () => {
    vi.mocked(daemonApi.setSshConnection).mockRejectedValue(
      new Error("Host key verification failed"),
    );
    await expect(applyConnection(connectionForm(saved))).rejects.toThrow(
      "Host key",
    );
    expect(daemonApi.setConnection).not.toHaveBeenCalled();
  });

  it("clears remote credentials when choosing local mode", async () => {
    await applyConnection({
      ...connectionForm(saved),
      mode: "local",
      token: "must not persist",
    });
    expect(daemonApi.setConnection).toHaveBeenCalledWith("", "");
    expect(daemonApi.setSshConnection).not.toHaveBeenCalled();
  });

  it("preserves WebSocket support and reuses a token only for its original URL", async () => {
    const previous = {
      ...saved,
      mode: "websocket" as const,
      url: "wss://example.com/rpc",
      ssh: null,
    };
    const form = connectionForm(previous);
    expect(canReuseToken(form, previous)).toBe(true);
    expect(
      canReuseToken({ ...form, url: "wss://other.example/rpc" }, previous),
    ).toBe(false);
    expect(canReuseToken(form, saved)).toBe(false);
    await applyConnection(form);
    expect(daemonApi.setConnection).toHaveBeenCalledWith(previous.url, "");
    await expect(
      applyConnection({ ...emptyConnectionForm, mode: "websocket" }),
    ).rejects.toThrow("URL");
  });

  it.each([
    ["Host key verification failed", "fingerprint"],
    ["REMOTE HOST IDENTIFICATION HAS CHANGED", "fingerprint"],
    ["Cannot start OpenSSH: file not found", "Install the SSH client"],
    ["Permission denied (publickey)", "SSH authentication failed"],
    ["HTTP error: 401 Unauthorized", "access token"],
    ["Remote daemon must use protocol 13", "protocol 13"],
  ])("explains %s", (error, expected) => {
    expect(connectionError(error)).toContain(expected);
  });
});
