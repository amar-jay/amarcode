import { Provider, useAtomValue } from "jotai";
import { useEffect, useState } from "react";
import {
  AbsoluteFill,
  Easing,
  Img,
  OffthreadVideo,
  continueRender,
  delayRender,
  interpolate,
  staticFile,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";

import {
  captionsAtom,
  closingAtom,
  featuresAtom,
  titleAtom,
  walkthroughAtom,
} from "./promo-state";

// Tokens and type taken directly from crates/landing/src/index.css.
const C = {
  paper: "#fafaf6",
  ink: "#242c2b",
  muted: "#5b6561",
  rule: "#d5d9d0",
  panel: "#edf2ed",
  amber: "#edbb70",
};

const clamp = {
  extrapolateLeft: "clamp" as const,
  extrapolateRight: "clamp" as const,
};
const ease = Easing.bezier(0.4, 0, 0.2, 1);

const features = [
  {
    title: "Keep the conversation.",
    text: "Your visible chat history lives in Amarcode, independently of an agent's session.",
  },
  {
    title: "Change your perspective.",
    text: "Switch agents within a conversation. The saved text carries forward.",
  },
  {
    title: "Leave your options open.",
    text: "Connect through ACP, the open protocol shared by a growing ecosystem.",
  },
];

function RegistrationCross({
  position,
  size = 9,
}: {
  position: "tl" | "tr" | "bl" | "br";
  size?: number;
}) {
  const style: React.CSSProperties = {
    position: "absolute",
    width: size,
    height: size,
    background: `linear-gradient(${C.rule},${C.rule}),linear-gradient(${C.rule},${C.rule})`,
    backgroundSize: `1px ${size}px, ${size}px 1px`,
    backgroundPosition: "center",
    backgroundRepeat: "no-repeat",
  };
  if (position === "tl") style.top = -5;
  if (position === "bl") style.bottom = -5;
  if (position === "tr") style.top = -5;
  if (position === "br") style.bottom = -5;
  if (position === "tl" || position === "bl") style.left = -5;
  if (position === "tr" || position === "br") style.right = -5;
  return <div style={style} />;
}

function RuleFrame({ children }: { children?: React.ReactNode }) {
  return (
    <div
      style={{
        position: "absolute",
        inset: 40,
        border: `1px solid ${C.rule}`,
        overflow: "hidden",
      }}
    >
      <RegistrationCross position="tl" />
      <RegistrationCross position="tr" />
      <RegistrationCross position="bl" />
      <RegistrationCross position="br" />
      {children}
    </div>
  );
}

function Eyebrow({ children }: { children: React.ReactNode }) {
  return (
    <div
      className="font-roboto"
      style={{
        fontSize: 20,
        letterSpacing: "0.065em",
        textTransform: "uppercase",
        color: C.muted,
        fontWeight: 500,
      }}
    >
      {children}
    </div>
  );
}

function HeroBackdrop() {
  return (
    <AbsoluteFill>
      <Img
        src={staticFile("landing/misty-mountains.webp")}
        style={{
          position: "absolute",
          inset: 0,
          width: "100%",
          height: "100%",
          objectFit: "cover",
          opacity: 0.9,
        }}
      />
      <Img
        src={staticFile("landing/grain.svg")}
        style={{
          position: "absolute",
          inset: 0,
          width: "100%",
          height: "100%",
          objectFit: "cover",
          opacity: 0.3,
          mixBlendMode: "soft-light",
        }}
      />
      <div
        style={{
          position: "absolute",
          inset: 0,
          background:
            "linear-gradient(90deg,transparent calc(25% - 0.5px),#53665f14 25%,transparent calc(25% + 0.5px),transparent calc(75% - 0.5px),#53665f14 75%,transparent calc(75% + 0.5px))",
        }}
      />
    </AbsoluteFill>
  );
}

function TitleBeat({ opacity }: { opacity: number }) {
  const f = useCurrentFrame();
  const enter = interpolate(f, [0, 30], [0, 1], { ...clamp, easing: ease });
  return (
    <AbsoluteFill
      style={{
        opacity: opacity * enter,
        background: C.paper,
        alignItems: "center",
        justifyContent: "center",
        textAlign: "center",
      }}
    >
      <HeroBackdrop />
      <RuleFrame>
        <div
          style={{
            position: "relative",
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            height: "100%",
            padding: "0 120px",
            gap: 28,
          }}
        >
          <Eyebrow>Amarcode / Desktop workspace</Eyebrow>
          <h1
            style={{
              fontSize: 132,
              fontWeight: 300,
              letterSpacing: "-0.055em",
              lineHeight: 0.99,
              margin: 0,
              color: C.ink,
            }}
          >
            Different agents.
            <br />
            One continuous <em style={{ fontWeight: 300 }}>thought.</em>
          </h1>
          <p
            className="font-roboto"
            style={{
              fontSize: 26,
              color: C.muted,
              maxWidth: 640,
              lineHeight: 1.4,
              margin: 0,
            }}
          >
            Bring your favorite coding agents into one desktop workspace. Keep
            the conversation, even when you change who you're working with.
          </p>
          <div
            style={{
              display: "flex",
              gap: 12,
              marginTop: 8,
            }}
          >
            <div
              className="font-roboto"
              style={{
                border: `1px solid #83968e`,
                background: `${C.paper}70`,
                color: C.ink,
                padding: "12px 23px",
                fontSize: 19,
                fontWeight: 500,
              }}
            >
              Download
            </div>
            <div
              className="font-roboto"
              style={{
                border: `1px solid ${C.rule}`,
                background: `${C.paper}70`,
                color: C.ink,
                padding: "12px 23px",
                fontSize: 19,
                fontWeight: 500,
              }}
            >
              Visit GitHub
            </div>
          </div>
          <p
            className="font-roboto"
            style={{
              fontSize: 16,
              color: C.muted,
              margin: 0,
            }}
          >
            Built around the open Agent Client Protocol.
          </p>
        </div>
      </RuleFrame>
    </AbsoluteFill>
  );
}

function FeatureCells({ opacity }: { opacity: number }) {
  const f = useCurrentFrame();
  const sectionEnter = interpolate(f, [240, 270], [0, 1], {
    ...clamp,
    easing: ease,
  });
  return (
    <AbsoluteFill
      style={{
        opacity: opacity * sectionEnter,
        background: C.paper,
        display: "flex",
        flexDirection: "column",
      }}
    >
      <RuleFrame>
        <div
          style={{
            position: "relative",
            display: "flex",
            flexDirection: "column",
            height: "100%",
          }}
        >
          <div
            style={{
              padding: "65px 54px 45px",
              borderBottom: `1px solid ${C.rule}`,
            }}
          >
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 9,
                marginBottom: 22,
              }}
            >
              <Eyebrow>Continuity, by design</Eyebrow>
            </div>
            <h2
              style={{
                fontSize: 76,
                fontWeight: 300,
                letterSpacing: "-0.035em",
                lineHeight: 1.07,
                margin: 0,
                color: C.ink,
              }}
            >
              The work moves forward.
              <br />
              The context stays with you.
            </h2>
          </div>
          <div
            style={{
              flex: 1,
              display: "grid",
              gridTemplateColumns: "1fr 1fr 1fr",
            }}
          >
            {features.map((feature, i) => {
              const cellEnter = interpolate(f, [255 + i * 12, 285 + i * 12], [0, 1], {
                ...clamp,
                easing: ease,
              });
              return (
                <div
                  key={feature.title}
                  style={{
                    padding: "37px 35px 32px",
                    borderLeft: i === 0 ? "none" : `1px solid ${C.rule}`,
                    opacity: cellEnter,
                    transform: `translateY(${(1 - cellEnter) * 12}px)`,
                  }}
                >
                  <div
                    style={{
                      width: 26,
                      height: 26,
                      marginBottom: 34,
                      border: `1px solid ${C.ink}`,
                      borderRadius: 999,
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "center",
                      color: C.ink,
                      fontSize: 14,
                      fontWeight: 500,
                    }}
                  >
                    {i + 1}
                  </div>
                  <h3
                    style={{
                      fontSize: 30,
                      fontWeight: 400,
                      letterSpacing: "-0.025em",
                      margin: "0 0 14px",
                      color: C.ink,
                    }}
                  >
                    {feature.title}
                  </h3>
                  <p
                    style={{
                      fontSize: 21,
                      lineHeight: 1.45,
                      color: C.muted,
                      margin: 0,
                    }}
                  >
                    {feature.text}
                  </p>
                </div>
              );
            })}
          </div>
        </div>
      </RuleFrame>
    </AbsoluteFill>
  );
}

