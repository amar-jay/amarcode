import { toast } from "sonner"

export type NotificationKind = "success" | "error" | "info" | "warning"

export function notify(message: string, kind: NotificationKind = "info") {
  toast[kind](message)
}
