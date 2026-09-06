import { BotIcon } from "lucide-react";

export function AgentLogo({ agentId }: { agentId: string }) {
  const normalizedId = agentId.toLowerCase();
  const source = normalizedId.includes("codex")
    ? "/agents/openai.svg"
    : normalizedId.includes("claude")
      ? "/agents/claude.svg"
      : normalizedId.includes("copilot")
        ? "/agents/github-copilot.svg"
        : normalizedId.includes("grok")
          ? "/agents/grok.svg"
          : null;

  return source ? (
    <img
      src={source}
      alt=""
      className="size-4 shrink-0 object-contain dark:invert"
    />
  ) : (
    <span className="flex size-4 shrink-0 items-center justify-center text-muted-foreground">
      <BotIcon className="size-4" aria-hidden="true" />
    </span>
  );
}