function WalkthroughBeat({ opacity }: { opacity: number }) {
  const f = useCurrentFrame();
  const { width, height } = useVideoConfig();
  const captions = useAtomValue(captionsAtom);
  const scale = interpolate(f, [480, 720, 1080], [1.02, 1.06, 1.02], {
    ...clamp,
    easing: ease,
  });
  const pan = interpolate(f, [480, 720, 1080], [0, -14, 0], {
    ...clamp,
    easing: ease,
  });
  return (
    <AbsoluteFill
      style={{
        opacity,
        background: C.paper,
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <HeroBackdrop />
      <RuleFrame>
        <div
          style={{
            position: "relative",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            height: "100%",
          }}
        >
          <div
            style={{
              position: "relative",
              width: width * 0.66,
              height: height * 0.72,
              border: `1px solid ${C.rule}`,
              background: "#fcfcfc",
              overflow: "hidden",
              boxShadow: "0 30px 80px #242c2b14",
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
            <RegistrationCross position="tl" />
            <RegistrationCross position="tr" />
            <RegistrationCross position="bl" />
            <RegistrationCross position="br" />
          </div>
        </div>
      </RuleFrame>
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
              bottom: 76,
              left: 90,
              maxWidth: 920,
              border: `1px solid ${C.rule}`,
              padding: "14px 20px",
              background: `${C.paper}ee`,
              color: C.ink,
              fontSize: 34,
              lineHeight: 1.25,
              fontWeight: 400,
              letterSpacing: "-0.01em",
              opacity: enter * leave,
              transform: `translateY(${(1 - enter) * 8}px)`,
            }}
          >
            {c.text}
          </div>
        );
      })}
    </AbsoluteFill>
  );
}

