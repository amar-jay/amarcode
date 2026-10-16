import Navigation from "@/components/navigation";
import CloudHero from "@/components/cloud-hero";
import AppPreview from "@/components/app-preview";
import AgentLogos from "@/components/agent-logos";
import { Features, Questions, Closing } from "@/components/sections";

export default function App() {
  return (
    <>
      <a
        href="#main"
        className="fixed -top-[100px] left-5 z-[100] bg-paper p-3 text-ink focus:top-2.5"
      >
        Skip to content
      </a>
      <div className="[--navigation-height:88px] max-[601px]:[--navigation-height:118px] motion-reduce:[&_*]:animate-none motion-reduce:[&_*]:transition-none mx-auto max-w-[1392px] border-x border-rule max-[901px]:mx-5 max-[601px]:mx-3 [&_a]:touch-manipulation [&_a]:underline-offset-[5px] [&_a]:[-webkit-tap-highlight-color:transparent] [&_button]:touch-manipulation [&_button]:rounded-none [&_button]:[-webkit-tap-highlight-color:transparent] [&_[data-slot]]:rounded-none [&_a:focus-visible]:outline-2 [&_a:focus-visible]:outline-[#80602a] [&_a:focus-visible]:outline-offset-[5px] [&_button:focus-visible]:outline-2 [&_button:focus-visible]:outline-[#80602a] [&_button:focus-visible]:outline-offset-[5px] [&_[role=tab]:focus-visible]:outline-2 [&_[role=tab]:focus-visible]:outline-[#80602a] [&_[role=tab]:focus-visible]:outline-offset-[5px]">
        <Navigation />
        <main id="main">
          <CloudHero />
          <AppPreview />
          <Features />
          <AgentLogos />
          <Questions />
          <Closing />
        </main>
      </div>
    </>
  );
}
