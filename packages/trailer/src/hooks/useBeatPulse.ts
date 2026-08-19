import { useCurrentFrame, useVideoConfig } from "remotion";
import beatgrid from "../data/beatgrid.json";

const beatFrames = beatgrid.beatTimesSec.map((t) => t * 30);

/**
 * Returns a 0→1→0 pulse value driven by the nearest detected beat to the
 * current frame — a subtle scale/opacity bump lands in sync with the actual
 * track instead of an arbitrary loop. `decayFrames` controls how fast the
 * pulse fades back to rest after each hit.
 */
export function useBeatPulse(decayFrames = 6, strength = 1) {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const scale = fps / 30; // beat grid was computed at 30fps
  let nearestDist = Infinity;
  for (const bf of beatFrames) {
    const d = Math.abs(frame - bf * scale);
    if (d < nearestDist) nearestDist = d;
  }
  const decay = decayFrames * scale;
  if (nearestDist > decay) return 0;
  return (1 - nearestDist / decay) * strength;
}

export function getBeatFramesInRange(startFrame: number, endFrame: number, fps: number) {
  const scale = fps / 30;
  return beatgrid.beatTimesSec
    .map((t) => t * 30 * scale)
    .filter((f) => f >= startFrame && f <= endFrame);
}

export { beatgrid };
