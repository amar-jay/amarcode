import { useAtomValue, useSetAtom } from "jotai";
import { agentSetupTargetAtom } from "@/state/agents";
import { agentSignInRequiredAtom } from "@/state/daemon-events";
import { LogIn } from "lucide-react";
import { TooltipButton } from "@/components/ui/tooltip-button";

export function AgentSetupButton({
  agent,
  onOpen,
}: {
  agent: { id: string; name: string };
  onOpen: () => void;
}) {
  const required = useAtomValue(agentSignInRequiredAtom)[agent.id];
  const openSetup = useSetAtom(agentSetupTargetAtom);
  if (!required) return null;
  return (
    <TooltipButton
      type="button"
      variant="ghost"
      size="icon"
      className="ml-auto size-6 shrink-0 text-primary"
      title={`Sign in to ${agent.name}`}
      aria-label={`Sign in to ${agent.name}`}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") event.stopPropagation();
      }}
      onClick={(event) => {
        event.preventDefault();
        event.stopPropagation();
        onOpen();
        openSetup({ id: agent.id, name: agent.name });
      }}
    >
      <LogIn className="size-3.5" />
    </TooltipButton>
  );
}
