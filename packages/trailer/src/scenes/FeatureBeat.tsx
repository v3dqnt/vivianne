import { useCurrentFrame, interpolate, Easing, useVideoConfig } from "remotion";
import type { Icon } from "@phosphor-icons/react";
import { Grain } from "../components/Grain";
import { useBeatPulse } from "../hooks/useBeatPulse";

const EASE = Easing.bezier(0.16, 1, 0.3, 1);

interface FeatureBeatProps {
  index: string;
  IconComponent: Icon;
  heading: string;
  description: string;
  MockupComponent: React.ComponentType;
}

export function FeatureBeat({ index, IconComponent, heading, description, MockupComponent }: FeatureBeatProps) {
  const frame = useCurrentFrame();
  const { durationInFrames } = useVideoConfig();
  const pulse = useBeatPulse(6, 1);

  // Staggered entrances
  const inProgress = interpolate(frame, [0, 16], [0, 1], {
    extrapolateRight: "clamp",
    easing: EASE,
  });
  
  const mockupY = interpolate(inProgress, [0, 1], [60, 0]);
  const textY = interpolate(inProgress, [0, 1], [100, 0]);
  const blur = interpolate(inProgress, [0, 1], [10, 0]);

  const fadeOut = interpolate(
    frame,
    [durationInFrames - 12, durationInFrames],
    [1, 0],
    { extrapolateLeft: "clamp", extrapolateRight: "clamp" }
  );

  // Soft grid camera zoom
  const cameraScale = interpolate(frame, [0, durationInFrames], [1, 1.04], {
    easing: Easing.out(Easing.quad),
  });

  return (
    <div
      style={{
        width: 2160,
        height: 3840,
        display: "flex",
        flexDirection: "column",
        opacity: fadeOut,
        position: "relative",
        overflow: "hidden",
      }}
    >
      {/* Top Panel: GUI Mockup Viewport */}
      <div
        style={{
          width: 2160,
          height: 2100,
          background: "var(--canvas)", // warm cream canvas background
          display: "flex",
          alignItems: "flex-end",
          justifyContent: "center",
          paddingBottom: 80, // tight gap from divider
          borderBottom: "1px solid var(--hairline)", // softened divider line
          position: "relative",
          zIndex: 1,
        }}
      >
        {/* Background Developer Grid within mockup panel */}
        <div
          style={{
            position: "absolute",
            inset: 0,
            backgroundImage: "linear-gradient(rgba(28,28,28,0.015) 1px, transparent 1px), linear-gradient(90deg, rgba(28,28,28,0.015) 1px, transparent 1px)",
            backgroundSize: "90px 90px",
            backgroundPosition: "center center",
            transform: `scale(${cameraScale})`,
            zIndex: 0,
          }}
        />

        {/* Double-Bezel (Doppelrand) Card Architecture */}
        <div
          style={{
            width: 1800,
            height: 1300,
            background: "linear-gradient(135deg, #3b82f6, #94a3b8)", // Bezel gradient between the double borders
            border: "1px solid rgba(28, 28, 28, 0.15)", // Outer hairline border
            borderRadius: 48, // Large outer radius
            padding: 28, // Bezel thickness area (gap between double border)
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            boxShadow: "0 40px 90px -20px rgba(28, 28, 28, 0.22), 0 15px 40px -15px rgba(28, 28, 28, 0.12)", // Rich diffused shadow stack
            transform: `scale(${interpolate(inProgress, [0, 1], [0.94, 1], { easing: EASE }) * (1 + pulse * 0.015)}) translateY(${mockupY}px)`,
            opacity: inProgress,
            filter: `blur(${blur}px)`,
            zIndex: 1,
          }}
        >
          {/* Inner Core container */}
          <div
            style={{
              width: "100%",
              height: "100%",
              background: "var(--canvas-warm)", // Elevated off-white surface
              border: "1px solid rgba(28, 28, 28, 0.12)", // Inner highlight border outlining the app screen
              borderRadius: 32, // Concentric inner radius
              overflow: "hidden",
              position: "relative",
            }}
          >
            <MockupComponent />
          </div>
        </div>
      </div>

      {/* Bottom Panel: Typographic Info Panel */}
      <div
        style={{
          width: 2160,
          height: 1740,
          background: "var(--canvas)",
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "flex-start",
          paddingTop: 120, // push text upwards closer to divider
          paddingLeft: 180,
          paddingRight: 180,
          position: "relative",
          zIndex: 1,
        }}
      >
        <div
          style={{
            opacity: inProgress,
            transform: `translateY(${textY}px)`,
            filter: `blur(${blur}px)`,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            textAlign: "center",
          }}
        >
          {/* Decorative Icon Circle */}
          <div
            style={{
              width: 180,
              height: 180,
              borderRadius: "50%",
              background: "var(--surface-soft)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              marginBottom: 44,
            }}
          >
            <IconComponent size={84} weight="light" color="var(--ink)" />
          </div>

          {/* Eyebrow / Index */}
          <div
            style={{
              fontFamily: "Sora",
              fontWeight: 500,
              fontSize: 32,
              letterSpacing: "0.26em",
              color: "var(--mute)",
              textTransform: "uppercase",
              marginBottom: 20,
            }}
          >
            {index} //
          </div>

          {/* Heading */}
          <h2
            style={{
              fontFamily: "Sora",
              fontWeight: 500,
              fontSize: 92,
              letterSpacing: "-0.03em",
              color: "var(--ink)",
              margin: 0,
              lineHeight: 1.15,
              maxWidth: "22ch",
            }}
          >
            {heading}
          </h2>

          {/* Description */}
          <p
            style={{
              fontFamily: "Sora",
              fontWeight: 400,
              fontSize: 48,
              lineHeight: 1.55,
              color: "var(--body)",
              marginTop: 44, // Generous breathing space below heading
              marginRight: 0,
              marginBottom: 0,
              marginLeft: 0,
              maxWidth: "28ch",
            }}
          >
            {description}
          </p>
        </div>
      </div>
      <Grain />
    </div>
  );
}
