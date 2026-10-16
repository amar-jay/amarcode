import { ArrowUpRight, MessagesSquare, Replace, Unplug } from "lucide-react";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import DownloadButton from "@/components/download-button";

const features = [
  {
    Icon: MessagesSquare,
    title: "Keep the conversation.",
    text: "Your visible chat history lives in Amarcode, independently of an agent’s session. Return to the work with the record still in place.",
  },
  {
    Icon: Replace,
    title: "Change your perspective.",
    text: "Switch agents within a conversation. When a native session can’t resume, Amarcode supplies saved text as context for the next prompt.",
  },
  {
    Icon: Unplug,
    title: "Leave your options open.",
    text: "Connect through ACP, an open protocol shared by a growing ecosystem of coding agents. Choose the tools that suit the task.",
  },
];
const questions = [
  {
    title: "Can I use Amarcode without the desktop app?",
    answer:
      "Yes. The daemon runs as an independent background service and can also run in the foreground for headless integrations. Clients communicate with it over its TCP or WebSocket API. Closing the desktop does not stop the installed service; the daemon has its own start, stop, and status commands.",
  },
  {
    title: "What is Amarcode?",
    answer:
      "Amarcode is a workspace for coding agents, powered by an independent headless daemon. The daemon owns agent processes and persistent workspace state; the desktop app is a client that connects to it.",
  },
  {
    title: "Can I continue a conversation with another agent?",
    answer:
      "Yes. Amarcode uses native session resumption when available. Otherwise, it provides saved user and assistant text as context with your next prompt. This carries the conversation forward, but does not restore hidden reasoning, provider state, or previous tool results in the new agent’s context.",
  },
  {
    title: "What is ACP?",
    answer:
      "The Agent Client Protocol is an open protocol that connects coding agents to applications. Amarcode uses it to communicate with compatible agents rather than requiring a separate integration for each one.",
  },
  {
    title: "Does Amarcode include access to the agents?",
    answer:
      "Agents have their own installation and authentication requirements. You use the account or API access required by the agent you choose; Amarcode provides the workspace around it.",
  },
];

