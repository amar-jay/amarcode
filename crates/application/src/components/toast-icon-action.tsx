import type { LucideIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

export function ToastIconAction({
  label,
  icon: Icon,
  onClick,
}: {
  label: string;
  icon: LucideIcon;
  onClick: () => void;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          className="ml-auto size-8 shrink-0"
          aria-label={label}
          onClick={onClick}
        >
          <Icon className="size-4" aria-hidden="true" />
        </Button>
      </TooltipTrigger>
      <TooltipContent className="z-[110]">{label}</TooltipContent>
    </Tooltip>
  );
}
