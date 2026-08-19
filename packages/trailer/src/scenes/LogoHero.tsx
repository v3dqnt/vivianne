import { Img, staticFile, useCurrentFrame, useVideoConfig, interpolate, spring, Easing } from "remotion";
import { Grain } from "../components/Grain";
import { useBeatPulse } from "../hooks/useBeatPulse";
import { SignalField } from "../components/SignalField";

/**
 * Scene 7 — the hero moment. The real logo asset scales in at center: this
 * is the one place the brand's sky-blue-to-brushed-silver ring gets to be
 * the entire story, "earned rather than decorative" per the design system.
 * A soft ambient glow breathes in behind it, pulsing gently on the beat.
 */
export function LogoHero() {
  const frame = useCurrentFrame();
  const { fps, durationInFrames } = useVideoConfig();
  const pulse = useBeatPulse(6, 1);

  const scaleSpring = spring({
    frame,
    fps,
    config: { damping: 14, stiffness: 60, mass: 1.4 },
  });
  const scale = interpolate(scaleSpring, [0, 1], [0.4, 1]);
  const opacity = interpolate(frame, [0, 14], [0, 1], { extrapolateRight: "clamp" });

  const fadeOut = interpolate(
    frame,
    [durationInFrames - 14, durationInFrames],
    [1, 0],
    { extrapolateLeft: "clamp", extrapolateRight: "clamp" }
  );

  const glowScale = 1.05 + pulse * 0.12;
  const glowOpacity = 0.3 + pulse * 0.35;

  // Orbit rotation values (mograph style)
  const orbit1Rot = frame * 0.4;
  const orbit2Rot = -frame * 0.2;

  // Smooth camera zoom
  const cameraScale = interpolate(frame, [0, durationInFrames], [1, 1.04], {
    easing: Easing.out(Easing.quad),
  });

  return (
    <div
      style={{
        width: "100%",
        height: "100%",
        background: "var(--canvas)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        opacity: fadeOut,
        position: "relative",
        overflow: "hidden",
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

      {/* Pulsing Ambient Glow (Monochrome) */}
      <div
        style={{
          position: "absolute",
          width: 2200,
          height: 2200,
          borderRadius: "50%",
          background:
            "radial-gradient(circle, rgba(17,17,17,0.04) 0%, rgba(17,17,17,0.01) 45%, rgba(255,255,255,0) 72%)",
          transform: `scale(${glowScale * cameraScale})`,
          opacity: glowOpacity,
          zIndex: 1,
        }}
      />

      {/* Orbiting Tech Ring 1 (Dashed Outer - Monochrome) */}
      <svg
        width={1700}
        height={1700}
        viewBox="0 0 1700 1700"
        style={{
          position: "absolute",
          transform: `rotate(${orbit1Rot}deg) scale(${cameraScale})`,
          opacity: 0.25 * opacity,
          zIndex: 2,
        }}
      >
        <circle
          cx={850}
          cy={850}
          r={620}
          fill="none"
          stroke="var(--ink)"
          strokeWidth={2}
          strokeDasharray="36 24 12 24"
        />
      </svg>

      {/* Orbiting Tech Ring 2 (Dashed Inner - Monochrome) */}
      <svg
        width={1700}
        height={1700}
        viewBox="0 0 1700 1700"
        style={{
          position: "absolute",
          transform: `rotate(${orbit2Rot}deg) scale(${cameraScale})`,
          opacity: 0.18 * opacity,
          zIndex: 2,
        }}
      >
        <circle
          cx={850}
          cy={850}
          r={480}
          fill="none"
          stroke="var(--ink)"
          strokeWidth={1.5}
          strokeDasharray="80 30"
        />
        <circle
          cx={850}
          cy={850}
          r={460}
          fill="none"
          stroke="var(--ink)"
          strokeWidth={1}
          strokeDasharray="6 12"
        />
      </svg>

      {/* Central Logo - Swapped with SVG and Enlarged */}
      <Img
        src={staticFile("vivianne-logo.svg")}
        style={{
          width: 900,
          height: 900,
          objectFit: "contain",
          opacity,
          transform: `scale(${scale * cameraScale * (1 + pulse * 0.02)})`,
          zIndex: 3,
          filter: "drop-shadow(0 25px 65px rgba(17, 17, 17, 0.08))",
        }}
      />
      <div
        style={{
          position: "absolute",
          bottom: 750,
          color: "var(--ink-strong)",
          fontFamily: "Sora",
          fontWeight: 500,
          fontSize: 48,
          letterSpacing: "0.24em",
          opacity,
          zIndex: 4,
        }}
      >
        VIVIANNE / AGENTIC HARNESS
      </div>
      <Grain />
    </div>
  );
}
