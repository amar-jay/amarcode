import { CloudShader } from "@/components/ui/cloud-shader";

export default function CloudShaderDemo() {
  return (
    <section aria-labelledby="hero-title">
      <CloudShader className="hero-texture h-svh min-h-[32rem] bg-[#8cbfe8]">
        <div className="mx-auto max-w-4xl px-6 py-24 text-center text-[#142d46]">
          <img
            src="/images/app/amarcode-logo.svg"
            alt=""
            width={72}
            height={72}
            className="mx-auto mb-7 size-16 sm:size-18"
          />
          <h1
            id="hero-title"
            className="font-heading text-6xl font-light tracking-tight sm:text-8xl md:text-9xl"
          >
            Amarcode
          </h1>
          <p className="mt-6 text-xl sm:text-2xl">Coming soon.</p>
        </div>
      </CloudShader>
    </section>
  );
}
