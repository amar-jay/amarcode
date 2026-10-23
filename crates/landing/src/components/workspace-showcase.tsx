import { useState } from "react";
import {
  ArrowUpRight,
  FileCode2,
  GitCompareArrows,
  Replace,
  Users,
} from "lucide-react";

const views = [
  {
    id: "editor",
    label: "Edit code",
    Icon: FileCode2,
    title: "The file is right here.",
    description:
      "Browse your project and edit code alongside the conversation. Keep your draft and undo history as you move between files.",
    image: "editor",
    width: 1016,
    height: 1011,
    alt: "Amarcode’s project tree and editor displaying a TypeScript file with syntax highlighting.",
    link: "/docs/workspace/files-and-git/",
  },
  {
    id: "review",
    label: "Review changes",
    Icon: GitCompareArrows,
    title: "See what changed. Decide what stays.",
    description:
      "Read the diff, stage files, and commit your changes from the same workspace. Your agent’s edits stay open to review.",
    image: "git-review",
    width: 1126,
    height: 1011,
    alt: "Amarcode’s Git review showing staged and unstaged files, an inline README diff, and the commit input.",
    link: "/docs/workspace/files-and-git/",
  },
  {
    id: "agents",
    label: "Choose an agent",
    Icon: Users,
    title: "A different agent, a familiar place.",
    description:
      "Choose an installed agent inside your conversation, or use built-in Amarcode ACP with your own OpenAI-compatible provider.",
    image: "agent-picker",
    width: 384,
    height: 330,
    alt: "The agent picker listing Amarcode ACP, Codex, Copilot, Grok Build, Muse Code, OpenCode, and pi.",
    link: "/docs/agents/overview/",
  },
  {
    id: "handoff",
    label: "Follow the handoff",
    Icon: Replace,
    title: "One conversation. Three perspectives.",
    description:
      "A real conversation moving from Codex to Grok Build to Amarcode ACP. The activity timeline keeps each agent’s run visible in the same chat.",
    image: "agent-handoff",
    width: 1596,
    height: 385,
    alt: "One Amarcode conversation’s activity timeline showing successive runs by codex-acp, grok-build, and amarcode-acp.",
    link: "/docs/concepts/continuity/",
  },
] as const;

export default function WorkspaceShowcase() {
  const [selected, setSelected] = useState<string>("editor");
  const view = views.find((item) => item.id === selected) ?? views[0];

  return (
    <section
      id="workspace"
      aria-labelledby="workspace-title"
      className="border-t border-rule"
    >
      <div className="grid grid-cols-[1fr_2fr] gap-[30px] px-[54px] pt-[65px] pb-[45px] max-[901px]:grid-cols-1 max-[901px]:gap-2 max-[901px]:px-7 max-[901px]:py-10 max-[601px]:px-5 max-[601px]:py-9">
        <p className="mt-3 mb-0 text-[16px] tracking-[0.065em] uppercase max-[901px]:mt-0 max-[601px]:text-[13px]">
          Inside your workspace
        </p>
        <div>
          <h2
            id="workspace-title"
            className="mb-[18px] text-[clamp(38px,4.4vw,60px)] leading-[1.07] font-light tracking-[-0.035em]"
          >
            From conversation
            <br />
            to changes you can see.
          </h2>
          <p className="mb-0 max-w-[620px] text-[22px] leading-[1.4] text-muted-ink max-[601px]:text-[20px]">
            Open the code, review the work, and bring another agent into the
            conversation.
          </p>
        </div>
      </div>
      <div
        role="group"
        aria-label="Workspace views"
        className="grid grid-cols-4 border-t border-rule max-[601px]:grid-cols-2"
      >
        {views.map(({ id, label, Icon }) => (
          <button
            key={id}
            type="button"
            aria-pressed={selected === id}
            aria-controls="workspace-view"
            onClick={() => setSelected(id)}
            className="flex cursor-pointer items-center justify-center gap-3 border-r border-rule px-3 py-5 text-[20px] last:border-r-0 hover:bg-panel aria-pressed:bg-ink aria-pressed:text-paper max-[601px]:border-b max-[601px]:px-2 max-[601px]:py-4 max-[601px]:text-[17px] max-[601px]:[&:nth-child(2)]:border-r-0"
          >
            <Icon size={18} strokeWidth={1.4} aria-hidden="true" />
            {label}
          </button>
        ))}
      </div>
      <div
        id="workspace-view"
        className="grid grid-cols-[1fr_2fr] border-t border-rule max-[901px]:grid-cols-1"
      >
        <div
          aria-live="polite"
          className="flex flex-col items-start px-[35px] py-10 max-[901px]:px-7 max-[901px]:py-7 max-[601px]:px-5"
        >
          <h3 className="mb-4 text-[34px] leading-[1.12] font-normal tracking-[-0.025em]">
            {view.title}
          </h3>
          <p className="mb-7 text-[21px] leading-[1.45] text-muted-ink">
            {view.description}
          </p>
          <a
            href={view.link}
            className="mt-auto inline-flex items-center gap-3 text-[18px] text-ink no-underline hover:underline"
          >
            Explore in the handbook{" "}
            <ArrowUpRight size={16} aria-hidden="true" />
          </a>
        </div>
        <figure className="m-0 flex min-h-[460px] flex-col justify-center border-l border-rule bg-panel p-7 max-[901px]:min-h-0 max-[901px]:border-t max-[901px]:border-l-0 max-[601px]:p-4">
          <a
            href={`/images/workspace/${view.image}.webp`}
            target="_blank"
            rel="noreferrer"
            aria-label={`Open full-size screenshot: ${view.label}`}
            className="block"
          >
            <img
              key={view.image}
              src={`/images/workspace/${view.image}.webp`}
              width={view.width}
              height={view.height}
              alt={view.alt}
              loading="lazy"
              decoding="async"
              className={`mx-auto block h-auto max-h-[560px] max-w-full border border-rule object-contain ${view.id === "agents" ? "w-[384px]" : "w-full"}`}
            />
          </a>
          <figcaption className="mt-4 text-center text-[16px] text-muted-ink">
            {view.id === "handoff"
              ? "Codex → Grok Build → Amarcode ACP"
              : "Captured in Amarcode"}{" "}
            · Open image to enlarge
          </figcaption>
        </figure>
      </div>
    </section>
  );
}
