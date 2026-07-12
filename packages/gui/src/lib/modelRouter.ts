// Auto model selection — picks the best available model for a given prompt
// from the already-discovered catalog (ModelInfo[], including the capability
// labels from model_discovery.rs's classify_capabilities).
//
// ponytail: this is a rule-based heuristic (task-complexity signal + a
// name-pattern tier lookup), not a learned router or an LLM-judged one — that
// would mean an extra API call (cost + latency) before every real one just to
// pick a model. Runs in a few microseconds client-side instead. Upgrade path:
// an actual routing model if the heuristic picks visibly wrong often enough
// to matter.

import type { ModelInfo } from "@/components/AgentProvider";

type Tier = "flagship" | "mid" | "fast";

/** Rough capability tier from the model id/name — same pattern-matching
 *  approach as the backend's classify_capabilities, kept in sync loosely
 *  (exact parity isn't required; this only needs to rank candidates). */
function tierOf(model: ModelInfo): Tier {
  const s = `${model.id} ${model.name}`.toLowerCase();

  const flagship = [
    "opus", "gpt-4o", "gpt-4.1", "gpt-5", "o1-preview", "o3", "grok-4", "grok-3",
    "gemini-1.5-pro", "gemini-2.0-pro", "gemini-2.5-pro", "gemini-pro",
    "deepseek-r1", "deepseek-v3", "llama-3.1-405b", "qwen2.5-72b", "qwen-72b",
  ];
  const fast = [
    "haiku", "mini", "flash", "nano", "-8b", "-7b", "-3b", "-1b", "small",
    "lite", "gpt-3.5", "gemma", "phi-3",
  ];

  if (flagship.some((p) => s.includes(p))) return "flagship";
  if (fast.some((p) => s.includes(p))) return "fast";
  return "mid";
}

/** 0 (trivial) .. 1 (complex) — from message length, code fences, and a few
 *  keywords that tend to correlate with wanting the strongest model. */
function estimateComplexity(prompt: string): number {
  const len = prompt.trim().length;
  const hasCodeFence = /```/.test(prompt);
  const hasMultiStepWords = /\b(refactor|architect|migrate|design|debug|analyze|analyse|investigate|implement|build|comprehensive)\b/i.test(prompt);

  let score = 0;
  score += Math.min(len / 800, 0.5); // long prompts lean complex, caps at 0.5
  if (hasCodeFence) score += 0.2;
  if (hasMultiStepWords) score += 0.3;
  return Math.min(score, 1);
}

const TIER_TARGET_SCORE: Record<Tier, (complexity: number) => number> = {
  flagship: (c) => c, // best for high complexity
  fast: (c) => 1 - c, // best for low complexity
  mid: () => 0.6, // reasonable default, never the top pick either way
};

export interface ChooseModelInput {
  models: ModelInfo[];
  prompt: string;
  hasImages: boolean;
}

/** Pick the best model for `prompt` from `models`, or `null` if nothing
 *  qualifies (e.g. images attached but no vision-capable model available). */
export function chooseBestModel({ models, prompt, hasImages }: ChooseModelInput): ModelInfo | null {
  const complexity = estimateComplexity(prompt);

  const candidates = models.filter((m) => {
    if (m.isImageModel || m.isAudioModel) return false; // never valid as a chat model
    if (hasImages && !m.supportsVision) return false;
    return true;
  });
  if (candidates.length === 0) return null;

  let best: ModelInfo | null = null;
  let bestScore = -Infinity;
  for (const m of candidates) {
    const tier = tierOf(m);
    let score = TIER_TARGET_SCORE[tier](complexity);
    // Tie-break: prefer a larger context window (more headroom), then cheaper.
    score += Math.min((m.contextWindow ?? 0) / 2_000_000, 0.05);
    score -= Math.min((m.inputCostPerMillion ?? 0) / 1000, 0.05);
    if (score > bestScore) {
      bestScore = score;
      best = m;
    }
  }
  return best;
}
