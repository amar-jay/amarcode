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
const captions = [
  { from: 0.4, to: 2.6, text: "Several agents. The subscriptions you already use." },
  { from: 3.2, to: 6.2, text: "Start the work with any of them." },
  { from: 6.6, to: 8.8, text: "Git history stays in the conversation." },
  { from: 9.0, to: 12.0, text: "Switch agents in the middle of the chat." },
  { from: 12.4, to: 15.4, text: "The next agent already has the context." },
  { from: 16.0, to: 22.6, text: "Review the files and the diff beside the chat." },
  { from: 23.2, to: 26.0, text: "Leave, and the same conversation is still here." },
];

export function LaunchFilm() {
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
  const splash = interpolate(f, [26.3 * 60, 27.1 * 60], [0, 1], {
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
              style={{
                position: "absolute",
                left: 36,
                bottom: 32,
                maxWidth: 920,
                padding: "14px 22px",
                background: "#fafaf6ee",
                border: `1px solid ${C.rule}`,
                color: C.ink,
                fontSize: 34,
                fontWeight: 400,
                letterSpacing: -0.6,
                lineHeight: 1.15,
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
