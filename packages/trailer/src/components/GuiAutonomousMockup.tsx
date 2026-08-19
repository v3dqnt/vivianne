import React from "react";
import { useCurrentFrame, useVideoConfig, interpolate, spring } from "remotion";

export function GuiAutonomousMockup() {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  // Checklist items
  const renderCheckItem = (text: string, state: "done" | "active" | "pending", delay: number) => {
    const itemSpring = spring({
      frame: frame - delay,
      fps,
      config: { stiffness: 150, damping: 15 },
    });
    const x = interpolate(itemSpring, [0, 1], [-20, 0]);
    const op = interpolate(itemSpring, [0, 1], [0, 1]);

    const isDone = state === "done";
    const isActive = state === "active";

    return (
      <div
        key={text}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 20,
          opacity: op,
          transform: `translateX(${x}px)`,
        }}
      >
        <div
          style={{
            width: 36,
            height: 36,
            borderRadius: "50%",
            background: isDone ? "var(--ink)" : isActive ? "var(--surface-soft)" : "transparent",
            border: `2px solid ${isDone || isActive ? "var(--ink)" : "var(--hairline-strong)"}`,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          {isDone ? (
            <span style={{ color: "var(--canvas-warm)", fontSize: 20, fontWeight: "bold" }}>✓</span>
          ) : isActive ? (
            <div
              style={{
                width: 12,
                height: 12,
                border: "2px solid var(--ink)",
                borderTopColor: "transparent",
                borderRadius: "50%",
                animation: "spin 1s linear infinite",
              }}
            />
          ) : (
            <div style={{ width: 6, height: 6, borderRadius: "50%", background: "var(--mute)" }} />
          )}
        </div>
        <span
          style={{
            fontSize: 28,
            fontFamily: "Sora",
            fontWeight: isDone || isActive ? 500 : 400,
            color: isDone ? "var(--ink)" : isActive ? "var(--ink-strong)" : "var(--mute)",
          }}
        >
          {text}
        </span>
      </div>
    );
  };

  // Beat-synced local frames:
  // Beat 21: 35 (step 3 done, step 4 active)
  // Beat 22: 52 (test logs start)
  // End sequence resolves at 65+
  const step3State = frame >= 35 ? "done" : "active";
  const step4State = frame >= 35 ? (frame >= 65 ? "done" : "active") : "pending";

  const showStep3Logs = frame >= 10;
  const showSavedMsg = frame >= 35;
  const showStep4Logs = frame >= 35;
  const showRunningMsg = frame >= 52;
  const showPassMsg = frame >= 65;

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
      {/* Left Sidebar: File Tree */}
      <div
        style={{
          width: 460,
          background: "var(--canvas)",
          borderRight: "1px solid var(--hairline)",
          padding: "36px 16px",
          display: "flex",
          flexDirection: "column",
          gap: 12,
        }}
      >
        <div style={{ color: "var(--mute)", fontSize: 20, fontWeight: 500, letterSpacing: "0.08em", paddingLeft: 12 }}>
          📁 WORKSPACE : GUI
        </div>
        <div style={{ padding: "18px 24px", color: "var(--body)", fontFamily: "Sora", fontSize: 26, paddingLeft: 36 }}>
          📁 src
        </div>
        <div style={{ padding: "18px 24px", color: "var(--body)", fontFamily: "Sora", fontSize: 26, paddingLeft: 64 }}>
          📁 app
        </div>
        <div style={{ padding: "18px 24px", background: "var(--surface-soft)", borderRadius: 12, color: "var(--ink)", fontFamily: "Sora", fontSize: 26, paddingLeft: 92, fontWeight: 500 }}>
          📄 layout.tsx
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
            gui-session-autonomous
          </span>
        </div>

        {/* Scrollable activity log */}
        <div
          style={{
            flex: 1,
            padding: "44px",
            display: "flex",
            flexDirection: "row",
            gap: 36,
            overflow: "hidden",
          }}
        >
          {/* Left Block: Checklist */}
          <div
            style={{
              flex: 1.1,
              display: "flex",
              flexDirection: "column",
              gap: 28,
            }}
          >
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              <span style={{ color: "var(--mute)", fontSize: 20, fontFamily: "Sora", fontWeight: 500, letterSpacing: "0.08em" }}>
                CURRENT MISSION
              </span>
              <span style={{ color: "var(--ink)", fontSize: 28, fontFamily: "Sora", fontWeight: 500 }}>
                Implement OAuth login switch
              </span>
            </div>

            <div style={{ display: "flex", flexDirection: "column", gap: 24, marginTop: 12 }}>
              {renderCheckItem("Research files & imports", "done", 8)}
              {renderCheckItem("Draft implementation plan", "done", 12)}
              {renderCheckItem("Execute edits in layout.tsx", step3State, 16)}
              {renderCheckItem("Verify with build & tests", step4State, 20)}
            </div>
          </div>

          {/* Right Block: Active Terminal Logs */}
          <div
            style={{
              flex: 0.9,
              background: "var(--surface-subtle)",
              borderRadius: 16,
              border: "1px solid var(--hairline)",
              padding: "28px",
              fontFamily: "monospace",
              fontSize: 24,
              color: "var(--body)",
              display: "flex",
              flexDirection: "column",
              gap: 12,
              overflow: "hidden",
            }}
          >
            {showStep3Logs && (
              <div>
                <span style={{ color: "var(--ink)" }}>[edit]</span> Writing layout.tsx...
              </div>
            )}

            {showSavedMsg && (
              <div style={{ color: "#10b981" }}>
                ✓ Saved edits (24 lines changed)
              </div>
            )}

            {showStep4Logs && (
              <div style={{ marginTop: 12, display: "flex", flexDirection: "column", gap: 10 }}>
                <div>
                  <span style={{ color: "var(--mute)" }}>shell $</span> npx jest
                </div>
                {showRunningMsg && (
                  <div>
                    Running layout tests...
                  </div>
                )}
                {showPassMsg && (
                  <div style={{ color: "#10b981", fontSize: 22, lineHeight: 1.35 }}>
                    PASS  src/app/layout.test.tsx
                    <br />
                    ✓ renders header component (84ms)
                    <br />
                    ✓ binds login oauth buttons (45ms)
                    <br />
                    <br />
                    <span style={{ color: "var(--mute)" }}>Test Suites: 1 passed, 1 total</span>
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
      </div>

      <style>{`
        @keyframes spin {
          0% { transform: rotate(0deg); }
          100% { transform: rotate(360deg); }
        }
      `}</style>
    </div>
  );
}
