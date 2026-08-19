import React from "react";
import { useCurrentFrame, useVideoConfig, interpolate, spring, Easing } from "remotion";

export function PipelineMockup() {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  // Entrance spring for the overall canvas
  const entrance = spring({
    frame,
    fps,
    config: { stiffness: 120, damping: 14, mass: 1 },
  });
  const scale = interpolate(entrance, [0, 1], [0.85, 1]);
  const opacity = interpolate(entrance, [0, 1], [0, 1]);

  const steps = [
    { title: "Research codebase", duration: [10, 24] },
    { title: "Draft implementation plan", duration: [28, 42] },
    { title: "Execute source edits", duration: [46, 62] },
    { title: "Run verification tests", duration: [66, 82] },
  ];

  const renderStep = (step: typeof steps[0], index: number) => {
    const [start, end] = step.duration;

    // Item fade/slide in
    const itemEntrance = spring({
      frame: frame - start,
      fps,
      config: { stiffness: 150, damping: 15 },
    });
    const x = interpolate(itemEntrance, [0, 1], [-20, 0]);
    const op = interpolate(itemEntrance, [0, 1], [0, 1]);

    // Active state (completed vs running vs pending)
    const isCompleted = frame >= end;
    const isActive = frame >= start && frame < end;

    // Checkmark/Spinner scale animation
    const stateSpring = spring({
      frame: frame - (isCompleted ? end : start),
      fps,
      config: { stiffness: 200, damping: 12 },
    });
    const stateScale = interpolate(stateSpring, [0, 1], [0, 1]);

    return (
      <div
        key={index}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 24,
          opacity: op,
          transform: `translateX(${x}px)`,
        }}
      >
        {/* Status circle/checkmark */}
        <div
          style={{
            width: 44,
            height: 44,
            borderRadius: "50%",
            background: isCompleted ? "#10b981" : isActive ? "rgba(56,189,248,0.15)" : "#1e293b",
            border: `2px solid ${isCompleted ? "#10b981" : isActive ? "#38bdf8" : "#334155"}`,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            transform: `scale(${stateScale})`,
            boxShadow: isActive ? "0 0 12px #38bdf8" : "none",
          }}
        >
          {isCompleted ? (
            <span style={{ color: "#0f172a", fontSize: 24, fontWeight: "bold" }}>✓</span>
          ) : isActive ? (
            // Spinner
            <div
              style={{
                width: 18,
                height: 18,
                border: "2px solid #38bdf8",
                borderTopColor: "transparent",
                borderRadius: "50%",
                animation: "spin 1s linear infinite",
              }}
            />
          ) : (
            <div style={{ width: 8, height: 8, borderRadius: "50%", background: "#475569" }} />
          )}
        </div>

        {/* Step details */}
        <div style={{ display: "flex", flexDirection: "column" }}>
          <span
            style={{
              fontSize: 26,
              fontWeight: isActive || isCompleted ? 600 : 400,
              color: isCompleted ? "#e2e8f0" : isActive ? "#38bdf8" : "#64748b",
            }}
          >
            {step.title}
          </span>
        </div>
      </div>
    );
  };

  // Timeline connector height calculation
  const connectorHeight = interpolate(
    frame,
    [10, 82],
    [0, 200],
    {
      extrapolateLeft: "clamp",
      extrapolateRight: "clamp",
      easing: Easing.linear,
    }
  );

  return (
    <div
      style={{
        width: "100%",
        height: 480,
        background: "#090d16",
        borderRadius: 32,
        border: "1px solid rgba(255,255,255,0.1)",
        boxShadow: "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
        overflow: "hidden",
        display: "flex",
        flexDirection: "column",
        opacity,
        transform: `scale(${scale})`,
        fontFamily: "monospace",
      }}
    >
      {/* Title bar */}
      <div
        style={{
          height: 48,
          background: "#111827",
          borderBottom: "1px solid rgba(255,255,255,0.05)",
          display: "flex",
          alignItems: "center",
          padding: "0 24px",
          gap: 8,
        }}
      >
        <div style={{ width: 12, height: 12, borderRadius: "50%", background: "#ef4444" }} />
        <div style={{ width: 12, height: 12, borderRadius: "50%", background: "#f59e0b" }} />
        <div style={{ width: 12, height: 12, borderRadius: "50%", background: "#10b981" }} />
        <div
          style={{
            marginLeft: 32,
            color: "#64748b",
            fontSize: 22,
          }}
        >
          agent-runner.log
        </div>
      </div>

      {/* Steps Pipeline container */}
      <div
        style={{
          flex: 1,
          padding: "48px 64px",
          position: "relative",
          display: "flex",
          flexDirection: "column",
          gap: 36,
        }}
      >
        {/* Left vertical connector line */}
        <div
          style={{
            position: "absolute",
            left: 85, // align center to circles
            top: 70,
            width: 2,
            height: 220,
            background: "#1e293b",
            zIndex: 0,
          }}
        />

        {/* Left vertical active glowing connector line */}
        <div
          style={{
            position: "absolute",
            left: 85,
            top: 70,
            width: 2,
            height: connectorHeight,
            background: "#38bdf8",
            boxShadow: "0 0 8px #38bdf8",
            zIndex: 0,
          }}
        />

        {/* Steps */}
        <div style={{ zIndex: 1, display: "flex", flexDirection: "column", gap: 36 }}>
          {steps.map((step, index) => renderStep(step, index))}
        </div>
      </div>

      {/* CSS injection for spinning (Since native CSS keyframes are not in Remotion runtime, we can do manual spin but it's simpler to inject a style tag) */}
      <style>{`
        @keyframes spin {
          0% { transform: rotate(0deg); }
          100% { transform: rotate(360deg); }
        }
      `}</style>
    </div>
  );
}
