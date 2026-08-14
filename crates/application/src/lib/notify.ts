import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { toast, type ExternalToast } from "sonner";

export type NotificationKind = "success" | "error" | "info" | "warning";

const announcedAttention = new Set<string>();
let permissionRequest: Promise<boolean> | undefined;

async function hasNotificationPermission(): Promise<boolean> {
  if (await isPermissionGranted()) return true;

  permissionRequest ??= requestPermission().then(
    (permission) => permission === "granted",
  );
  return permissionRequest;
}

async function sendWhenAppIsUnfocused(title: string, body: string) {
  try {
    // The Tauri bridge is absent when the Vite app is opened in a browser.
    if (await getCurrentWindow().isFocused()) return;
    if (!(await hasNotificationPermission())) return;
    sendNotification({ title, body });
  } catch {
    // Native notifications are an enhancement; the in-app toast still works.
  }
}

export function notify(message: string, kind: NotificationKind = "info") {
  toast[kind](message);
  void sendWhenAppIsUnfocused(
    kind === "info" ? "Amarcode" : `Amarcode — ${kind}`,
    message,
  );
}

/** For Sonner features such as an action button that have no OS equivalent. */
export function notifyToast(message: string, options?: ExternalToast) {
  toast(message, options);
  const body =
    typeof options?.description === "string" ? options.description : message;
  void sendWhenAppIsUnfocused("Amarcode", body);
}

/** Announces a background event that is already represented by a blocking dialog. */
export function notifyAttention(id: string, title: string, body: string) {
  if (announcedAttention.has(id)) return;
  announcedAttention.add(id);
  void sendWhenAppIsUnfocused(title, body);
}
