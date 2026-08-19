import { interpolate, useCurrentFrame, useVideoConfig } from "remotion";

type SignalFieldProps = {
  accent?: "violet" | "cyan" | "coral";
  intensity?: number;
};

const palettes = {
  violet: ["#5B4BFF", "#A78BFA", "#FF5B8A"],
  cyan: ["#04BFE7", "#5B4BFF", "#B5F25A"],
  coral: ["#FF5B6B", "#FFB347", "#5B4BFF"],
};

/** A composited layer of animated editorial guides and colour signals. */
export const SignalField: React.FC<SignalFieldProps> = ({
  accent = "violet",
  intensity = 1,
}) => {
  const frame = useCurrentFrame();
  const { durationInFrames } = useVideoConfig();
  const [primary, secondary, tertiary] = palettes[accent];
  const sweep = interpolate(frame, [0, durationInFrames], [-420, 2560], {
    extrapolateRight: "clamp",
  });
  const counterSweep = interpolate(frame, [0, durationInFrames], [2460, -360], {
    extrapolateRight: "clamp",
  });
  const breath = 0.75 + Math.sin(frame / 11) * 0.25;

  return (
    <svg
      viewBox="0 0 2160 3840"
      preserveAspectRatio="none"
      style={{ position: "absolute", inset: 0, width: "100%", height: "100%", pointerEvents: "none" }}
    >
      <defs>
        <linearGradient id={`signal-${accent}`} x1="0" x2="1">
          <stop stopColor={primary} stopOpacity="0" />
          <stop offset="0.45" stopColor={primary} stopOpacity="0.75" />
          <stop offset="1" stopColor={secondary} stopOpacity="0" />
        </linearGradient>
      </defs>
      <path d={`M -100 ${sweep} L 2260 ${sweep - 720}`} stroke={`url(#signal-${accent})`} strokeWidth="7" opacity={0.65 * intensity} />
      <path d={`M -120 ${counterSweep} L 2280 ${counterSweep + 560}`} stroke={secondary} strokeWidth="2" opacity={0.38 * intensity} />
      <path d={`M 120 ${counterSweep - 800} L 2020 ${counterSweep - 800}`} stroke={tertiary} strokeWidth="3" strokeDasharray="26 28" opacity={0.35 * intensity} />
      <circle cx="1850" cy={Math.max(260, Math.min(3570, sweep + 400))} r={42 + breath * 18} fill={primary} opacity={0.12 * intensity} />
      <circle cx="1850" cy={Math.max(260, Math.min(3570, sweep + 400))} r="9" fill={primary} opacity={0.9 * intensity} />
      <rect x="120" y="2940" width="220" height="14" fill={tertiary} opacity={0.7 * intensity} transform={`translate(${Math.sin(frame / 16) * 48} 0)`} />
      <rect x="1760" y="640" width="260" height="8" fill={secondary} opacity={0.65 * intensity} transform={`translate(${Math.cos(frame / 17) * 60} 0)`} />
    </svg>
  );
};
