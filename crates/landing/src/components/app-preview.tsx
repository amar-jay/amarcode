import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

const screenshots = [
  { theme: "light", label: "Light", src: "/images/app/amarcode-light.png" },
  { theme: "dark", label: "Dark", src: "/images/app/amarcode-dark.png" },
];

export default function AppPreview() {
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
      <Tabs
        defaultValue="light"
        className="mx-[54px] gap-0 border border-rule max-[901px]:mx-7 max-[601px]:mx-3"
      >
        <div className="flex items-center justify-between border-b border-rule bg-panel px-[18px] py-3 max-[601px]:gap-2.5 max-[601px]:p-2.5">
          <span className="inline-flex items-center gap-[9px] text-[17px] max-[601px]:gap-1.5 max-[601px]:text-[14px]">
            <span className="inline-block size-[5px] shrink-0 bg-current" />{" "}
            Amarcode in focus
          </span>
          <TabsList
            aria-label="Application screenshot theme"
            className="h-8 rounded-none border border-[#b7c0b6] bg-transparent p-0"
          >
            {screenshots.map(({ theme, label }) => (
              <TabsTrigger
                key={theme}
                value={theme}
                className="rounded-none px-[19px] py-0.5 text-[17px] font-normal data-[state=active]:bg-ink data-[state=active]:text-paper max-[601px]:px-3 max-[601px]:text-[16px]"
              >
                {label}
              </TabsTrigger>
            ))}
          </TabsList>
        </div>
        {screenshots.map(({ theme, src }) => (
          <TabsContent
            key={theme}
            value={theme}
            className="[&_figure]:m-0 [&_img]:block [&_img]:h-auto [&_img]:w-full"
          >
            <figure>
              <img
                src={src}
                alt={`Amarcode in ${theme} mode showing a coding conversation, code blocks, and the message composer.`}
                width={1853}
                height={1048}
                loading="lazy"
                decoding="async"
              />
            </figure>
          </TabsContent>
        ))}
      </Tabs>
      <div className="flex justify-between px-[54px] pt-[17px] pb-[37px] text-[16px] text-muted-ink max-[901px]:px-7 max-[601px]:gap-[15px] max-[601px]:px-[15px] max-[601px]:pt-[15px] max-[601px]:pb-[25px] max-[601px]:text-[13px]">
        <span>A real workspace. A real conversation.</span>
        <span>Light & dark themes</span>
      </div>
    </section>
  );
}
