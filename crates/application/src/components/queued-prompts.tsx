import { useState } from "react";
import { ListPlus, Paperclip, Pause, Pencil, Play, X } from "lucide-react";
import type { PromptQueueState } from "@/types";
import { Button } from "./ui/button";
import { TooltipButton } from "./ui/tooltip-button";

type Action = {
  action: "pause" | "resume" | "remove" | "edit";
  id?: string;
  text?: string;
};
export function QueuedPrompts({
  queue,
  onUpdate,
}: {
  queue?: PromptQueueState;
  onUpdate: (action: Action) => Promise<void>;
}) {
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const items = queue?.items.filter((item) => item.status !== "running") ?? [];
  if (!items.length) return null;
  const waiting = items.filter((item) => item.status === "queued").length;
  const update = async (action: Action) => {
    if (busy) return;
    setBusy(true);
    try {
      await onUpdate(action);
      if (action.action === "edit" || action.action === "remove")
        setEditing(null);
    } catch {
      /* The state action displays the delivery error. Keep edits for retry. */
    } finally {
      setBusy(false);
    }
  };
  return (
    <section
      aria-label="Follow-up queue"
      className="mb-2 overflow-hidden rounded-lg border border-border bg-muted/20 text-xs"
    >
      <div className="flex items-center gap-2 px-3 py-1.5 text-muted-foreground">
        <ListPlus className="size-3.5" />
        <span aria-live="polite">
          {waiting} queued{queue?.paused ? " · Paused" : ""}
        </span>
        {waiting > 0 && (
          <Button
            type="button"
            size="xs"
            variant="ghost"
            disabled={busy}
            className="ml-auto"
            onClick={() =>
              void update({ action: queue?.paused ? "resume" : "pause" })
            }
          >
            {queue?.paused ? (
              <Play className="size-3" />
            ) : (
              <Pause className="size-3" />
            )}
            {queue?.paused ? "Resume" : "Pause"}
          </Button>
        )}
      </div>
      {queue?.paused && queue.reason && (
        <p className="px-3 pb-2 text-[11px] text-muted-foreground">
          {queue.reason}
        </p>
      )}
      <ol className="max-h-40 overflow-y-auto border-t border-border/60">
        {items.map((item, index) => (
          <li
            key={item.id}
            className="flex items-start gap-2 border-b border-border/40 px-3 py-2 last:border-b-0"
          >
            <span className="mt-0.5 text-[10px] tabular-nums text-muted-foreground">
              {index + 1}
            </span>
            <div className="min-w-0 flex-1">
              {editing === item.id ? (
                <>
                  <textarea
                    aria-label="Edit queued message"
                    autoFocus
                    value={draft}
                    onChange={(event) => setDraft(event.target.value)}
                    className="min-h-16 w-full resize-y rounded border border-input bg-background p-2 outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  />
                  <div className="mt-1 flex gap-1">
                    <Button
                      type="button"
                      size="xs"
                      disabled={
                        busy || (!draft.trim() && !item.attachment_count)
                      }
                      onClick={() =>
                        void update({
                          action: "edit",
                          id: item.id,
                          text: draft,
                        })
                      }
                    >
                      Save
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="xs"
                      disabled={busy}
                      onClick={() => setEditing(null)}
                    >
                      Cancel
                    </Button>
                  </div>
                </>
              ) : (
                <p className="line-clamp-2 whitespace-pre-wrap break-words">
                  {item.text || "Attachment message"}
                </p>
              )}
              {item.attachment_count > 0 && (
                <span className="mt-1 inline-flex items-center gap-1 text-[10px] text-muted-foreground">
                  <Paperclip className="size-3" />
                  {item.attachment_count} attachment
                  {item.attachment_count === 1 ? "" : "s"}
                </span>
              )}
              {item.status !== "queued" && (
                <p className="mt-1 text-[11px] text-muted-foreground">
                  {item.status === "interrupted" ? "Interrupted" : "Stopped"} ·
                  Review the chat before resending.
                </p>
              )}
            </div>
            {item.status === "queued" && editing !== item.id && (
              <TooltipButton
                type="button"
                variant="ghost"
                size="icon-xs"
                title="Edit queued message"
                aria-label="Edit queued message"
                disabled={busy}
                onClick={() => {
                  setEditing(item.id);
                  setDraft(item.text);
                }}
              >
                <Pencil className="size-3" />
              </TooltipButton>
            )}
            <TooltipButton
              type="button"
              variant="ghost"
              size="icon-xs"
              title="Remove queued message"
              aria-label="Remove queued message"
              disabled={busy}
              onClick={() => void update({ action: "remove", id: item.id })}
            >
              <X className="size-3" />
            </TooltipButton>
          </li>
        ))}
      </ol>
    </section>
  );
}
