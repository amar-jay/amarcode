import { Provider, useAtomValue } from "jotai";
import { useEffect, useState } from "react";
import {
  AbsoluteFill,
  Audio,
  Easing,
  Img,
  OffthreadVideo,
  continueRender,
  delayRender,
  interpolate,
  staticFile,
  useCurrentFrame,
} from "remotion";
import { captionsAtom, splashAtAtom } from "./film-state";

// Tokens and type taken directly from crates/landing/src/index.css.
const C = {
  paper: "#fafaf6",
  ink: "#242c2b",
  muted: "#5b6561",
  rule: "#d5d9d0",
  panel: "#edf2ed",
};
const clamp = {
  extrapolateLeft: "clamp" as const,
  extrapolateRight: "clamp" as const,
};
const ease = Easing.bezier(0.4, 0, 0.2, 1);
const times = [0, 3, 8, 12, 16, 22, 26.2, 30].map((t) => t * 60);
const zooms = [1.02, 1.06, 1.08, 1.05, 1.04, 1.08, 1.02, 1.02];
const pans = [0, -20, -10, 0, 0, 16, 0, 0];
export function LaunchFilm() {
  return (
    <Provider>
      <Film />
    </Provider>
  );
}

function Film() {
  const f = useCurrentFrame();
  const [handle] = useState(() => delayRender("Load landing-page typography"));
  useEffect(() => {
    Promise.all([
      document.fonts.load("300 48px Cormorant"),
      document.fonts.load("400 40px Cormorant"),
      document.fonts.load("400 20px Roboto"),
    ]).then(() => continueRender(handle));
  }, [handle]);
  const scale = interpolate(f, times, zooms, { ...clamp, easing: ease });
  const pan = interpolate(f, times, pans, { ...clamp, easing: ease });
  const captions = useAtomValue(captionsAtom);
  const splashAt = useAtomValue(splashAtAtom);
  const splash = interpolate(f, [splashAt * 60, (splashAt + 0.8) * 60], [0, 1], {
    ...clamp,
    easing: ease,
  });
  return (
    <AbsoluteFill
      style={{
        background: C.paper,
        color: C.ink,
        fontFamily: "Cormorant",
        overflow: "hidden",
      }}
    >
      <Audio src={staticFile("audio/score.wav")} volume={0.22} />
      {/* One uninterrupted real app recording stays on screen for all 1,800 frames. */}
      <div
        style={{
          position: "absolute",
          inset: 0,
          opacity: 1 - splash,
          overflow: "hidden",
          background: "#fcfcfc",
        }}
      >
        <div
          style={{
            position: "absolute",
            inset: 0,
            transform: `translateY(${pan}px) scale(${scale})`,
            transformOrigin: "50% 42%",
          }}
        >
          <OffthreadVideo
            muted
            src={staticFile("walkthrough/app-walkthrough.mp4")}
            style={{ width: "100%", height: "100%", display: "block" }}
          />
        </div>
        {captions.map((c) => {
          const at = c.from * 60;
          const end = c.to * 60;
          if (f < at || f >= end) return null;
          const enter = interpolate(f, [at, at + 12], [0, 1], {
            ...clamp,
            easing: ease,
          });
          const leave = interpolate(f, [end - 10, end], [1, 0], clamp);
          return (
            <div
              key={c.from}
              className="absolute bottom-8 left-9 max-w-[920px] border px-5 py-3.5 text-[34px] leading-tight font-normal tracking-tight"
              style={{
                background: "#fafaf6ee",
                borderColor: C.rule,
                color: C.ink,
                opacity: enter * leave,
                transform: `translateY(${(1 - enter) * 8}px)`,
              }}
            >
              {c.text}
            </div>
          );
        })}
      </div>
      <div
        style={{
          position: "absolute",
          inset: 0,
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          gap: 28,
          background: C.paper,
          opacity: splash,
        }}
      >
        <Img
          src={staticFile("brand/app-icon.svg")}
          style={{ width: 96, height: 96, borderRadius: 16 }}
        />
        <div style={{ fontSize: 92, fontWeight: 400, letterSpacing: -2.4 }}>
          amarcode
        </div>
        <div
          style={{
            fontFamily: "Roboto",
            fontSize: 28,
            letterSpacing: 0.4,
            color: C.muted,
          }}
        >
          amarcode.amarjay.com
        </div>
      </div>
    </AbsoluteFill>
  );
}
