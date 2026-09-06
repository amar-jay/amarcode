import { BotIcon } from "lucide-react";

export function AgentLogo({ icon }: { icon?: string | null }) {
  return icon ? (
    <img
      src={icon}
      alt=""
      className="size-4 shrink-0 object-contain"
    />
  ) : (
    <span className="flex size-4 shrink-0 items-center justify-center text-muted-foreground">
      <BotIcon className="size-4" aria-hidden="true" />
    </span>
  );
}
