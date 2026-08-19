import { useCurrentFrame, useVideoConfig, interpolate, spring, Easing, Img, staticFile } from "remotion";
import { Grain } from "../components/Grain";

const EASE = Easing.bezier(0.16, 1, 0.3, 1);

export function QuietOpen() {
  const frame = useCurrentFrame();
  const { durationInFrames, fps } = useVideoConfig();

  // Slow, smooth camera zoom-in
  const cameraScale = interpolate(frame, [0, durationInFrames], [0.96, 1.04], {
    easing: Easing.out(Easing.quad),
  });

  // Fade out for the whole scene
  const fadeOut = interpolate(
    frame,
    [durationInFrames - 12, durationInFrames],
    [1, 0],
    { extrapolateLeft: "clamp", extrapolateRight: "clamp" }
  );

  // Logo Spring Entrance (Snappy spring, slight overshoot)
  const logoSpring = spring({
    frame,
    fps,
    config: { damping: 11, stiffness: 95, mass: 1 },
  });
  const logoScale = interpolate(logoSpring, [0, 1], [0, 1]);
  const logoRot = interpolate(logoSpring, [0, 1], [-180, 0]);

  // Horizontal marquee offsets
  const marquee1X = (frame * -4) % 1800;
  const marquee2X = (frame * 4) % 1800 - 1800;

  // Background letter V animation
  const vScale = interpolate(frame, [0, durationInFrames], [0.8, 1.15], {
    easing: EASE,
  });
  const vOpacity = interpolate(frame, [0, 30], [0, 0.05], {
    extrapolateRight: "clamp",
  });

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
          transform: `scale(${cameraScale})`,
          zIndex: 0,
        }}
      />



      {/* Top Marquee */}
      <div
        style={{
          width: "100%",
          overflow: "hidden",
          zIndex: 2,
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

      {/* Center SVG Logo */}
      <div
        style={{
          transform: `scale(${logoScale * cameraScale}) rotate(${logoRot}deg)`,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          zIndex: 3,
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

      {/* Bottom Marquee */}
      <div
        style={{
          width: "100%",
          overflow: "hidden",
          zIndex: 2,
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
