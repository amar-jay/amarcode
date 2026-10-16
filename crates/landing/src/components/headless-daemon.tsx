import {
  ArrowDown,
  ArrowRight,
  ArrowUpRight,
  AppWindow,
  Server,
  Unplug,
} from "lucide-react";

const layers = [
  {
    Icon: AppWindow,
    title: "Your client",
    description:
      "Use the desktop app, a CLI, or your own integration to connect to the same workspace.",
  },
  {
    Icon: Server,
    title: "Independent daemon",
    description:
      "A headless background service owns agent processes, chats, messages, and runs.",
  },
  {
    Icon: Unplug,
    title: "Your coding agents",
    description:
      "The daemon talks to compatible agents through the open Agent Client Protocol.",
  },
];

export default function HeadlessDaemon() {
  return (
    <section
      id="daemon"
      aria-labelledby="daemon-title"
      className="relative border-t border-rule before:pointer-events-none before:absolute before:-top-[5px] before:-left-[5px] before:z-20 before:size-[9px] before:bg-[linear-gradient(var(--color-rule),var(--color-rule)),linear-gradient(var(--color-rule),var(--color-rule))] before:bg-[size:1px_9px,9px_1px] before:bg-center before:bg-no-repeat before:content-['']"
    >
      <div className="grid grid-cols-[1fr_2fr] gap-[30px] px-[54px] pt-[65px] pb-[45px] max-[901px]:grid-cols-1 max-[901px]:gap-1.5 max-[901px]:px-7 max-[901px]:pt-[45px] max-[901px]:pb-8 max-[601px]:px-5 max-[601px]:pt-9 max-[601px]:pb-7">
        <p className="mt-[13px] mb-[22px] text-[16px] tracking-[0.065em] uppercase max-[901px]:mt-0 max-[901px]:mb-2 max-[601px]:text-[13px]">
          Independent by architecture
        </p>
        <div>
          <h2
            id="daemon-title"
            className="mb-[18px] text-[clamp(38px,4.4vw,60px)] leading-[1.07] font-light tracking-[-0.035em]"
          >
            Close the window.
            <br />
            The daemon stays.
          </h2>
          <p className="mb-0 max-w-[620px] text-[22px] leading-[1.4] text-muted-ink max-[601px]:text-[20px]">
            Amarcode runs on an independent headless daemon. It manages your
            agents and keeps your workspace state. The desktop connects to it;
            closing the app leaves the service running.
          </p>
        </div>
      </div>
      <ol
        aria-label="How clients connect to coding agents"
        className="m-0 grid list-none grid-cols-3 border-t border-rule p-0 max-[601px]:grid-cols-1"
      >
        {layers.map(({ Icon, title, description }, index) => (
          <li
            key={title}
            className="relative px-[35px] py-9 not-first:border-l not-first:border-rule max-[901px]:px-[22px] max-[601px]:not-first:border-t max-[601px]:not-first:border-l-0"
          >
            <Icon
              size={26}
              strokeWidth={1.25}
              aria-hidden="true"
              className="mb-7"
            />
            <h3 className="mb-3.5 text-[30px] font-normal tracking-[-0.025em] max-[901px]:text-[26px]">
              {title}
            </h3>
            <p className="mb-0 text-[21px] leading-[1.45] text-muted-ink">
              {description}
            </p>
            {index < layers.length - 1 && (
              <span
                aria-hidden="true"
                className="absolute top-9 -right-[13px] z-10 flex size-[26px] items-center justify-center bg-paper max-[601px]:top-auto max-[601px]:-bottom-[13px] max-[601px]:left-[22px]"
              >
                <ArrowRight size={17} className="max-[601px]:hidden" />
                <ArrowDown size={17} className="hidden max-[601px]:block" />
              </span>
            )}
          </li>
        ))}
      </ol>
      <div className="flex items-center justify-between gap-5 border-t border-rule bg-panel px-[35px] py-[22px] max-[601px]:flex-col max-[601px]:items-start max-[601px]:p-5">
        <p className="mb-0 max-w-[650px] text-[19px] leading-[1.4] text-muted-ink">
          Run headless, connect your own client, or use the desktop. Your
          workspace belongs to the service.
        </p>
        <a
          href="https://github.com/amar-jay/amarcode/tree/main/crates/daemon#local-run"
          className="inline-flex shrink-0 items-center gap-3 text-[18px] text-ink no-underline hover:underline"
        >
          Explore the daemon <ArrowUpRight size={17} aria-hidden="true" />
        </a>
      </div>
    </section>
  );
}