function ClosingBeat({ opacity }: { opacity: number }) {
  const f = useCurrentFrame();
  const enter = interpolate(f, [1080, 1110], [0, 1], {
    ...clamp,
    easing: ease,
  });
  return (
    <AbsoluteFill
      style={{
        opacity: opacity * enter,
        background: "#e9efeb",
        alignItems: "center",
        justifyContent: "center",
        textAlign: "center",
      }}
    >
      <HeroBackdrop />
      <RuleFrame>
        <div
          style={{
            position: "relative",
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            height: "100%",
            gap: 18,
          }}
        >
          <Img
            src={staticFile("brand/app-icon.svg")}
            style={{ width: 84, height: 84, borderRadius: 16 }}
          />
          <div
            style={{
              fontSize: 96,
              fontWeight: 400,
              letterSpacing: "-2.4px",
              color: C.ink,
            }}
          >
            amarcode
          </div>
          <div
            className="font-roboto"
            style={{
              fontSize: 24,
              letterSpacing: 0.4,
              color: C.muted,
            }}
          >
            amarcode.amarjay.com
          </div>
        </div>
      </RuleFrame>
    </AbsoluteFill>
  );
}

export function PromoFilm() {
  return (
    <Provider>
      <Film />
    </Provider>
  );
}

function Film() {
  const f = useCurrentFrame();
  const [handle] = useState(() => delayRender("Load typography"));
  useEffect(() => {
    Promise.all([
      document.fonts.load("300 132px Cormorant"),
      document.fonts.load("400 96px Cormorant"),
      document.fonts.load("400 26px Roboto"),
      document.fonts.load("500 20px Roboto"),
    ]).then(() => continueRender(handle));
  }, [handle]);

  const title = useAtomValue(titleAtom);
  const features = useAtomValue(featuresAtom);
  const walkthrough = useAtomValue(walkthroughAtom);
  const closing = useAtomValue(closingAtom);

  // Crossfade beats: hold title until features fade in, etc.
  const activeTitle = f < features.from * 60;
  const activeFeatures = f >= features.from * 60 && f < walkthrough.from * 60;
  const activeWalkthrough =
    f >= walkthrough.from * 60 && f < closing.from * 60;
  const activeClosing = f >= closing.from * 60;

  return (
    <AbsoluteFill
      style={{
        background: C.paper,
        color: C.ink,
        fontFamily: "Cormorant",
        overflow: "hidden",
      }}
    >
      {activeTitle && <TitleBeat opacity={1} />}
      {activeFeatures && <FeatureCells opacity={1} />}
      {activeWalkthrough && <WalkthroughBeat opacity={1} />}
      {activeClosing && <ClosingBeat opacity={1} />}
    </AbsoluteFill>
  );
}
