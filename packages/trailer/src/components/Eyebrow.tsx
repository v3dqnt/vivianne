export function Eyebrow({ children, style }: { children: React.ReactNode; style?: React.CSSProperties }) {
  return (
    <span
      style={{
        display: "inline-block",
        fontSize: 30,
        fontWeight: 500,
        textTransform: "uppercase",
        letterSpacing: "0.28em",
        color: "var(--mute)",
        fontFamily: "Sora",
        ...style,
      }}
    >
      {children}
    </span>
  );
}
