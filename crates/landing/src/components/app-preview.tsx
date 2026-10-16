import { useState, type CSSProperties } from "react";
import { MoveHorizontal } from "lucide-react";

export default function AppPreview() {
  const [reveal, setReveal] = useState(50);
  return (
    <section
      id="app-preview"
      aria-labelledby="preview-title"
      className="relative border-t border-rule before:pointer-events-none before:absolute before:-top-[5px] before:-left-[5px] before:z-20 before:size-[9px] before:bg-[linear-gradient(var(--color-rule),var(--color-rule)),linear-gradient(var(--color-rule),var(--color-rule))] before:bg-[size:1px_9px,9px_1px] before:bg-center before:bg-no-repeat before:content-['']"
    >
      <div className="grid grid-cols-[1fr_2fr] gap-[30px] px-[54px] pt-[65px] pb-[45px] [&>p]:mt-[13px] [&>div>p]:mb-0 [&>div>p]:max-w-[600px] [&>div>p]:text-[22px] [&>div>p]:leading-[1.4] [&>div>p]:text-muted-ink max-[901px]:grid-cols-1 max-[901px]:gap-1.5 max-[901px]:px-7 max-[901px]:pt-[45px] max-[901px]:pb-8 max-[901px]:[&>p]:mt-0 max-[901px]:[&>p]:mb-2 max-[601px]:px-5 max-[601px]:pt-9 max-[601px]:pb-7 max-[601px]:[&>div>p]:text-[20px]">
        <p className="mb-[22px] flex items-center gap-[9px] text-[16px] tracking-[0.065em] uppercase max-[601px]:text-[13px]">
          The workspace
        </p>
        <div>
          <h2
            className="mb-[18px] text-[clamp(38px,4.4vw,60px)] leading-[1.07] font-light tracking-[-0.035em]"
            id="preview-title"
          >
            Room to do your best work.
          </h2>
          <p>
            A familiar place for your conversations, code, and the agents you
            choose.
          </p>
        </div>
      </div>
      <div className="mx-[54px] border border-rule max-[901px]:mx-7 max-[601px]:mx-3">
        <figure
          className="relative m-0 isolate select-none has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-offset-4 has-[:focus-visible]:outline-[#80602a]"
          style={{ "--reveal": `${reveal}%` } as CSSProperties}
        >
          <img
            src="/images/app/amarcode-light.png"
            alt="Amarcode in light mode, showing a coding conversation and message composer."
            width={1853}
            height={1048}
            loading="lazy"
            decoding="async"
            className="block h-auto w-full"
          />
          <img
            src="/images/app/amarcode-dark.png"
            alt="The same Amarcode workspace in dark mode."
            width={1853}
            height={1048}
            loading="lazy"
            decoding="async"
            className="pointer-events-none absolute inset-0 size-full [clip-path:inset(0_calc(100%_-_var(--reveal))_0_0)]"
          />
          <div
            aria-hidden="true"
            className="pointer-events-none absolute inset-x-0 bottom-3 flex justify-between px-3 text-[14px] max-[601px]:bottom-2 max-[601px]:px-2 max-[601px]:text-[12px]"
          >
            <span className="border border-white/20 bg-ink px-2 py-0.5 text-paper">
              Dark
            </span>
            <span className="border border-rule bg-paper px-2 py-0.5 text-ink">
              Light
            </span>
          </div>
          <div
            aria-hidden="true"
            className="pointer-events-none absolute inset-y-0 left-(--reveal) w-0.5 -translate-x-1/2 bg-paper shadow-[0_0_0_1px_#242c2b40]"
          >
            <span className="absolute top-1/2 left-1/2 flex size-11 -translate-1/2 items-center justify-center border border-rule bg-paper text-ink shadow-md max-[601px]:size-9">
              <MoveHorizontal size={21} />
            </span>
          </div>
          <input
            type="range"
            min={0}
            max={100}
            value={reveal}
            onChange={(event) => setReveal(Number(event.target.value))}
            aria-label="Compare light and dark themes"
            aria-describedby="preview-slider-hint"
            aria-valuetext={`${reveal}% dark theme, ${100 - reveal}% light theme`}
            className="absolute inset-0 m-0 size-full cursor-ew-resize touch-pan-y appearance-none opacity-0 [&::-webkit-slider-thumb]:h-11 [&::-webkit-slider-thumb]:w-1 [&::-webkit-slider-thumb]:appearance-none [&::-moz-range-thumb]:h-11 [&::-moz-range-thumb]:w-1 [&::-moz-range-thumb]:border-0"
          />
        </figure>
      </div>
      <div className="py-3">
      </div>
    </section>
  );
}
