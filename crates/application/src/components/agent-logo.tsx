import { BotIcon } from "lucide-react";

export function AgentLogo({ icon }: { icon?: string | null }) {
  return icon ? (
    <span
      aria-hidden="true"
      className="size-4 shrink-0 bg-primary"
      style={{
        maskImage: `url("${icon}")`,
        maskPosition: "center",
        maskRepeat: "no-repeat",
        maskSize: "contain",
        WebkitMaskImage: `url("${icon}")`,
        WebkitMaskPosition: "center",
        WebkitMaskRepeat: "no-repeat",
        WebkitMaskSize: "contain",
      }}
    />
  ) : (
    <span className="flex size-4 shrink-0 items-center justify-center text-primary">
      <BotIcon className="size-4" aria-hidden="true" />
    </span>
  );
}
