/**
 * Soft logo watermark for the empty home / new-chat screen.
 * Decorative only — sits behind the prompt and never captures pointer events.
 *
 * Opacity reacts to the sibling prompt shell via the parent
 * `[data-home-stage]:has([data-prompt-shell]:…)` rules in index.css.
 */
export function HomeWatermark() {
  return (
    <div
      aria-hidden
      className="pointer-events-none absolute inset-0 overflow-hidden"
    >
      {/* Ambient glow behind the mark */}
      <div
        data-watermark-glow
        className="absolute inset-0 bg-[radial-gradient(ellipse_80%_60%_at_50%_45%,color-mix(in_oklab,var(--primary)_14%,transparent),transparent_70%)]"
      />
      <div
        data-watermark-glow
        className="absolute inset-0 bg-[radial-gradient(circle_at_50%_55%,color-mix(in_oklab,#f3ba63_10%,transparent),transparent_55%)] dark:bg-[radial-gradient(circle_at_50%_55%,color-mix(in_oklab,#f3ba63_8%,transparent),transparent_50%)]"
      />

      {/* Logo mark — large, muted, gradient-faded toward edges */}
      <div className="absolute left-1/2 top-[42%] w-[min(78vw,26rem)] -translate-x-1/2 -translate-y-1/2">
        <div
          data-watermark-mark
          style={{
            maskImage:
              "radial-gradient(ellipse 70% 70% at 50% 50%, black 20%, transparent 75%)",
            WebkitMaskImage:
              "radial-gradient(ellipse 70% 70% at 50% 50%, black 20%, transparent 75%)",
          }}
        >
          <svg
            viewBox="0 0 512 512"
            className="h-auto w-full text-foreground"
            fill="currentColor"
          >
            {/* Mark only (no solid app-icon plate) so it reads as a watermark */}
            <path d="M112 132 254 246a13 13 0 0 1 0 20L112 380l-31-39 118-85L81 171l31-39Z" />
            <rect x="280" y="337" width="151" height="38" rx="19" />
          </svg>
        </div>
      </div>

      {/* Soft vertical wash so the input sits on a calm field */}
      <div className="absolute inset-0 bg-linear-to-b from-background/80 via-transparent to-background/90" />
    </div>
  );
}
