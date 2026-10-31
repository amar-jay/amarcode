import { beforeEach, describe, expect, it, vi } from "vitest";
vi.mock("@/api", () => ({
  appUpdateApi: { check: vi.fn(), install: vi.fn() },
  daemonApi: { checkUpdate: vi.fn(), getConnection: vi.fn(), update: vi.fn() },
}));
import { appUpdateApi, daemonApi, type DaemonConnectionInfo } from "@/api";
import type { Health } from "@/types";
import {
  applyUpdatePlan,
  checkUpdatePlan,
  hasUpdates,
  type UpdatePlan,
} from "./updates";
const connection: DaemonConnectionInfo = {
  remote: true,
  mode: "websocket",
  url: "wss://remote.example/rpc",
  scope: "wss://remote.example/rpc",
  hasToken: true,
  ssh: null,
};
const app = {
  status: "available" as const,
  version: "2.0.0",
  currentVersion: "1.0.0",
  notes: "New release",
};
const daemon = {
  status: "available" as const,
  version: "0.9.0",
  currentVersion: "0.8.3",
};
const health: Health = {
  status: "ok",
  version: daemon.version,
  protocol_version: 13,
  remote_auth_required: true,
  addr: "127.0.0.1:43721",
};
const plan = (): UpdatePlan => ({ app, daemon, connection, issues: [] });
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(appUpdateApi.check).mockResolvedValue(app);
  vi.mocked(daemonApi.checkUpdate).mockResolvedValue(daemon);
  vi.mocked(daemonApi.getConnection).mockResolvedValue(connection);
  vi.mocked(daemonApi.update).mockResolvedValue(health);
  vi.mocked(appUpdateApi.install).mockResolvedValue(undefined);
});
describe("combined update checks", () => {
  it("collects both releases into one plan", async () => {
    expect(await checkUpdatePlan()).toEqual(plan());
  });
  it("reports up to date only when both checks succeed", async () => {
    vi.mocked(appUpdateApi.check).mockResolvedValue({
      status: "upToDate",
      currentVersion: "1",
    });
    vi.mocked(daemonApi.checkUpdate).mockResolvedValue({
      status: "upToDate",
      currentVersion: "1",
    });
    const result = await checkUpdatePlan();
    expect(hasUpdates(result)).toBe(false);
    expect(result.issues).toEqual([]);
  });
  it("keeps an application update available when the daemon cannot update", async () => {
    vi.mocked(daemonApi.checkUpdate).mockResolvedValue({
      status: "unavailable",
      reason: "Custom service",
    });
    const result = await checkUpdatePlan();
    expect(result.app).toEqual(app);
    expect(result.daemon).toBeNull();
    expect(result.issues).toEqual(["Daemon: Custom service"]);
  });
  it("retains check errors instead of saying everything is up to date", async () => {
    vi.mocked(appUpdateApi.check).mockRejectedValue(new Error("Offline"));
    vi.mocked(daemonApi.checkUpdate).mockRejectedValue("Unreachable");
    const result = await checkUpdatePlan();
    expect(hasUpdates(result)).toBe(false);
    expect(result.issues).toHaveLength(2);
    expect(result.issues.join(" ")).toContain("Offline");
    expect(result.issues.join(" ")).toContain("Unreachable");
  });
  it("does not offer a daemon update without identifying its host", async () => {
    vi.mocked(daemonApi.getConnection).mockRejectedValue(
      new Error("Settings unavailable"),
    );
    const result = await checkUpdatePlan();
    expect(result.daemon).toBeNull();
    expect(result.app).toEqual(app);
    expect(result.issues[0]).toContain("identify");
  });
});
describe("combined update installation", () => {
  it("updates and reconnects the daemon before starting the app installer", async () => {
    const order: string[] = [];
    vi.mocked(daemonApi.update).mockImplementation(async (_version, status) => {
      order.push("daemon");
      status({ status: "ready", version: daemon.version });
      return health;
    });
    vi.mocked(appUpdateApi.install).mockImplementation(async (status) => {
      order.push("application");
      status({ status: "restarting" });
    });
    const progress = vi.fn();
    await applyUpdatePlan(
      plan(),
      progress,
      () => {
        order.push("daemon complete");
      },
      async () => {
        order.push("reconnect");
      },
    );
    expect(order).toEqual([
      "daemon",
      "daemon complete",
      "reconnect",
      "application",
    ]);
    expect(progress).toHaveBeenCalledWith({
      component: "daemon",
      status: { status: "ready", version: daemon.version },
    });
    expect(progress).toHaveBeenCalledWith({
      component: "application",
      status: { status: "restarting" },
    });
    expect(progress).toHaveBeenLastCalledWith({ component: "complete" });
  });
  it("stops before app restart if the daemon update fails", async () => {
    vi.mocked(daemonApi.update).mockRejectedValue(new Error("Rolled back"));
    const complete = vi.fn();
    await expect(
      applyUpdatePlan(plan(), vi.fn(), complete, vi.fn()),
    ).rejects.toThrow("Rolled back");
    expect(complete).not.toHaveBeenCalled();
    expect(appUpdateApi.install).not.toHaveBeenCalled();
  });
  it("retains daemon success when app installation fails so retry skips it", async () => {
    let remaining = plan();
    vi.mocked(appUpdateApi.install).mockRejectedValueOnce(
      new Error("Download failed"),
    );
    await expect(
      applyUpdatePlan(
        remaining,
        vi.fn(),
        () => {
          remaining = { ...remaining, daemon: null };
        },
        vi.fn(),
      ),
    ).rejects.toThrow("Download failed");
    await applyUpdatePlan(remaining, vi.fn(), vi.fn(), vi.fn());
    expect(daemonApi.update).toHaveBeenCalledTimes(1);
    expect(appUpdateApi.install).toHaveBeenCalledTimes(2);
  });
  it("refuses to update another host after the selection changes", async () => {
    vi.mocked(daemonApi.getConnection).mockResolvedValue({
      ...connection,
      mode: "local",
      scope: "",
      remote: false,
    });
    await expect(
      applyUpdatePlan(plan(), vi.fn(), vi.fn(), vi.fn()),
    ).rejects.toThrow("connection changed");
    expect(daemonApi.update).not.toHaveBeenCalled();
    expect(appUpdateApi.install).not.toHaveBeenCalled();
  });
  it("handles a daemon-only release without restarting the application", async () => {
    const progress = vi.fn();
    await applyUpdatePlan({ ...plan(), app: null }, progress, vi.fn(), vi.fn());
    expect(daemonApi.update).toHaveBeenCalledWith(
      daemon.version,
      expect.any(Function),
    );
    expect(appUpdateApi.install).not.toHaveBeenCalled();
    expect(progress).toHaveBeenLastCalledWith({ component: "complete" });
  });
  it("handles an app-only release even when the daemon is unavailable", async () => {
    await applyUpdatePlan(
      { ...plan(), daemon: null, connection: null },
      vi.fn(),
      vi.fn(),
      vi.fn(),
    );
    expect(daemonApi.update).not.toHaveBeenCalled();
    expect(appUpdateApi.install).toHaveBeenCalledTimes(1);
  });
});
