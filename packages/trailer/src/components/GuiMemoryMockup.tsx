import React from "react";
import { useCurrentFrame, useVideoConfig, interpolate, spring } from "remotion";

export function GuiMemoryMockup() {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  // Restore badge spring
  const badgeSpring = spring({
    frame: frame - 29, // synchronized to Beat 17 (Frame 29)
    fps,
    config: { stiffness: 140, damping: 12 },
  });
  const badgeScale = interpolate(badgeSpring, [0, 1], [0.88, 1]);
  const badgeOpacity = interpolate(badgeSpring, [0, 1], [0, 1]);

  // Database list items
  const renderDbRow = (table: string, count: string, delay: number, isActive = false) => {
    const rowSpring = spring({
      frame: frame - delay,
      fps,
      config: { stiffness: 160, damping: 15 },
    });
    const x = interpolate(rowSpring, [0, 1], [-12, 0]);
    const op = interpolate(rowSpring, [0, 1], [0, 1]);

    return (
      <div
        key={table}
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          padding: "16px 24px",
          borderRadius: 12,
          background: isActive ? "var(--surface-soft)" : "transparent",
          border: `1px solid ${isActive ? "var(--ink)" : "var(--hairline)"}`,
          opacity: op,
          transform: `translateX(${x}px)`,
          color: "var(--ink)",
          fontFamily: "monospace",
          fontSize: 24,
          fontWeight: isActive ? 500 : 400,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <span style={{ color: isActive ? "var(--ink)" : "var(--mute)", fontSize: 28 }}>📁</span>
          <span>{table}</span>
        </div>
        <span style={{ color: "var(--mute)", fontSize: 20 }}>{count} rows</span>
      </div>
    );
  };

  // Beat-synced local frames:
  // Beat 17: 29 (badge popup)
  // Beat 18: 52 (workspace_symbols active)
  // Beat 19: 69 (vector_embeddings active)
  const showOverlay = frame >= 29;
  const isSymbolsActive = frame >= 52;
  const isVectorsActive = frame >= 69;

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
      {/* Left Sidebar: Session History */}
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
          TODAY
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
          Workspace analysis
        </div>

        <div style={{ color: "var(--mute)", fontSize: 20, fontWeight: 500, letterSpacing: "0.08em", paddingLeft: 12, marginTop: 24 }}>
          YESTERDAY
        </div>
        <div
          style={{
            padding: "18px 24px",
            background: "var(--surface-soft)", // Yesterday's session is selected / active
            borderRadius: 12,
            color: "var(--ink)",
            fontFamily: "Sora",
            fontSize: 26,
            fontWeight: 500,
          }}
        >
          oauth_setup
        </div>
      </div>

      {/* Right Main Panel: Activity Log */}
      <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden", position: "relative" }}>
        
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
            oauth_setup (Restored)
          </span>
        </div>

        {/* Scrollable activity log */}
        <div
          style={{
            flex: 1,
            padding: "44px",
            display: "flex",
            flexDirection: "column",
            gap: 28,
            overflow: "hidden",
          }}
        >
          {/* Restored User Message Row */}
          <div
            style={{
              background: "var(--surface-soft)",
              border: "1px solid var(--hairline)",
              borderRadius: 20,
              padding: "20px 28px",
              display: "flex",
              flexDirection: "column",
              gap: 8,
              opacity: interpolate(frame, [8, 16], [0, 0.7], { extrapolateRight: "clamp" }),
              transform: `translateY(${interpolate(frame, [8, 16], [12, 0], { extrapolateRight: "clamp" })}px)`,
            }}
          >
            <div style={{ fontSize: 20, color: "var(--mute)", fontFamily: "Sora", fontWeight: 500, letterSpacing: "0.04em" }}>
              USER QUERY
            </div>
            <div style={{ color: "var(--ink)", fontSize: 28, fontFamily: "Sora", lineHeight: 1.4 }}>
              let's add github oauth to the main layout
            </div>
          </div>

          {/* SQLite Database Restore activity row */}
          <div
            style={{
              background: "var(--canvas-warm)",
              border: "1px solid var(--hairline)",
              borderRadius: 20,
              padding: "20px 28px",
              display: "flex",
              flexDirection: "column",
              opacity: interpolate(frame, [16, 24], [0, 1], { extrapolateRight: "clamp" }),
              transform: `translateY(${interpolate(frame, [16, 24], [12, 0], { extrapolateRight: "clamp" })}px)`,
            }}
          >
            {/* Header row */}
            <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
              <span style={{ fontSize: 32 }}>💾</span>
              <span style={{ color: "var(--ink)", fontSize: 28, fontWeight: 500, fontFamily: "Sora" }}>
                Sqlite Database Indexes
              </span>
              <span style={{ color: "#10b981", fontSize: 20, marginLeft: "auto", fontWeight: 500, background: "rgba(16, 185, 129, 0.08)", padding: "4px 12px", borderRadius: 8 }}>
                Restored
              </span>
            </div>

            {/* List of active indexes inside the output block */}
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "1fr 1fr",
                gap: 12,
                marginTop: 18,
                background: "var(--surface-subtle)",
                borderRadius: 12,
                padding: "16px",
                border: "1px solid var(--hairline)",
              }}
            >
              {renderDbRow("prior_sessions", "3", 8)}
              {renderDbRow("file_index", "142", 12)}
              {renderDbRow("workspace_symbols", "2,981", 16, isSymbolsActive)}
              {renderDbRow("vector_embeddings", "1,884", 20, isVectorsActive)}
            </div>
          </div>

          {/* Context Restored Overlay dialog */}
          {showOverlay && (
            <div
              style={{
                position: "absolute",
                top: "35%",
                left: "20%",
                right: "20%",
                background: "var(--canvas-warm)",
                border: "1px solid var(--hairline-strong)",
                boxShadow: "0 25px 65px rgba(28,28,28,0.08)", // soft, elevated dialog shadow
                borderRadius: 24,
                padding: "44px",
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                gap: 16,
                opacity: badgeOpacity,
                transform: `scale(${badgeScale})`,
                zIndex: 10,
              }}
            >
              <div style={{ fontSize: 56 }}>⚡</div>
              <div style={{ color: "var(--ink)", fontSize: 32, fontWeight: 500, fontFamily: "Sora", textAlign: "center" }}>
                Session Context Restored
              </div>
              <div style={{ color: "var(--body)", fontSize: 24, fontFamily: "monospace", textAlign: "center", marginTop: 4 }}>
                sqlite: restored 142 files, 3 prior chat histories
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
