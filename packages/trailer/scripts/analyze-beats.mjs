// Beat/onset analysis for the trailer's background track.
//
// Decodes the mp3 to raw mono PCM via ffmpeg, computes a short-time energy
// envelope, picks onsets via an adaptive-threshold energy-flux detector,
// estimates BPM from the inter-onset-interval histogram, and snaps onsets to
// a regular beat grid. Also scans the full track for the highest-energy
// contiguous ~22s window to use as the trailer's excerpt.
//
// Output: public/audio/beatgrid.json — consumed by the Remotion composition
// to drive scene cuts / pulses in sync with the actual track.

import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const SRC = path.join(__dirname, "..", "public", "audio", "theme.mp3");
const OUT_JSON = path.join(__dirname, "..", "public", "audio", "beatgrid.json");

const SR = 22050; // analysis sample rate — plenty for onset/beat detection

function decodeToMonoF32(file) {
  return new Promise((resolve, reject) => {
    const ff = spawn("ffmpeg", [
      "-i", file,
      "-f", "f32le",
      "-ac", "1",
      "-ar", String(SR),
      "-loglevel", "error",
      "-",
    ]);
    const chunks = [];
    ff.stdout.on("data", (d) => chunks.push(d));
    ff.stderr.on("data", (d) => process.stderr.write(d));
    ff.on("error", reject);
    ff.on("close", (code) => {
      if (code !== 0) return reject(new Error(`ffmpeg exited ${code}`));
      const buf = Buffer.concat(chunks);
      const samples = new Float32Array(buf.buffer, buf.byteOffset, buf.length / 4);
      resolve(samples);
    });
  });
}

function shortTimeEnergy(samples, sr, hopMs = 10, winMs = 23) {
  const hop = Math.round((sr * hopMs) / 1000);
  const win = Math.round((sr * winMs) / 1000);
  const frames = Math.floor((samples.length - win) / hop);
  const energy = new Float32Array(frames);
  for (let i = 0; i < frames; i++) {
    const start = i * hop;
    let sum = 0;
    for (let j = 0; j < win; j++) {
      const s = samples[start + j];
      sum += s * s;
    }
    energy[i] = Math.sqrt(sum / win);
  }
  return { energy, hopSec: hop / sr };
}

// Energy-flux onset strength: positive part of the derivative of a smoothed
// energy curve — a simple, dependency-free stand-in for spectral flux that
// works well on percussive/kick-driven tracks.
function onsetStrength(energy) {
  // Light smoothing (3-tap moving average) to suppress single-frame jitter.
  const smoothed = new Float32Array(energy.length);
  for (let i = 0; i < energy.length; i++) {
    const a = energy[Math.max(0, i - 1)];
    const b = energy[i];
    const c = energy[Math.min(energy.length - 1, i + 1)];
    smoothed[i] = (a + b + c) / 3;
  }
  const flux = new Float32Array(smoothed.length);
  for (let i = 1; i < smoothed.length; i++) {
    flux[i] = Math.max(0, smoothed[i] - smoothed[i - 1]);
  }
  return flux;
}

function pickPeaks(flux, hopSec, minIntervalSec = 0.2) {
  const minGap = Math.round(minIntervalSec / hopSec);
  // Adaptive local threshold: mean + k * local std over a rolling window.
  const win = Math.round(1.0 / hopSec); // 1s rolling window
  const peaks = [];
  let lastPeak = -Infinity;
  for (let i = 0; i < flux.length; i++) {
    const lo = Math.max(0, i - win);
    const hi = Math.min(flux.length, i + win);
    let sum = 0;
    let sumSq = 0;
    for (let j = lo; j < hi; j++) {
      sum += flux[j];
      sumSq += flux[j] * flux[j];
    }
    const n = hi - lo;
    const mean = sum / n;
    const variance = Math.max(0, sumSq / n - mean * mean);
    const std = Math.sqrt(variance);
    const threshold = mean + 1.4 * std;
    if (flux[i] > threshold && flux[i] > 0 && i - lastPeak >= minGap) {
      // local max check within a small neighborhood
      let isMax = true;
      for (let j = Math.max(0, i - 3); j <= Math.min(flux.length - 1, i + 3); j++) {
        if (flux[j] > flux[i]) { isMax = false; break; }
      }
      if (isMax) {
        peaks.push(i);
        lastPeak = i;
      }
    }
  }
  return peaks;
}

