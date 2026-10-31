import { appUpdateApi, daemonApi } from "@/api";
import type {
  AppUpdateCheck,
  AppUpdateStatus,
  DaemonConnectionInfo,
  DaemonUpdateCheck,
  DaemonUpdateStatus,
} from "@/api";

export type UpdatePlan = {
  app: Extract<AppUpdateCheck, { status: "available" }> | null;
  daemon: Extract<DaemonUpdateCheck, { status: "available" }> | null;
  connection: DaemonConnectionInfo | null;
  issues: string[];
};
export type UpdateProgress =
  | { component: "application"; status: AppUpdateStatus }
  | { component: "daemon"; status: DaemonUpdateStatus }
  | { component: "complete" };
export const hasUpdates = (plan: UpdatePlan | null) =>
  !!(plan?.app || plan?.daemon);
export const updateError = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

/** A failed check never becomes an "up to date" result. */
export async function checkUpdatePlan(): Promise<UpdatePlan> {
  const [app, daemon, connection] = await Promise.allSettled([
    appUpdateApi.check(),
    daemonApi.checkUpdate(),
    daemonApi.getConnection(),
  ]);
  const plan: UpdatePlan = {
    app: null,
    daemon: null,
    connection: null,
    issues: [],
  };
  if (app.status === "fulfilled") {
    if (app.value.status === "available") plan.app = app.value;
  } else
    plan.issues.push(`Application check failed: ${updateError(app.reason)}`);
  if (connection.status === "fulfilled") plan.connection = connection.value;
  if (daemon.status === "fulfilled") {
    if (daemon.value.status === "available") {
      if (plan.connection) plan.daemon = daemon.value;
      else
        plan.issues.push(
          "Cannot identify the daemon connection. Check again before updating it.",
        );
    } else if (daemon.value.status === "unavailable")
      plan.issues.push(`Daemon: ${daemon.value.reason}`);
  } else plan.issues.push(`Daemon check failed: ${updateError(daemon.reason)}`);
  return plan;
}

/** Keep the verified native update/rollback mechanisms; restart the app last. */
export async function applyUpdatePlan(
  plan: UpdatePlan,
  progress: (value: UpdateProgress) => void,
  daemonComplete: () => void,
  reconcile: () => Promise<void>,
): Promise<void> {
  if (plan.daemon) {
    const current = await daemonApi.getConnection();
    if (
      !plan.connection ||
      current.scope !== plan.connection.scope ||
      current.mode !== plan.connection.mode
    )
      throw new Error(
        "The daemon connection changed. Check for updates again.",
      );
    progress({
      component: "daemon",
      status: { status: "downloading", received: 0, total: 0 },
    });
    await daemonApi.update(plan.daemon.version, (status) =>
      progress({ component: "daemon", status }),
    );
    // Retain partial success if the app installer fails and the user retries.
    daemonComplete();
    await reconcile();
  }
  if (plan.app) {
    progress({
      component: "application",
      status: { status: "downloading", received: 0, total: 0 },
    });
    await appUpdateApi.install((status) =>
      progress({ component: "application", status }),
    );
  }
  progress({ component: "complete" });
}
