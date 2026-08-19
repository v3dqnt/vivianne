import React from "react";
import { useCurrentFrame, useVideoConfig, interpolate, spring, Easing } from "remotion";

export function TerminalMockup() {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  // Entrance animation for the terminal
  const entrance = spring({
    frame,
    fps,
    config: { stiffness: 120, damping: 14, mass: 1 },
  });
  const scale = interpolate(entrance, [0, 1], [0.85, 1]);
  const opacity = interpolate(entrance, [0, 1], [0, 1]);

  // Command typing
  const command = "vivianne run tests --watch=false";
  const cmdProgress = interpolate(
    frame - 10,
    [0, 15],
    [0, command.length],
    {
      extrapolateLeft: "clamp",
      extrapolateRight: "clamp",
      easing: Easing.linear,
    }
  );
  const typedCommand = command.slice(0, Math.floor(cmdProgress));

  // Outputs staggered entrance
  const getOutputStyle = (delay: number) => {
    const outputSpring = spring({
      frame: frame - delay,
      fps,
      config: { stiffness: 150, damping: 16 },
    });
    const y = interpolate(outputSpring, [0, 1], [15, 0]);
    const op = interpolate(outputSpring, [0, 1], [0, 1]);

    return {
      transform: `translateY(${y}px)`,
      opacity: op,
    };
  };

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
      {/* Terminal Title Bar */}
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
          bash - vivianne@agent-terminal
        </div>
      </div>

      {/* Terminal body */}
      <div
        style={{
          flex: 1,
          padding: "36px 48px",
          display: "flex",
          flexDirection: "column",
          gap: 16,
          fontSize: 26,
          lineHeight: 1.4,
          color: "#e2e8f0",
        }}
      >
        {/* Terminal Prompt Line */}
        <div style={{ display: "flex", gap: 16 }}>
          <span style={{ color: "#10b981" }}>vivianne-shell $</span>
          <span>
            {typedCommand}
            {frame >= 10 && frame % 15 < 8 && <span style={{ color: "#38bdf8" }}>█</span>}
          </span>
        </div>

        {/* Outputs */}
        {frame >= 28 && (
          <div style={{ ...getOutputStyle(28), color: "#94a3b8" }}>
            [info] Initializing sandboxed test execution environment...
          </div>
        )}
        {frame >= 36 && (
          <div style={{ ...getOutputStyle(36), color: "#f59e0b" }}>
            [info] Running Jest test suite on 14 files...
          </div>
        )}
        {frame >= 44 && (
          <div
            style={{
              ...getOutputStyle(44),
              display: "flex",
              gap: 16,
              alignItems: "center",
            }}
          >
            <span
              style={{
                background: "#10b981",
                color: "#0f172a",
                fontWeight: "bold",
                padding: "2px 8px",
                borderRadius: 4,
                fontSize: 22,
              }}
            >
              PASS
            </span>
            <span style={{ color: "#e2e8f0", fontWeight: 500 }}>src/parser.test.ts</span>
          </div>
        )}
        {frame >= 50 && (
          <div style={{ ...getOutputStyle(50), color: "#10b981", marginLeft: 8 }}>
            ✓ parseWorkspace() matches exact AST structure (142ms)
          </div>
        )}
        {frame >= 56 && (
          <div style={{ ...getOutputStyle(56), color: "#10b981", fontWeight: "bold" }}>
            Test Suites: 1 passed, 1 total
            <br />
            Tests:       12 passed, 12 total
            <br />
            Snapshots:   0 total
            <br />
            Time:        1.25s
          </div>
        )}
      </div>
    </div>
  );
}
