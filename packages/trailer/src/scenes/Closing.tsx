import { Img, staticFile, useCurrentFrame, useVideoConfig, interpolate, spring, Easing } from "remotion";
import { Grain } from "../components/Grain";
import { useBeatPulse } from "../hooks/useBeatPulse";

const EASE = Easing.bezier(0.16, 1, 0.3, 1);

export function Closing() {
  const frame = useCurrentFrame();
  const { fps, durationInFrames } = useVideoConfig();
  const pulse = useBeatPulse(6, 1);

  // Logo entrance scale & opacity
  const logoEntrance = spring({
    frame,
    fps,
    config: { damping: 14, stiffness: 60, mass: 1.4 },
  });
  const logoScale = interpolate(logoEntrance, [0, 1], [0.4, 1]);
  const logoOpacity = interpolate(frame, [0, 14], [0, 1], { extrapolateRight: "clamp" });

  // Revolving 4-point star particles configurations (7 stars, enlarged sizes & wider orbits)
  const particles = [
    { angle: 0, radius: 370, size: 48, speed: 0.7 },
    { angle: 51, radius: 310, size: 34, speed: -1.0 },
    { angle: 102, radius: 390, size: 44, speed: 0.8 },
    { angle: 153, radius: 320, size: 38, speed: -0.6 },
    { angle: 204, radius: 355, size: 42, speed: 0.9 },
    { angle: 255, radius: 330, size: 36, speed: -0.8 },
    { angle: 306, radius: 380, size: 46, speed: 0.5 },
  ];

  // Staggered letters for "Vivianne" (starts at Beat 24 / local Frame 17)
  const letters = "Vivianne".split("");

  // Tagline reveal (starts at Beat 25 / local Frame 46)
  const taglineP = interpolate(frame, [46, 66], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: EASE,
  });

  // Domain name reveal (starts at Beat 27 / local Frame 86)
  const domainP = interpolate(frame, [86, 106], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: EASE,
  });

  // Soft overall camera zoom
  const cameraScale = interpolate(frame, [0, durationInFrames], [1, 1.04], {
    easing: Easing.out(Easing.quad),
  });

  // Global exit fade out (last 14 frames)
  const fadeOut = interpolate(
    frame,
    [durationInFrames - 14, durationInFrames],
    [1, 0],
    { extrapolateLeft: "clamp", extrapolateRight: "clamp" }
  );

  return (
    <div
      style={{
        width: 2160,
        height: 3840,
        background: "var(--canvas)",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center", // Perfectly centers the combined outro block
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
          backgroundImage: "linear-gradient(rgba(28,28,28,0.035) 1px, transparent 1px), linear-gradient(90deg, rgba(28,28,28,0.035) 1px, transparent 1px)",
          backgroundSize: "80px 80px",
          backgroundPosition: "center center",
          transform: `scale(${cameraScale})`,
          zIndex: 0,
        }}
      />

      {/* Unified Centered Outro Container */}
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          gap: 60, // balanced gap between logo and text
          zIndex: 2,
          transform: `scale(${cameraScale})`,
        }}
      >
        {/* UPPER SECTION: Animated Logo & Orbiting Rings */}
        <div
          style={{
            width: 900,
            height: 900,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            position: "relative",
          }}
        >
          {/* Radial Ambient Glow */}
          <div
            style={{
              position: "absolute",
              width: 1000,
              height: 1000,
              borderRadius: "50%",
              background: "radial-gradient(circle, rgba(59,130,246,0.06) 0%, rgba(59,130,246,0.01) 50%, rgba(255,255,255,0) 70%)",
              opacity: 0.3 + pulse * 0.35,
            }}
          />

          {/* Orbiting 4-point blue stars */}
          {particles.map((p, idx) => {
            const currentAngle = p.angle + frame * p.speed * 1.2;
            const rad = (currentAngle * Math.PI) / 180;
            const x = Math.cos(rad) * p.radius;
            const y = Math.sin(rad) * p.radius;

            // Subtle breathing scale on the stars
            const starPulse = 1 + Math.sin((frame * 0.08) + idx) * 0.15;

            return (
              <div
                key={idx}
                style={{
                  position: "absolute",
                  left: 450 + x - p.size / 2,
                  top: 450 + y - p.size / 2,
                  width: p.size,
                  height: p.size,
                  opacity: logoOpacity * 0.85,
                  transform: `scale(${starPulse}) rotate(${frame * p.speed * 2}deg)`,
                  filter: "drop-shadow(0 0 12px rgba(59, 130, 246, 0.7))",
                  zIndex: 1,
                }}
              >
                <svg width="100%" height="100%" viewBox="0 0 24 24" fill="none">
                  <path
                    d="M12,0 C12,6.6 6.6,12 0,12 C6.6,12 12,17.4 12,24 C12,17.4 17.4,12 24,12 C17.4,12 12,6.6 12,0 Z"
                    fill="#3b82f6"
                  />
                </svg>
              </div>
            );
          })}

          {/* Scaled Logo (Enlarged to 580px) */}
          <Img
            src={staticFile("vivianne-logo.svg")}
            style={{
              width: 580,
              height: 580,
              objectFit: "contain",
              opacity: logoOpacity,
              transform: `scale(${logoScale * (1 + pulse * 0.02)})`,
              filter: "drop-shadow(0 25px 65px rgba(28, 28, 28, 0.06))",
              zIndex: 2,
            }}
          />
        </div>

        {/* LOWER SECTION: Typographic Brand Reveal */}
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
          }}
        >
          {/* Wordmark (Staggered letter reveal) */}
          <div style={{ display: "flex", justifyContent: "center", alignItems: "center", gap: 3 }}>
            {letters.map((char, i) => {
              const letterSpring = spring({
                frame: frame - 17 - (i * 3), // starts at Frame 17, staggered by 3 frames
                fps,
                config: { damping: 14, stiffness: 120, mass: 1 },
              });
              const lY = interpolate(letterSpring, [0, 1], [40, 0]);
              const lScale = interpolate(letterSpring, [0, 1], [0.75, 1]);
              const lOp = interpolate(letterSpring, [0, 1], [0, 1]);
              const lBlur = interpolate(letterSpring, [0, 1], [6, 0]);

              return (
                <span
                  key={i}
                  style={{
                    display: "inline-block",
                    fontFamily: "Sora",
                    fontWeight: 500,
                    fontSize: 140,
                    letterSpacing: "-0.03em",
                    color: "var(--ink)",
                    lineHeight: 1.05,
                    opacity: lOp,
                    filter: `blur(${lBlur}px)`,
                    transform: `translateY(${lY}px) scale(${letterScale(lScale)})`,
                    transformOrigin: "center bottom",
                  }}
                >
                  {char}
                </span>
              );
            })}
          </div>

          {/* Tagline */}
          <div
            style={{
              opacity: taglineP,
              transform: `translateY(${interpolate(taglineP, [0, 1], [20, 0])}px)`,
              marginTop: 40,
            }}
          >
            <p
              style={{
                fontFamily: "Sora",
                fontWeight: 400,
                fontSize: 48,
                color: "var(--body)",
                margin: 0,
                textAlign: "center",
                letterSpacing: "-0.01em",
              }}
            >
              The coding harness that follows through.
            </p>
          </div>

          {/* Domain name Link */}
          <div
            style={{
              opacity: domainP,
              transform: `translateY(${interpolate(domainP, [0, 1], [15, 0])}px)`,
              marginTop: 80,
            }}
          >
            <p
              style={{
                fontFamily: "Sora",
                fontWeight: 500,
                fontSize: 40,
                letterSpacing: "0.04em",
                color: "var(--mute)",
                margin: 0,
                textAlign: "center",
              }}
            >
              vivianne-site.vercel.app
            </p>
          </div>
        </div>
      </div>

      <Grain />
    </div>
  );
}

function letterScale(scale: number) {
  return isNaN(scale) ? 0 : scale;
}