function estimateBpm(peakTimesSec) {
  const iois = [];
  for (let i = 1; i < peakTimesSec.length; i++) {
    const d = peakTimesSec[i] - peakTimesSec[i - 1];
    if (d > 0.25 && d < 1.5) iois.push(d); // plausible single/double-beat range (40-240bpm)
  }
  if (iois.length < 4) return { bpm: 100, period: 0.6 };
  // Histogram IOIs into 10ms bins, find the dominant bin, refine with mean of that cluster.
  const binSize = 0.01;
  const bins = new Map();
  for (const d of iois) {
    const b = Math.round(d / binSize);
    bins.set(b, (bins.get(b) ?? 0) + 1);
  }
  let bestBin = 0;
  let bestCount = 0;
  for (const [b, c] of bins) {
    if (c > bestCount) { bestCount = c; bestBin = b; }
  }
  const clusterVals = iois.filter((d) => Math.abs(Math.round(d / binSize) - bestBin) <= 2);
  const period = clusterVals.reduce((a, b) => a + b, 0) / clusterVals.length;
  let bpm = 60 / period;
  // Fold into a sane 70-170bpm range (halve/double octave errors).
  while (bpm > 170) bpm /= 2;
  while (bpm < 70) bpm *= 2;
  return { bpm, period: 60 / bpm };
}

function buildBeatGrid(peakTimesSec, period, trackDurationSec) {
  if (peakTimesSec.length === 0) return [];
  // Anchor on the earliest strong peak, then lay a regular grid across the
  // whole track and snap to the nearest detected onset within a tolerance
  // (keeps the grid musically regular while staying phase-locked to real hits).
  const anchor = peakTimesSec[0];
  const grid = [];
  const tolerance = period * 0.35;
  for (let t = anchor % period; t < trackDurationSec; t += period) {
    // find nearest real onset to snap to, if any within tolerance
    let nearest = null;
    let nearestDist = Infinity;
    for (const p of peakTimesSec) {
      const d = Math.abs(p - t);
      if (d < nearestDist) { nearestDist = d; nearest = p; }
    }
    grid.push(nearest !== null && nearestDist <= tolerance ? nearest : t);
  }
  return grid;
}

function findBestWindow(energy, hopSec, windowSec, trackDurationSec) {
  const windowFrames = Math.round(windowSec / hopSec);
  let best = { start: 0, score: -Infinity };
  // Precompute prefix sums for O(1) window sums.
  const prefix = new Float64Array(energy.length + 1);
  for (let i = 0; i < energy.length; i++) prefix[i + 1] = prefix[i] + energy[i];
  for (let i = 0; i + windowFrames < energy.length; i += Math.round(0.5 / hopSec)) {
    const sum = prefix[i + windowFrames] - prefix[i];
    const avg = sum / windowFrames;
    const tSec = i * hopSec;
    // Prefer energetic windows that aren't in the very first 3s (avoid a cold
    // start with no buildup) or the very last 5s (avoid trailing fade-out).
    if (tSec < 3 || tSec + windowSec > trackDurationSec - 5) continue;
    if (avg > best.score) best = { start: tSec, score: avg };
  }
  return best.start;
}

async function main() {
  console.log("Decoding audio...");
  const samples = await decodeToMonoF32(SRC);
  const trackDurationSec = samples.length / SR;
  console.log(`Duration: ${trackDurationSec.toFixed(1)}s`);

  const { energy, hopSec } = shortTimeEnergy(samples, SR);
  const flux = onsetStrength(energy);
  const peakIdx = pickPeaks(flux, hopSec);
  const peakTimesSec = peakIdx.map((i) => i * hopSec);
  console.log(`Detected ${peakTimesSec.length} onsets`);

  const { bpm, period } = estimateBpm(peakTimesSec);
  console.log(`Estimated BPM: ${bpm.toFixed(1)} (period ${period.toFixed(3)}s)`);

  const EXCERPT_SEC = 22;
  const sectionStartSec = findBestWindow(energy, hopSec, EXCERPT_SEC, trackDurationSec);
  console.log(`Best ${EXCERPT_SEC}s window starts at ${sectionStartSec.toFixed(2)}s`);

  const fullGrid = buildBeatGrid(peakTimesSec, period, trackDurationSec);
  const sectionEndSec = sectionStartSec + EXCERPT_SEC;
  const beatsInSection = fullGrid
    .filter((t) => t >= sectionStartSec && t <= sectionEndSec)
    .map((t) => +(t - sectionStartSec).toFixed(3));

  const out = {
    bpm: +bpm.toFixed(2),
    period: +period.toFixed(4),
    trackDurationSec: +trackDurationSec.toFixed(2),
    sectionStartSec: +sectionStartSec.toFixed(2),
    sectionEndSec: +sectionEndSec.toFixed(2),
    beatTimesSec: beatsInSection,
  };
  writeFileSync(OUT_JSON, JSON.stringify(out, null, 2));
  console.log(`Wrote ${OUT_JSON}`);
  console.log(`${beatsInSection.length} beats in the chosen excerpt`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
