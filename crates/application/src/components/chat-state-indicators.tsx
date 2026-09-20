import {
  CircleCheck,
  CircleStop,
  CircleX,
  LoaderCircle,
  Pickaxe,
  RotateCcwClock,
} from "lucide-react";
import type { RunStatus } from "@/types";

type ChatStateIndicatorsProps = {
  loading: boolean;
  isWorking: boolean;
  contextRestoration: string | null;
  runStatus: RunStatus | null;
};

export function ChatStateIndicators({
  loading,
  isWorking,
  contextRestoration,
  runStatus,
}: ChatStateIndicatorsProps) {
  const RunStatusIcon =
    runStatus === "completed"
      ? CircleCheck
      : runStatus === "stopped"
        ? CircleStop
        : runStatus === "failed"
          ? CircleX
          : LoaderCircle;
    const status = contextRestoration
      ? {
          Icon: RotateCcwClock,
          label: `${contextRestoration}`,
          className:
            "size-3.5 animate-[spin_2.5s_linear_infinite] motion-reduce:animate-none",
        }
      : isWorking
        ? {
            Icon: Pickaxe,
            label: "Building response",
            className:
              "size-4 origin-bottom animate-[pickaxe_0.45s_ease-in-out_infinite] motion-reduce:animate-none",
          }
        : loading
          ? {
              Icon: LoaderCircle,
              label: "Loading chat",
              className: "size-3.5 animate-spin motion-reduce:animate-none",
            }
          : runStatus && runStatus !== "running"
            ? {
                Icon: RunStatusIcon,
                label:
                  runStatus.charAt(0).toUpperCase() + runStatus.slice(1),
                className: "size-3.5 animate-pulse motion-reduce:animate-none",
              }
            : null;

    if (!status) return null;

  return (
    <span className="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
      <status.Icon className={status.className} aria-hidden="true" />
      {status.label}
    </span>
  );
}
