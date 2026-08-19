import React from "react";
import { useCurrentFrame, useVideoConfig, interpolate, spring, Easing } from "remotion";

const EASE = Easing.bezier(0.16, 1, 0.3, 1);

export function CodeEditorMockup() {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  // Entrance spring for the editor card itself
  const entrance = spring({
    frame,
    fps,
    config: { stiffness: 120, damping: 14, mass: 1 },
  });
  const scale = interpolate(entrance, [0, 1], [0.85, 1]);
  const opacity = interpolate(entrance, [0, 1], [0, 1]);

  // Sidebar files staggered fade-in
  const renderSidebarItem = (name: string, delay: number, isFolder = false) => {
    const itemEntrance = spring({
      frame: frame - delay,
      fps,
      config: { stiffness: 180, damping: 15 },
    });
    const x = interpolate(itemEntrance, [0, 1], [-20, 0]);
    const op = interpolate(itemEntrance, [0, 1], [0, 1]);

    return (
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 12,
          padding: "8px 12px",
          opacity: op,
          transform: `translateX(${x}px)`,
          color: isFolder ? "#94a3b8" : "#cbd5e1",
          fontFamily: "monospace",
          fontSize: 24,
        }}
      >
        <span style={{ fontSize: 26 }}>{isFolder ? "📁" : "📄"}</span>
        <span>{name}</span>
      </div>
    );
  };

  // Code lines to type out
  const codeLines = [
    "import { loadWorkspace } from 'vivianne';",
    "const repo = await loadWorkspace();",
    "// Reads and parses all real files",
    "const context = repo.findSymbols('user');",
    "console.log(`Context matches: ${context.length}`);",
  ];

  // Neon scan line animation
  const scannerY = interpolate(
    frame,
    [30, 80],
    [0, 360],
    {
      extrapolateLeft: "clamp",
      extrapolateRight: "clamp",
      easing: Easing.inOut(Easing.quad),
    }
  );
  const scannerOpacity = interpolate(
    frame,
    [25, 30, 75, 80],
    [0, 0.8, 0.8, 0],
    { extrapolateLeft: "clamp", extrapolateRight: "clamp" }
  );

  return (
    <div
      style={{
        width: "100%",
        height: 480,
        background: "#0f172a",
        borderRadius: 32,
        border: "1px solid rgba(255,255,255,0.1)",
        boxShadow: "0 25px 50px -12px rgba(0, 0, 0, 0.5)",
        overflow: "hidden",
        display: "flex",
        flexDirection: "column",
        opacity,
        transform: `scale(${scale})`,
      }}
    >
      {/* Editor Header / Title bar */}
      <div
        style={{
          height: 48,
          background: "#1e293b",
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
            fontFamily: "monospace",
          }}
        >
          workspace-parser.ts
        </div>
      </div>

      {/* Editor Body */}
      <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
        {/* Sidebar */}
        <div
          style={{
            width: 220,
            background: "#090d16",
            borderRight: "1px solid rgba(255,255,255,0.05)",
            padding: "24px 12px",
            display: "flex",
            flexDirection: "column",
            gap: 4,
          }}
        >
          {renderSidebarItem("src", 10, true)}
          {renderSidebarItem("  index.ts", 14)}
          {renderSidebarItem("  parser.ts", 18)}
          {renderSidebarItem("package.json", 22)}
          {renderSidebarItem("tsconfig.json", 26)}
        </div>

        {/* Code Viewport */}
        <div
          style={{
            flex: 1,
            padding: "32px 40px",
            background: "#0f172a",
            position: "relative",
            display: "flex",
            flexDirection: "column",
            gap: 16,
          }}
        >
          {/* Scanner Line Overlay */}
          <div
            style={{
              position: "absolute",
              left: 0,
              right: 0,
              top: scannerY,
              height: 4,
              background: "linear-gradient(90deg, rgba(56,189,248,0) 0%, #38bdf8 50%, rgba(56,189,248,0) 100%)",
              boxShadow: "0 0 16px #38bdf8",
              opacity: scannerOpacity,
              pointerEvents: "none",
            }}
          />

          {codeLines.map((line, index) => {
            const lineStartFrame = 15 + index * 8;
            const progress = interpolate(
              frame - lineStartFrame,
              [0, 10],
              [0, line.length],
              {
                extrapolateLeft: "clamp",
                extrapolateRight: "clamp",
                easing: Easing.linear,
              }
            );
            const typedText = line.slice(0, Math.floor(progress));

            // Code Syntax Highlight Helper (Very simple regex style)
            const highlightCode = (txt: string) => {
              if (txt.startsWith("//")) {
                return <span style={{ color: "#64748b" }}>{txt}</span>;
              }
              return txt.split(" ").map((word, idx) => {
                let color = "#e2e8f0";
                if (["const", "import", "from", "await"].includes(word)) {
                  color = "#f43f5e";
                } else if (word.includes("(")) {
                  color = "#38bdf8";
                } else if (word.startsWith("'") || word.startsWith("`")) {
                  color = "#10b981";
                }
                return (
                  <span key={idx} style={{ color, marginRight: 6 }}>
                    {word}
                  </span>
                );
              });
            };

            return (
              <div
                key={index}
                style={{
                  fontFamily: "monospace",
                  fontSize: 26,
                  lineHeight: 1.4,
                  whiteSpace: "pre-wrap",
                  height: 36,
                }}
              >
                <span style={{ color: "#334155", marginRight: 24, userSelect: "none" }}>
                  {index + 1}
                </span>
                {highlightCode(typedText)}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
