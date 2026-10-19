import { ArrowDown } from "lucide-react";
import { CloudShader } from "@/components/ui/cloud-shader";
import DownloadButton from "@/components/download-button";
import { Button } from "@/components/ui/button";

export default function CloudHero() {
  return (
    <section
      className="grid min-h-[calc(100svh-var(--navigation-height))] grid-rows-[1fr_auto] overflow-hidden relative border-t border-rule before:pointer-events-none before:absolute before:-top-[5px] before:-left-[5px] before:z-20 before:size-[9px] before:bg-[linear-gradient(var(--color-rule),var(--color-rule)),linear-gradient(var(--color-rule),var(--color-rule))] before:bg-[size:1px_9px,9px_1px] before:bg-center before:bg-no-repeat before:content-['']"
      aria-labelledby="hero-title"
    >
      <CloudShader
        className="isolate after:pointer-events-none after:absolute after:inset-0 after:z-[1] after:bg-[url('/images/grain.svg')] after:bg-[size:180px_180px] after:bg-repeat after:opacity-30 after:mix-blend-soft-light after:content-[''] h-full min-h-0 before:pointer-events-none before:absolute before:inset-0 before:z-[1] before:bg-[linear-gradient(90deg,transparent_calc(25%_-_0.5px),#53665f14_25%,transparent_calc(25%_+_0.5px),transparent_calc(75%_-_0.5px),#53665f14_75%,transparent_calc(75%_+_0.5px))] before:content-['']"
        speed={0.3}
      >
        <div className="max-[601px]:[@media(max-height:700px)]:py-4 max-[601px]:[@media(max-height:600px)]:py-2! relative max-w-[970px] px-7 py-[clamp(20px,5svh,62px)] text-center max-[601px]:px-[18px]">
          <h1
            className="mb-[clamp(14px,2.6svh,26px)] text-[clamp(56px,7.1vw,100px)] leading-[0.99] font-light tracking-[-0.055em] max-[601px]:text-[clamp(46px,11vw,66px)] max-[601px]:[@media(max-height:700px)]:text-[38px] max-[601px]:[@media(max-height:700px)]:mb-3 max-[601px]:[@media(max-height:600px)]:text-[32px]!"
            id="hero-title"
          >
            Different agents.
            <br />
            One continuous <em className="font-light">thought.</em>
          </h1>
          <p className="mx-auto mb-[clamp(16px,2.9svh,29px)] max-w-[565px] text-[23px] leading-[1.4] max-[601px]:text-[21px] max-[601px]:[@media(max-height:700px)]:text-[19px] max-[601px]:[@media(max-height:700px)]:mb-4 max-[601px]:[@media(max-height:600px)]:text-[17px]!">
            Bring your favorite coding agents into one desktop workspace. Keep
            the conversation, even when you change who you’re working with.
          </p>
          <div className="flex flex-wrap justify-center gap-3 max-[601px]:mx-auto max-[601px]:max-w-[290px] max-[601px]:flex-col max-[601px]:items-stretch">
            <DownloadButton />
            <Button
              asChild
              variant="outline"
              className="h-auto min-h-12 leading-[1.428571] gap-[22px] rounded-none px-[23px] py-3 text-[19px] font-normal shadow-none transition-colors duration-200 motion-reduce:transition-none border border-[#83968e] bg-[#fafaf670] text-ink hover:bg-paper"
            >
              <a href="https://github.com/amar-jay/amarcode">
                Visit GitHub{" "}
                <img
                  src="/images/github.svg"
                  alt=""
                  width={16}
                  height={16}
                  className="size-4 shrink-0"
                />
              </a>
            </Button>
          </div>
          <p className="mt-[clamp(12px,2.1svh,21px)] mb-0 text-[16px] max-[601px]:text-[15px] max-[601px]:[@media(max-height:700px)]:hidden">
            Built around the open Agent Client Protocol.
          </p>
        </div>
      </CloudShader>
      <div className="flex items-center justify-between gap-5 border-t border-[#bdcdc4] bg-[#edf2ed] px-10 py-4.25 text-[16px] max-[601px]:p-[15px] max-[601px]:text-[14px] max-[601px]:[&>span:nth-child(2)]:hidden">
        <span>Amarcode / Desktop workspace</span>
        <a
          href="#app-preview"
          aria-label="Scroll to workspace preview"
          className="-m-3 inline-flex p-3 text-ink"
        >
          <ArrowDown size={16} aria-hidden="true" />
        </a>
      </div>
    </section>
  );
}
