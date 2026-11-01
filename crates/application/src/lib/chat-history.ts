import type { MessageDetail } from "@/types";

export const CHAT_HISTORY_PAGE_SIZE = 40;

/** Incoming snapshots replace matching IDs; pages never duplicate messages. */
export function mergeChatHistory(
  existing: MessageDetail[],
  incoming: MessageDetail[],
): MessageDetail[] {
  const messages = new Map(existing.map((item) => [item.message.id, item]));
  for (const item of incoming) messages.set(item.message.id, item);
  return [...messages.values()].sort((left, right) => {
    const a = left.message.created_at;
    const b = right.message.created_at;
    return a < b
      ? -1
      : a > b
        ? 1
        : left.message.id < right.message.id
          ? -1
          : left.message.id > right.message.id
            ? 1
            : 0;
  });
}