export function Features() {
  return (
    <section
      className="bg-panel relative border-t border-rule before:pointer-events-none before:absolute before:-top-[5px] before:-left-[5px] before:z-20 before:size-[9px] before:bg-[linear-gradient(var(--color-rule),var(--color-rule)),linear-gradient(var(--color-rule),var(--color-rule))] before:bg-[size:1px_9px,9px_1px] before:bg-center before:bg-no-repeat before:content-['']"
      aria-labelledby="features-title"
			id="features"
    >
      <div className="grid grid-cols-[1fr_2fr] gap-[30px] px-[54px] pt-[65px] pb-[45px] [&>p]:mt-[13px] [&>div>p]:mb-0 [&>div>p]:max-w-[600px] [&>div>p]:text-[22px] [&>div>p]:leading-[1.4] [&>div>p]:text-muted-ink max-[901px]:grid-cols-1 max-[901px]:gap-1.5 max-[901px]:px-7 max-[901px]:pt-[45px] max-[901px]:pb-8 max-[901px]:[&>p]:mt-0 max-[901px]:[&>p]:mb-2 max-[601px]:px-5 max-[601px]:pt-9 max-[601px]:pb-7 max-[601px]:[&>div>p]:text-[20px]">
        <p className="mb-[22px] flex items-center gap-[9px] text-[16px] tracking-[0.065em] uppercase max-[601px]:text-[13px]">
          Continuity, by design
        </p>
        <div>
          <h2
            className="mb-[18px] text-[clamp(38px,4.4vw,60px)] leading-[1.07] font-light tracking-[-0.035em]"
            id="features-title"
          >
            The work moves forward.
            <br />
            The context stays with you.
          </h2>
        </div>
      </div>
      <div className="grid grid-cols-3 border-t border-rule [&>article]:px-[35px] [&>article]:pt-[37px] [&>article]:pb-8 [&>article+article]:border-l [&>article+article]:border-rule [&_svg]:mb-[34px] [&_h3]:mb-3.5 [&_h3]:text-[30px] [&_h3]:font-normal [&_h3]:tracking-[-0.025em] [&_p]:mb-0 [&_p]:text-[21px] [&_p]:leading-[1.45] [&_p]:text-muted-ink max-[901px]:[&>article]:px-[22px] max-[901px]:[&>article]:py-7 max-[901px]:[&_h3]:text-[26px] max-[601px]:grid-cols-1 max-[601px]:[&>article]:py-[30px] max-[601px]:[&>article+article]:border-l-0 max-[601px]:[&>article+article]:border-t max-[601px]:[&_svg]:mb-5">
        {features.map(({ Icon, title, text }) => (
          <article key={title}>
            <Icon size={26} strokeWidth={1.25} aria-hidden="true" />
            <h3>{title}</h3>
            <p>{text}</p>
          </article>
        ))}
      </div>
      <a
        className="inline-flex items-center gap-[18px] text-ink no-underline hover:underline m-0 w-full border-t border-rule px-[35px] py-[22px] text-[18px] max-[601px]:p-5 max-[601px]:text-[16px]"
        href="https://github.com/amar-jay/amarcode#advantages"
      >
        Read how conversation continuity works <ArrowUpRight size={17} />
      </a>
    </section>
  );
}
export function Questions() {
  return (
    <section
      id="questions"
      className="grid grid-cols-[1fr_1.3fr] max-[601px]:grid-cols-1 relative border-t border-rule before:pointer-events-none before:absolute before:-top-[5px] before:-left-[5px] before:z-20 before:size-[9px] before:bg-[linear-gradient(var(--color-rule),var(--color-rule)),linear-gradient(var(--color-rule),var(--color-rule))] before:bg-[size:1px_9px,9px_1px] before:bg-center before:bg-no-repeat before:content-['']"
      aria-labelledby="questions-title"
    >
      <div className="border-r border-rule px-[54px] py-[65px] [&>a]:mt-[15px] [&>a]:text-[18px] max-[901px]:px-7 max-[901px]:py-10 max-[601px]:border-r-0 max-[601px]:border-b max-[601px]:px-[22px] max-[601px]:py-9">
        <p className="mb-[22px] flex items-center gap-[9px] text-[16px] tracking-[0.065em] uppercase max-[601px]:text-[13px]">
          A little more context
        </p>
        <h2
          className="mb-[18px] text-[clamp(38px,4.4vw,60px)] leading-[1.07] font-light tracking-[-0.035em]"
          id="questions-title"
        >
          Good questions.
          <br />
          Clear answers.
        </h2>
        <a
          className="inline-flex items-center gap-[18px] text-ink no-underline hover:underline"
          href="https://github.com/amar-jay/amarcode"
        >
          Explore the project <ArrowUpRight size={17} />
        </a>
      </div>
      <Accordion
        type="single"
        collapsible
        className="px-10 py-7 [&_[data-slot=accordion-item]]:border-rule [&_[data-slot=accordion-trigger]]:rounded-none [&_[data-slot=accordion-trigger]]:px-0 [&_[data-slot=accordion-trigger]]:py-[25px] [&_[data-slot=accordion-trigger]]:text-[23px] [&_[data-slot=accordion-trigger]]:leading-[1.2] [&_[data-slot=accordion-trigger]]:font-normal [&_[data-slot=accordion-content]>div]:pb-[26px] [&_[data-slot=accordion-content]>div]:text-[20px] [&_[data-slot=accordion-content]>div]:leading-[1.5] [&_[data-slot=accordion-content]>div]:text-muted-ink motion-reduce:[&_[data-slot=accordion-content]]:animate-none max-[901px]:px-[25px] max-[901px]:py-[15px] max-[601px]:px-[22px] max-[601px]:[&_[data-slot=accordion-trigger]]:text-[21px]"
      >
        {questions.map(({ title, answer }, i) => (
          <AccordionItem key={title} value={`question-${i}`}>
            <AccordionTrigger>{title}</AccordionTrigger>
            <AccordionContent>{answer}</AccordionContent>
          </AccordionItem>
        ))}
      </Accordion>
    </section>
  );
}
export function Closing() {
  return (
    <>
      <section className="bg-[#e9efeb] bg-[linear-gradient(90deg,transparent_calc(25%_-_0.5px),var(--color-rule)_25%,transparent_calc(25%_+_0.5px),transparent_calc(75%_-_0.5px),var(--color-rule)_75%,transparent_calc(75%_+_0.5px)),linear-gradient(#fafaf680,#fafaf650),url('/images/misty-mountains.webp')] bg-cover bg-center px-[25px] pt-[65px] pb-[70px] text-center [&>p]:justify-center [&>h2]:mb-[30px] [&>h2]:text-[clamp(55px,6vw,82px)] max-[601px]:py-[45px] relative border-t border-rule before:pointer-events-none before:absolute before:-top-[5px] before:-left-[5px] before:z-20 before:size-[9px] before:bg-[linear-gradient(var(--color-rule),var(--color-rule)),linear-gradient(var(--color-rule),var(--color-rule))] before:bg-[size:1px_9px,9px_1px] before:bg-center before:bg-no-repeat before:content-['']">
        <p className="mb-[22px] flex items-center gap-[9px] text-[16px] tracking-[0.065em] uppercase max-[601px]:text-[13px]">
          Make room for what’s next
        </p>
        <h2 className="mb-[18px] text-[clamp(38px,4.4vw,60px)] leading-[1.07] font-light tracking-[-0.035em]">
          Many minds.
          <br />
          <em className="font-light">One place to build.</em>
        </h2>
        <DownloadButton />
      </section>
      <footer className="flex items-center justify-between gap-[25px] border-t border-rule px-10 py-[30px] text-[16px] max-[901px]:px-[25px] max-[601px]:flex-wrap max-[601px]:gap-[17px] max-[601px]:px-5 max-[601px]:py-[25px] max-[601px]:[&>span]:order-3 max-[601px]:[&>span]:w-full">
        <a
          href="#"
          className="inline-flex items-center gap-2.5 text-[29px] tracking-[-1px] text-ink no-underline max-[601px]:text-[26px]"
        >
          <img
            src="/images/app/amarcode-logo.svg"
            alt=""
            width={26}
            height={26}
          />
          <span>amarcode</span>
        </a>
        <span>A workspace for independent thinking.</span>
        <a
          className="inline-flex items-center gap-[18px] text-ink no-underline hover:underline"
          href="https://github.com/amar-jay/amarcode"
        >
          GitHub <ArrowUpRight size={15} />
        </a>
      </footer>
    </>
  );
}
