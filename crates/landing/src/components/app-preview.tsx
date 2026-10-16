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
      className="bg-[#f5f8fb] px-4 py-16 text-[#142d46] sm:px-8 sm:py-24"
    >
      <div className="mx-auto max-w-7xl">
        <div className="mb-9 text-center">
          <h2 id="preview-title" className="text-4xl font-light sm:text-5xl">
            A look inside Amarcode.
          </h2>
          <p className="mt-4 text-xl">Your workspace, in light and dark.</p>
        </div>
        <Tabs defaultValue="light" className="gap-6">
          <TabsList
            aria-label="Application screenshot theme"
            className="mx-auto"
          >
            {screenshots.map(({ theme, label }) => (
              <TabsTrigger
                key={theme}
                value={theme}
                className="px-5 text-lg font-normal"
              >
                {label}
              </TabsTrigger>
            ))}
          </TabsList>
          {screenshots.map(({ theme, src }) => (
            <TabsContent key={theme} value={theme}>
              <figure className="overflow-hidden rounded-xl border border-[#142d46]/15 shadow-xl shadow-[#142d46]/10">
                <img
                  src={src}
                  alt={`Amarcode desktop application in ${theme} mode, showing the chat sidebar, an agent conversation, code blocks, and the message composer.`}
                  width={1853}
                  height={1048}
                  loading="lazy"
                  decoding="async"
                  className="h-auto w-full"
                />
              </figure>
            </TabsContent>
          ))}
        </Tabs>
      </div>
    </section>
  );
}
