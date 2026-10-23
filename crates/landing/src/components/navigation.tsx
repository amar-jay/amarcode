import { ArrowUpRight } from "lucide-react";

export default function Navigation() {
  return (
    <header className="flex h-(--navigation-height) shrink-0 items-center justify-between gap-6 px-10 max-[901px]:px-[23px] max-[601px]:flex-wrap max-[601px]:gap-2.5 max-[601px]:p-[15px]">
      <a
        href="#"
        className="inline-flex items-center gap-2.5 text-[29px] tracking-[-1px] text-ink no-underline max-[601px]:text-[26px]"
        aria-label="Amarcode home"
      >
        <img
          src="/images/app/amarcode-logo.svg"
          width="30"
          height="30"
          alt=""
        />
        <span>amarcode</span>
      </a>
      <nav
        className="flex gap-[35px] text-[19px] [&>a]:text-ink [&>a]:no-underline [&>a:hover]:underline max-[901px]:gap-5 max-[601px]:order-3 max-[601px]:w-full max-[601px]:justify-between max-[601px]:border-t max-[601px]:border-rule max-[601px]:pt-3 max-[601px]:text-[17px]"
        aria-label="Main navigation"
      >
        {/* <a href="#app-preview">Workspace</a>
        <a href="#features">Features</a>
        <a href="#agents">Agents</a> */}
        <a href="/docs/">Docs</a>
      </nav>
      <a
        className="inline-flex items-center gap-[18px] border-l border-rule pl-7 text-ink no-underline max-[901px]:pl-[15px] max-[601px]:gap-1.5 max-[601px]:border-0 max-[601px]:p-0 max-[601px]:text-[16px]"
        href="https://github.com/amar-jay/amarcode"
      >
        View source <ArrowUpRight size={16} />
      </a>
    </header>
  );
}
