import React from "react";
import { useCurrentFrame, useVideoConfig, interpolate, spring } from "remotion";

export function GuiEditCommandMockup() {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  // Diff line helpers (staggered by 3 frames)
  const renderDiffLine = (oldNo: string, newNo: string, sign: string, text: string, type: "add" | "remove" | "context", delay: number) => {
    const lineEntrance = spring({
      frame: frame - delay,
      fps,
      config: { stiffness: 150, damping: 15 },
    });
    const op = interpolate(lineEntrance, [0, 1], [0, 1]);
    const y = interpolate(lineEntrance, [0, 1], [10, 0]);

    let bg = "transparent";
    let color = "var(--body)";
    if (type === "add") {
      bg = "rgba(16, 185, 129, 0.06)";
      color = "#10b981";
    } else if (type === "remove") {
      bg = "rgba(244, 63, 94, 0.06)";
      color = "#f43f5e";
    }

    return (
      <div
        key={text}
        style={{
          display: "flex",
          background: bg,
          color,
          fontSize: 24,
          fontFamily: "monospace",
          lineHeight: 1.35,
          padding: "6px 20px",
          opacity: op,
          transform: `translateY(${y}px)`,
        }}
      >
        <span style={{ width: 44, color: "var(--mute)", textAlign: "right", paddingRight: 12, userSelect: "none" }}>{oldNo}</span>
        <span style={{ width: 44, color: "var(--mute)", textAlign: "right", paddingRight: 12, userSelect: "none" }}>{newNo}</span>
        <span style={{ width: 24, userSelect: "none", color: type === "add" ? "#10b981" : type === "remove" ? "#f43f5e" : "var(--mute)" }}>{sign}</span>
        <span style={{ whiteSpace: "pre-wrap" }}>{text}</span>
      </div>
    );
  };

  // Beat-synced local frames:
  // Beat 13: 34 (terminal block appears)
  // Beat 14: 52 (success state + build logs)
  // Beat 15: 75 (build complete check)
  const isSuccess = frame >= 52;
  const showConsole = frame >= 34;
  const isBuilding = frame >= 52 && frame < 75;
  const isComplete = frame >= 75;

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
      {/* Left Sidebar: Session List */}
      <div
        style={{
          width: 460,
          background: "var(--canvas)", // Creamy background sidebar
          borderRight: "1px solid var(--hairline)",
          padding: "36px 16px",
          display: "flex",
          flexDirection: "column",
          gap: 12,
        }}
      >
        {/* Today Group */}
        <div style={{ color: "var(--mute)", fontSize: 20, fontWeight: 500, letterSpacing: "0.08em", paddingLeft: 12 }}>
          TODAY
        </div>
        <div
          style={{
            padding: "18px 24px",
            background: "var(--surface-soft)", // active session highlight
            borderRadius: 12,
            color: "var(--ink)",
            fontFamily: "Sora",
            fontSize: 26,
            fontWeight: 500,
          }}
        >
          Workspace analysis
        </div>

        {/* Yesterday Group */}
        <div style={{ color: "var(--mute)", fontSize: 20, fontWeight: 500, letterSpacing: "0.08em", paddingLeft: 12, marginTop: 24 }}>
          YESTERDAY
        </div>
        <div
          style={{
            padding: "18px 24px",
            color: "var(--body)",
            fontFamily: "Sora",
            fontSize: 26,
            fontWeight: 400,
          }}
        >
          oauth_setup
        </div>
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
            gui-session-edit
          </span>
        </div>

        {/* Scrollable activity log */}
        <div
          style={{
            flex: 1,
            padding: "40px",
            display: "flex",
            flexDirection: "column",
            gap: 28,
            overflow: "hidden",
          }}
        >
          {/* Tool Card Activity Row (write_to_file) */}
          <div
            style={{
              background: "var(--canvas-warm)",
              border: "1px solid var(--hairline)",
              borderRadius: 20,
              padding: "20px 28px",
              display: "flex",
              flexDirection: "column",
              opacity: interpolate(frame, [5, 12], [0, 1], { extrapolateRight: "clamp" }),
            }}
          >
            {/* Header row */}
            <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
              <span style={{ fontSize: 32 }}>🔧</span>
              <span style={{ color: "var(--ink)", fontSize: 28, fontWeight: 500, fontFamily: "Sora" }}>
                Tool: write_to_file
              </span>
              <span style={{ color: "var(--body)", fontSize: 26, fontFamily: "monospace", marginLeft: 16 }}>
                src/components/WorkspacePanel.tsx
              </span>
              <span
                style={{
                  background: isSuccess ? "rgba(16, 185, 129, 0.08)" : "var(--surface-soft)",
                  color: isSuccess ? "#10b981" : "var(--ink-strong)",
                  border: `1px solid ${isSuccess ? "#10b981" : "var(--hairline)"}`,
                  padding: "4px 16px",
                  borderRadius: 8,
                  fontSize: 20,
                  fontWeight: 500,
                  fontFamily: "Sora",
                  marginLeft: "auto",
                }}
              >
                {isSuccess ? "Success" : "Executing"}
              </span>
            </div>

            {/* Expanded diff output block */}
            <div
              style={{
                background: "var(--surface-subtle)",
                borderRadius: 12,
                marginTop: 18,
                padding: "16px 0",
                border: "1px solid var(--hairline)",
                overflow: "hidden",
                height: 240, // compact diff size
              }}
            >
              {renderDiffLine("30", "30", " ", "export function WorkspacePanel() {", "context", 10)}
              {renderDiffLine("31", " ", "-", "  const { folderPath, setFolder } = useAgent();", "remove", 13)}
              {renderDiffLine(" ", "31", "+", "  const { workspaceFolders, setAgentWorkspace } = useAgent();", "add", 16)}
              {renderDiffLine("32", "32", " ", "  const [busy, setBusy] = useState(false);", "context", 19)}
              {renderDiffLine("33", " ", "-", "  const triggerLabel = folderName(folderPath);", "remove", 22)}
              {renderDiffLine(" ", "33", "+", "  const triggerLabel = workspaceFolders.length === 1", "add", 25)}
            </div>
          </div>

          {/* Terminal Bash Activity Row (run_command) */}
          {showConsole && (
            <div
              style={{
                background: "var(--canvas-warm)",
                border: "1px solid var(--hairline)",
                borderRadius: 20,
                padding: "20px 28px",
                display: "flex",
                flexDirection: "column",
                opacity: interpolate(frame, [34, 40], [0, 1], { extrapolateRight: "clamp" }),
              }}
            >
              {/* Header row */}
              <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
                <span style={{ fontSize: 32 }}>💻</span>
                <span style={{ color: "var(--ink)", fontSize: 28, fontWeight: 500, fontFamily: "Sora" }}>
                  Command
                </span>
                <span style={{ color: "var(--body)", fontSize: 26, fontFamily: "monospace", marginLeft: 16 }}>
                  npm run build
                </span>
                <span
                  style={{
                    background: isComplete ? "rgba(16, 185, 129, 0.08)" : "var(--surface-soft)",
                    color: isComplete ? "#10b981" : "var(--ink-strong)",
                    border: `1px solid ${isComplete ? "#10b981" : "var(--hairline)"}`,
                    padding: "4px 16px",
                    borderRadius: 8,
                    fontSize: 20,
                    fontWeight: 500,
                    fontFamily: "Sora",
                    marginLeft: "auto",
                  }}
                >
                  {isComplete ? "Success" : "Running"}
                </span>
              </div>

              {/* Terminal logs block */}
              <div
                style={{
                  background: "var(--surface-subtle)",
                  borderRadius: 12,
                  marginTop: 18,
                  padding: "16px 24px",
                  border: "1px solid var(--hairline)",
                  fontFamily: "monospace",
                  fontSize: 24,
                  color: "var(--body)",
                  lineHeight: 1.4,
                  display: "flex",
                  flexDirection: "column",
                  gap: 6,
                }}
              >
                <div>
                  <span style={{ color: "var(--mute)" }}>vivianne-shell $</span> npm run build
                </div>
                {(isBuilding || isComplete) && <div>&gt; tsc --noEmit && vite build</div>}
                {isComplete && (
                  <div style={{ color: "#10b981", fontWeight: 500 }}>
                    ✓ Build completed successfully (1.18s)
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
