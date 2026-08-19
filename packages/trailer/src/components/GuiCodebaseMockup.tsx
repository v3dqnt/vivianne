import React from "react";
import { useCurrentFrame, useVideoConfig, interpolate, spring } from "remotion";

export function GuiCodebaseMockup() {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  // Sidebar tree files staggered reveal
  const renderTreeItem = (name: string, indent: number, delay: number, isActive = false) => {
    const itemEntrance = spring({
      frame: frame - delay,
      fps,
      config: { stiffness: 180, damping: 16 },
    });
    const x = interpolate(itemEntrance, [0, 1], [-16, 0]);
    const op = interpolate(itemEntrance, [0, 1], [0, 1]);

    return (
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 16,
          padding: "20px 28px",
          paddingLeft: 28 + indent * 36,
          opacity: op,
          transform: `translateX(${x}px)`,
          background: isActive ? "var(--surface-soft)" : "transparent",
          borderLeft: isActive ? "4px solid var(--ink)" : "4px solid transparent",
          color: isActive ? "var(--ink)" : "var(--body)",
          fontFamily: "Sora, monospace",
          fontSize: 30,
          fontWeight: isActive ? 500 : 400,
        }}
      >
        <span style={{ fontSize: 32, opacity: 0.8 }}>{name.includes(".") ? "📄" : "📁"}</span>
        <span>{name}</span>
        {isActive && (
          <span style={{
            fontSize: 20,
            color: "var(--ink)",
            marginLeft: "auto",
            background: "var(--surface-soft)",
            border: "1px solid var(--hairline)",
            padding: "4px 12px",
            borderRadius: 8,
            fontWeight: 500,
            letterSpacing: "0.04em",
          }}>
            SCANNING
          </span>
        )}
      </div>
    );
  };

  // User input message (typed)
  const chatText = "Can you help me analyze the codebase directory?";
  const typedChat = chatText.slice(0, Math.floor(interpolate(frame, [5, 20], [0, chatText.length], { extrapolateRight: "clamp" })));

  // Reasoning transitions beat-synced to local frames:
  // Beat 9: 28
  // Beat 10: 51
  // Beat 11: 69
  const step1Active = frame >= 28 && frame < 51;
  const step2Active = frame >= 51 && frame < 69;
  const step3Active = frame >= 69;

  return (
    <div
      style={{
        width: "100%",
        height: "100%",
        display: "flex",
        flexDirection: "row",
        background: "var(--canvas-warm)",
      }}
    >
      {/* Left Sidebar: Workspace File Tree */}
      <div
        style={{
          width: 460,
          background: "var(--canvas)", // Creamy background for sidebar
          borderRight: "1px solid var(--hairline)",
          padding: "36px 12px",
          display: "flex",
          flexDirection: "column",
          gap: 6,
        }}
      >
        <div
          style={{
            padding: "16px 28px 24px 28px",
            color: "var(--mute)",
            fontSize: 26,
            fontWeight: 500,
            fontFamily: "Sora",
            borderBottom: "1px solid var(--hairline)",
            marginBottom: 16,
            letterSpacing: "0.06em",
          }}
        >
          📁 WORKSPACE : GUI
        </div>
        {renderTreeItem("src", 0, 10)}
        {renderTreeItem("components", 1, 14)}
        {renderTreeItem("AgentProvider.tsx", 2, 18, step1Active)}
        {renderTreeItem("ChatInterface.tsx", 2, 22, step2Active)}
        {renderTreeItem("WorkspacePanel.tsx", 2, 26, step3Active)}
        {renderTreeItem("DiffView.tsx", 2, 30)}
      </div>

      {/* Right Main Panel: Activity Log */}
      <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
        
        {/* Header bar */}
        <div
          style={{
            height: 96,
            background: "rgba(28, 28, 28, 0.01)",
            borderBottom: "1px solid var(--hairline)",
            display: "flex",
            alignItems: "center",
            padding: "0 40px",
            gap: 16,
          }}
        >
          <div style={{ width: 14, height: 14, borderRadius: "50%", background: "var(--mute)" }} />
          <div style={{ width: 14, height: 14, borderRadius: "50%", background: "var(--mute)" }} />
          <div style={{ width: 14, height: 14, borderRadius: "50%", background: "var(--mute)" }} />
          <span
            style={{
              marginLeft: 24,
              color: "var(--mute)",
              fontSize: 28,
              fontFamily: "Sora",
              fontWeight: 500,
            }}
          >
            gui-session-analysis
          </span>
        </div>

        {/* Activity Feed content */}
        <div
          style={{
            flex: 1,
            padding: "48px",
            display: "flex",
            flexDirection: "column",
            gap: 32,
          }}
        >
          {/* User Message Row */}
          {frame >= 5 && (
            <div
              style={{
                background: "var(--surface-subtle)",
                border: "1px solid var(--hairline)",
                borderRadius: 20,
                padding: "24px 32px",
                display: "flex",
                flexDirection: "column",
                gap: 8,
              }}
            >
              <div style={{ fontSize: 22, color: "var(--mute)", fontFamily: "Sora", fontWeight: 500, letterSpacing: "0.04em" }}>
                USER QUERY
              </div>
              <div style={{ color: "var(--ink)", fontSize: 32, fontFamily: "Sora", lineHeight: 1.4 }}>
                {typedChat}
              </div>
            </div>
          )}

          {/* Assistant Reasoning Row (Activity Feed row style) */}
          {frame >= 28 && (
            <div
              style={{
                background: "var(--canvas-warm)",
                border: "1px solid var(--hairline)",
                borderRadius: 20,
                padding: "24px 32px",
                display: "flex",
                flexDirection: "column",
                opacity: interpolate(frame, [28, 38], [0, 1], { extrapolateRight: "clamp" }),
              }}
            >
              {/* Row Header */}
              <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
                <span style={{ fontSize: 32 }}>💭</span>
                <span style={{ color: "var(--ink)", fontSize: 28, fontWeight: 500, fontFamily: "Sora" }}>
                  Reasoning Process
                </span>
                <span style={{ color: "var(--mute)", fontSize: 24, marginLeft: "auto", fontFamily: "monospace" }}>
                  active
                </span>
              </div>

              {/* Row Output block */}
              <div
                style={{
                  background: "var(--surface-subtle)",
                  borderRadius: 12,
                  padding: "20px 24px",
                  marginTop: 20,
                  fontFamily: "monospace",
                  fontSize: 26,
                  color: "var(--body)",
                  lineHeight: 1.5,
                  border: "1px solid var(--hairline)",
                }}
              >
                {step1Active && "> Searching workspace entry points..."}
                {step2Active && "> Found package.json. Parsing workspaces..."}
                {step3Active && "> Analyzing files in gui/src/components..."}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
