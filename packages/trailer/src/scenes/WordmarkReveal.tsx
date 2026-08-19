import { useCurrentFrame, useVideoConfig, interpolate, spring, Easing, Img, staticFile } from "remotion";
import { Grain } from "../components/Grain";
import { Eyebrow } from "../components/Eyebrow";

const EASE = Easing.bezier(0.16, 1, 0.3, 1);

export function WordmarkReveal() {
  const frame = useCurrentFrame();
  const { fps, durationInFrames } = useVideoConfig();

  // Eyebrow reveal
  const eyebrowProgress = interpolate(frame, [40, 60], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: EASE,
  });
  const eyebrowY = interpolate(eyebrowProgress, [0, 1], [30, 0]);
  const eyebrowBlur = interpolate(eyebrowProgress, [0, 1], [6, 0]);

  // Fade out for the whole scene
  const fadeOut = interpolate(
    frame,
    [durationInFrames - 14, durationInFrames],
    [1, 0],
    { extrapolateLeft: "clamp", extrapolateRight: "clamp" }
  );

  // Logo transition (moves up and scales down)
  const logoTransition = spring({
    frame,
    fps,
    config: { damping: 15, stiffness: 80 },
  });
  const logoY = interpolate(logoTransition, [0, 1], [0, -800]);
  const logoScale = interpolate(logoTransition, [0, 1], [1, 0.55]);

  const letters = "Vivianne".split("");

  // Horizontal marquee offsets
  const marquee1X = (frame * -4) % 1800;
  const marquee2X = (frame * 4) % 1800 - 1800;

  const marqueeText1 = " VIVIANNE AUTONOMOUS AGENT PRECISION SYSTEM SHIPPING CODE INTEGRITY";
  const marqueeText2 = " READ WRITE EDIT RUN COMMANDS PERSISTENT CONTEXT SESSION TO SESSION";

  return (
    <div
      style={{
        width: "100%",
        height: "100%",
        background: "var(--canvas)",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "space-between",
        opacity: fadeOut,
        position: "relative",
        overflow: "hidden",
        padding: "160px 0",
      }}
    >
      {/* Background Developer Grid */}
      <div
        style={{
          position: "absolute",
          inset: 0,
          backgroundImage: "linear-gradient(rgba(17,17,17,0.02) 1px, transparent 1px), linear-gradient(90deg, rgba(17,17,17,0.02) 1px, transparent 1px)",
          backgroundSize: "80px 80px",
          backgroundPosition: "center center",
          zIndex: 0,
        }}
      />

      {/* Top Marquee */}
      <div
        style={{
          width: "100%",
          overflow: "hidden",
          zIndex: 1,
          display: "flex",
        }}
      >
        <div
          style={{
            transform: `translateX(${marquee1X}px)`,
            whiteSpace: "nowrap",
            fontFamily: "Sora",
            fontWeight: 500,
            fontSize: 72,
            letterSpacing: "0.15em",
            textTransform: "uppercase",
            color: "rgba(17, 17, 17, 0.05)",
          }}
        >
          {marqueeText1 + marqueeText1}
        </div>
      </div>

      {/* Interactive logo transition panel */}
      <div
        style={{
          position: "absolute",
          top: "40%",
          left: 0,
          right: 0,
          transform: `translateY(${logoY}px) scale(${logoScale})`,
          display: "flex",
          justifyContent: "center",
          alignItems: "center",
          zIndex: 2,
          transformOrigin: "center center",
        }}
      >
        <Img
          src={staticFile("vivianne-logo.svg")}
          style={{
            width: 1300,
            height: 1300,
            filter: "drop-shadow(0 30px 70px rgba(79, 143, 224, 0.25))",
          }}
        />
      </div>

      {/* Wordmark (Staggered Letters) */}
      <div
        style={{
          display: "flex",
          justifyContent: "center",
          alignItems: "center",
          gap: 2,
          zIndex: 3,
          position: "absolute",
          top: "48%",
        }}
      >
        {letters.map((char, i) => {
          const lSpring = spring({
            frame: frame - 15 - i * 3, // staggered by 3 frames per letter
            fps,
            config: { damping: 12, stiffness: 130, mass: 1 },
          });
          const lY = interpolate(lSpring, [0, 1], [120, 0]);
          const lBlur = interpolate(lSpring, [0, 1], [12, 0]);
          const lScale = interpolate(lSpring, [0, 1], [0.4, 1]);
          const lOp = interpolate(lSpring, [0, 1], [0, 1]);

          return (
            <span
              key={i}
              style={{
                display: "inline-block",
                fontFamily: "Sora",
                fontWeight: 500,
                fontSize: 200,
                letterSpacing: "-0.03em",
                color: "var(--ink)",
                lineHeight: 1.02,
                opacity: lOp,
                filter: `blur(${lBlur}px)`,
                transform: `translateY(${lY}px) scale(${lScale})`,
                transformOrigin: "center bottom",
              }}
            >
              {char}
            </span>
          );
        })}
      </div>

      {/* Eyebrow / Tagline at bottom */}
      <div
        style={{
          opacity: eyebrowProgress,
          transform: `translateY(${eyebrowY}px)`,
          filter: `blur(${eyebrowBlur}px)`,
          zIndex: 3,
          position: "absolute",
          bottom: "320px",
        }}
      >
        <Eyebrow style={{ display: "block", fontSize: 54, letterSpacing: "0.22em" }}>
          Autonomous Coding Agent
        </Eyebrow>
      </div>

      {/* Bottom Marquee */}
      <div
        style={{
          width: "100%",
          overflow: "hidden",
          zIndex: 1,
          display: "flex",
        }}
      >
        <div
          style={{
            transform: `translateX(${marquee2X}px)`,
            whiteSpace: "nowrap",
            fontFamily: "Sora",
            fontWeight: 500,
            fontSize: 72,
            letterSpacing: "0.15em",
            textTransform: "uppercase",
            color: "rgba(17, 17, 17, 0.05)",
          }}
        >
          {marqueeText2 + marqueeText2}
        </div>
      </div>

      <Grain />
    </div>
  );
}
