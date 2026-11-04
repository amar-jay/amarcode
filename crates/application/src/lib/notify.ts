import { isWeb } from "@/lib/platform";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { toast, type ExternalToast } from "sonner";
import { createElement } from "react";
import type { LucideIcon } from "lucide-react";
import { ToastIconAction } from "@/components/toast-icon-action";

type IconToastAction = { label: string; icon: LucideIcon; onClick: () => void };
type NotifyToastOptions = Omit<ExternalToast, "action"> & {
  action?: ExternalToast["action"] | IconToastAction;
};

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
  if (isWeb) return;
  try {
    const pageIsFocused = () =>
      typeof document !== "undefined" && document.hasFocus();
    // The Tauri bridge is absent when the Vite app is opened in a browser.
    if (pageIsFocused()) return;
    const window = getCurrentWindow();
    if (await window.isFocused()) return;
    if (!(await hasNotificationPermission())) return;
    // Permission checks (and the permission dialog) can outlive a focus change.
    // Recheck native focus, then synchronously check the webview before dispatch.
    if (await window.isFocused()) return;
    if (pageIsFocused()) return;
    sendNotification({ title, body });
  } catch {
    // Native notifications are an enhancement; the in-app toast still works.
  }
}

export function notify(
  message: string,
  kind: NotificationKind = "info",
  options?: ExternalToast,
) {
  toast[kind](message, options);
  void sendWhenAppIsUnfocused(
    kind === "info" ? "Amarcode" : `Amarcode, ${kind}`,
    message,
  );
}

/** For Sonner features such as an action button that have no OS equivalent. */
export function notifyToast(message: string, options?: NotifyToastOptions) {
  const action = options?.action;
  if (action && typeof action === "object" && "icon" in action) {
    const id = toast(message, {
      ...options,
      action: createElement(ToastIconAction, {
        label: action.label,
        icon: action.icon,
        onClick: () => {
          action.onClick();
          toast.dismiss(id);
        },
      }),
    });
  } else {
    toast(message, options as ExternalToast | undefined);
  }
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
