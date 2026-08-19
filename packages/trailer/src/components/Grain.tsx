import { useCurrentFrame } from "remotion";

/**
 * Fixed full-frame film grain overlay — matches the site's texture spec
 * (opacity 0.03, blend-mode overlay) so the video reads as photographic
 * rather than flat digital output. Re-seeds the turbulence pattern every few
 * frames so the grain feels alive without being distracting.
 */
export function Grain() {
  const frame = useCurrentFrame();
  const seed = Math.floor(frame / 3) % 100;

  return (
    <svg
      style={{
        position: "absolute",
        inset: 0,
        width: "100%",
        height: "100%",
        opacity: 0.05,
        mixBlendMode: "overlay",
        pointerEvents: "none",
      }}
    >
      <filter id="grain">
        <feTurbulence
          type="fractalNoise"
          baseFrequency="0.9"
          numOctaves="2"
          seed={seed}
          stitchTiles="stitch"
        />
        <feColorMatrix type="saturate" values="0" />
      </filter>
      <rect width="100%" height="100%" filter="url(#grain)" />
    </svg>
  );
}
