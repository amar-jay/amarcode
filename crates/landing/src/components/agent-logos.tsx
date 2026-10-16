import { useState, type CSSProperties } from "react";
import { Ellipsis, ArrowUp } from "lucide-react";
import agents from "@/lib/agents.json";
import { Button } from "@/components/ui/button";

const activeAgents = agents.filter((agent) => !agent.quarantined);
const featured = [
  "claude-acp",
  "codex-acp",
  "gemini",
  "cursor",
  "github-copilot",
  "opencode",
  "goose",
  "kimi",
  "qwen-code",
  "amp-acp",
  "junie",
  "grok-build",
];
const orderedAgents = [...activeAgents].sort((a, b) => {
  const rank = (id: string) => {
    const index = featured.indexOf(id);
    return index === -1 ? 100 : index;
  };
  return rank(a.id) - rank(b.id) || a.name.localeCompare(b.name);
});

export default function AgentLogos() {
  const [expanded, setExpanded] = useState(false);
  const visibleAgents = expanded ? orderedAgents : orderedAgents.slice(0, 11);
  return (
    <section
      id="agents"
      aria-labelledby="agents-title"
      className="relative border-t border-rule before:pointer-events-none before:absolute before:-top-[5px] before:-left-[5px] before:z-20 before:size-[9px] before:bg-[linear-gradient(var(--color-rule),var(--color-rule)),linear-gradient(var(--color-rule),var(--color-rule))] before:bg-[size:1px_9px,9px_1px] before:bg-center before:bg-no-repeat before:content-['']"
    >
      <div className="grid grid-cols-[1fr_2fr] gap-[30px] px-[54px] pt-[65px] pb-[45px] [&>p]:mt-[13px] [&>div>p]:mb-0 [&>div>p]:max-w-[600px] [&>div>p]:text-[22px] [&>div>p]:leading-[1.4] [&>div>p]:text-muted-ink max-[901px]:grid-cols-1 max-[901px]:gap-1.5 max-[901px]:px-7 max-[901px]:pt-[45px] max-[901px]:pb-8 max-[901px]:[&>p]:mt-0 max-[901px]:[&>p]:mb-2 max-[601px]:px-5 max-[601px]:pt-9 max-[601px]:pb-7 max-[601px]:[&>div>p]:text-[20px]">
        <p className="mb-[22px] flex items-center gap-[9px] text-[16px] tracking-[0.065em] uppercase max-[601px]:text-[13px]">
          An open ecosystem
        </p>
        <div>
          <h2
            className="mb-[18px] text-[clamp(38px,4.4vw,60px)] leading-[1.07] font-light tracking-[-0.035em]"
            id="agents-title"
          >
            Choose your collaborators.
          </h2>
          <p>
            Bring the agents you already work with. Discover the ones you’ll
            work with next.
          </p>
        </div>
      </div>
      <ul
        id="agent-directory"
        className="m-0 grid list-none grid-cols-6 border-t border-rule p-0 max-[901px]:grid-cols-4 max-[601px]:grid-cols-3"
      >
        {visibleAgents.map((agent) => (
          <li
            key={agent.id}
            className="flex min-h-[135px] flex-col items-center justify-center gap-[18px] border-r border-b border-rule p-0 text-center text-[18px] min-[901px]:[&:nth-child(6n)]:border-r-0 min-[601px]:max-[901px]:[&:nth-child(4n)]:border-r-0 max-[601px]:min-h-[120px] max-[601px]:text-[16px] max-[601px]:[&:nth-child(3n)]:border-r-0"
          >
            <a
              href={agent.repository}
              className="group relative flex min-h-[inherit] w-full flex-1 flex-col items-center justify-center gap-[18px] overflow-hidden px-3 py-8 text-inherit no-underline transition-colors duration-[240ms] hover:bg-[#edf1eb] focus-visible:bg-[#edf1eb] focus-visible:-outline-offset-[5px] after:absolute after:bottom-0 after:left-0 after:h-0.5 after:w-full after:origin-left after:scale-x-0 after:bg-amber after:transition-transform after:duration-350 after:ease-out after:content-[''] hover:after:scale-x-100 focus-visible:after:scale-x-100 motion-reduce:transition-none motion-reduce:after:transition-none max-[601px]:px-2 max-[601px]:py-[23px]"
              aria-label={`${agent.name} project`}
            >
              <span
                aria-hidden="true"
                className="block size-[27px] bg-ink [mask-image:var(--agent-icon)] [mask-size:contain] mask-center mask-no-repeat transition-transform duration-350 ease-[cubic-bezier(0.2,0.8,0.2,1)] group-hover:-translate-y-[5px] group-hover:-rotate-5 group-hover:scale-[1.12] group-focus-visible:-translate-y-[5px] group-focus-visible:-rotate-5 group-focus-visible:scale-[1.12] motion-reduce:[transform:none]! motion-reduce:[translate:none]! motion-reduce:[rotate:none]! motion-reduce:[scale:none]! motion-reduce:transition-none"
                style={
                  {
                    "--agent-icon": `url(${agent.icon})`,
                  } as CSSProperties
                }
              />
              <span>{agent.name}</span>
            </a>
          </li>
        ))}
        {!expanded && orderedAgents.length > 11 && (
          <li className="flex min-h-[135px] flex-col items-center justify-center gap-[18px] border-r border-b border-rule p-0 text-center text-[18px] min-[901px]:[&:nth-child(6n)]:border-r-0 min-[601px]:max-[901px]:[&:nth-child(4n)]:border-r-0 max-[601px]:min-h-[120px] max-[601px]:text-[16px] max-[601px]:[&:nth-child(3n)]:border-r-0">
            <button
              type="button"
              className="flex min-h-[inherit] w-full flex-1 cursor-pointer flex-col items-center justify-center gap-[18px] px-3 py-8 text-ink transition-colors hover:bg-[#edf1eb] focus-visible:bg-[#edf1eb] motion-reduce:transition-none max-[601px]:px-2 max-[601px]:py-[23px]"
              aria-label="Show all agents"
              aria-expanded={false}
              aria-controls="agent-directory"
              onClick={() => {
                setExpanded(true);
                requestAnimationFrame(() => {
                  document
                    .querySelector<HTMLAnchorElement>(
                      "#agent-directory li:nth-child(12) a",
                    )
                    ?.focus({ preventScroll: true });
                });
              }}
            >
              <Ellipsis size={27} aria-hidden="true" />
              <span>More agents</span>
            </button>
          </li>
        )}
      </ul>
      <div className="flex items-center justify-between gap-5 px-[35px] py-5 [&>p]:m-0 [&>p]:text-[16px] [&>p]:text-muted-ink max-[601px]:flex-col max-[601px]:gap-3 max-[601px]:px-[15px] max-[601px]:py-[22px]">
        <p>{activeAgents.length} active entries in the local ACP registry.</p>
        {expanded && (
          <Button
            variant="ghost"
            className="rounded-none text-[18px] font-normal"
            aria-expanded={true}
            aria-controls="agent-directory"
            onClick={() => {
              setExpanded(false);
              requestAnimationFrame(() => {
                document
                  .querySelector<HTMLButtonElement>("#agent-directory button")
                  ?.focus({ preventScroll: true });
              });
            }}
          >
            Show fewer agents <ArrowUp />
          </Button>
        )}
      </div>
    </section>
  );
}
