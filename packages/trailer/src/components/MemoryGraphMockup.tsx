import React from "react";
import { useCurrentFrame, useVideoConfig, interpolate, spring, Easing } from "remotion";

export function MemoryGraphMockup() {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  // Entrance spring for the overall canvas card
  const entrance = spring({
    frame,
    fps,
    config: { stiffness: 120, damping: 14, mass: 1 },
  });
  const scale = interpolate(entrance, [0, 1], [0.85, 1]);
  const opacity = interpolate(entrance, [0, 1], [0, 1]);

  // Coordinates of the nodes
  const nodes = [
    { x: 120, y: 320, label: "Session 1: Context Root", delay: 10 },
    { x: 300, y: 140, label: "Session 2: Edit Plan", delay: 26 },
    { x: 480, y: 300, label: "Session 3: Run Verify", delay: 42 },
  ];

  const renderNode = (node: typeof nodes[0], index: number) => {
    const nodeSpring = spring({
      frame: frame - node.delay,
      fps,
      config: { stiffness: 150, damping: 12 },
    });
    const s = interpolate(nodeSpring, [0, 1], [0, 1]);
    const op = interpolate(nodeSpring, [0, 1], [0, 1]);

    const isCurrent = index === 2; // Last node represents the current session

    return (
      <g key={index} style={{ opacity: op }}>
        {/* Glowing aura */}
        <circle
          cx={node.x}
          cy={node.y}
          r={isCurrent ? 28 : 20}
          fill="none"
          stroke={isCurrent ? "#38bdf8" : "#818cf8"}
          strokeWidth={4}
          style={{
            opacity: interpolate(
              Math.sin(frame * 0.1 + index) * 0.5 + 0.5,
              [0, 1],
              [0.3, 0.9]
            ),
          }}
        />
        {/* Solid core */}
        <circle
          cx={node.x}
          cy={node.y}
          r={isCurrent ? 14 : 10}
          fill={isCurrent ? "#38bdf8" : "#818cf8"}
          transform={`scale(${s})`}
          style={{ transformOrigin: `${node.x}px ${node.y}px` }}
        />
        {/* Label */}
        <text
          x={node.x}
          y={node.y + 44}
          fill="#cbd5e1"
          fontSize={18}
          fontFamily="monospace"
          textAnchor="middle"
          fontWeight="bold"
        >
          {node.label}
        </text>
      </g>
    );
  };

  // Line drawing animations
  const path1Progress = spring({
    frame: frame - 18,
    fps,
    config: { stiffness: 90, damping: 14 },
  });
  const path2Progress = spring({
    frame: frame - 34,
    fps,
    config: { stiffness: 90, damping: 14 },
  });

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
            fontFamily: "monospace",
          }}
        >
          agent-memory-db.sqlite
        </div>
      </div>

      {/* SVG Canvas for Memory Graph */}
      <div style={{ flex: 1, position: "relative" }}>
        <svg
          width="100%"
          height="100%"
          viewBox="0 0 600 400"
          style={{ overflow: "visible" }}
        >
          {/* Paths connecting nodes */}
          {/* Path 1: Node 0 -> Node 1 */}
          <path
            d="M 120 320 Q 210 230 300 140"
            fill="none"
            stroke="#4f46e5"
            strokeWidth={3}
            strokeDasharray={300}
            strokeDashoffset={300 * (1 - path1Progress)}
            style={{ opacity: 0.7 }}
          />

          {/* Path 2: Node 1 -> Node 2 */}
          <path
            d="M 300 140 Q 390 220 480 300"
            fill="none"
            stroke="#4f46e5"
            strokeWidth={3}
            strokeDasharray={300}
            strokeDashoffset={300 * (1 - path2Progress)}
            style={{ opacity: 0.7 }}
          />

          {/* Pulse 1: Traveling along path 1 */}
          {frame >= 18 && frame <= 42 && (
            <circle cx={120 + 180 * ((frame - 18) / 24)} cy={320 - 180 * ((frame - 18) / 24)} r={6} fill="#38bdf8">
              <animate attributeName="r" values="6;10;6" dur="0.8s" repeatCount="indefinite" />
            </circle>
          )}

          {/* Pulse 2: Traveling along path 2 */}
          {frame >= 34 && frame <= 58 && (
            <circle cx={300 + 180 * ((frame - 34) / 24)} cy={140 + 160 * ((frame - 34) / 24)} r={6} fill="#38bdf8">
              <animate attributeName="r" values="6;10;6" dur="0.8s" repeatCount="indefinite" />
            </circle>
          )}

          {/* Render Nodes */}
          {nodes.map((node, index) => renderNode(node, index))}
        </svg>

        {/* Ambient Grid overlay */}
        <div
          style={{
            position: "absolute",
            inset: 0,
            backgroundImage: "radial-gradient(rgba(255,255,255,0.03) 1px, transparent 1px)",
            backgroundSize: "24px 24px",
            pointerEvents: "none",
            zIndex: -1,
          }}
        />
      </div>
    </div>
  );
}
