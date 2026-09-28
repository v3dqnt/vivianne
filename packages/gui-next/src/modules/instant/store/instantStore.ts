import { create } from 'zustand'

/** Mirrors `instant::session::StepRecord` — `#[serde(rename_all = "camelCase")]`. */
export interface InstantStep {
  index: number
  tool: string
  summary: string
  status: 'running' | 'ok' | 'failed'
  detail: string | null
}

/**
 * Where a Viv Instant activation is in its lifecycle.
 *
 * Kept as one enum rather than a set of booleans because these are genuinely
 * exclusive — you cannot be listening and executing at once — and the overlay
 * renders a different thing for each. Booleans would allow states that cannot
 * happen and force every consumer to decide what they mean.
 */
export type InstantPhase =
  | 'idle'
  | 'listening'
  | 'transcribing'
  | 'working'
  | 'done'
  | 'error'

/**
 * A destructive step being held before it runs.
 *
 * Mirrors the `instant:risky` payload. `deadline` is stored as an absolute
 * timestamp rather than a remaining duration so the countdown stays correct
 * across re-renders and a backgrounded window, where an interval-decremented
 * counter would drift.
 */
export interface InstantRisk {
  summary: string
  /** Total length of the hold, for drawing the bar. */
  ms: number
  deadline: number
}

interface InstantState {
  phase: InstantPhase
  /** Final transcript of what the user said. */
  transcript: string
  /** Partial transcript while they are still speaking. */
  interim: string
  steps: InstantStep[]
  /** The model's closing line, spoken aloud. */
  summary: string | null
  error: string | null
  /** The destructive step currently counting down, if any. */
  risk: InstantRisk | null
  /** First-run speech-model download, if one is in progress. */
  download: { percent: number; index: number; count: number } | null

  activate: () => void
  dismiss: () => void
  setPhase: (phase: InstantPhase) => void
  setInterim: (interim: string) => void
  setTranscript: (transcript: string) => void
  upsertStep: (step: InstantStep) => void
  setRisk: (risk: InstantRisk | null) => void
  setDownload: (d: { percent: number; index: number; count: number } | null) => void
  finish: (summary: string) => void
  fail: (error: string) => void
}

const blank = {
  transcript: '',
  interim: '',
  steps: [] as InstantStep[],
  summary: null,
  error: null,
  risk: null,
  download: null,
}

export const useInstantStore = create<InstantState>((set) => ({
  phase: 'idle',
  ...blank,

  // Every activation starts clean. Viv Instant is a command, not a
  // conversation, so leaving the last run's steps on screen would make it
  // ambiguous whether what you are looking at just happened.
  activate: () => set({ phase: 'listening', ...blank }),
  dismiss: () => set({ phase: 'idle', ...blank }),

  setPhase: (phase) => set({ phase }),
  setInterim: (interim) => set({ interim }),
  setTranscript: (transcript) => set({ transcript, interim: '' }),

  // Steps arrive twice — once as `running`, once resolved — so this is an
  // upsert keyed on the backend's index rather than an append.
  upsertStep: (step) =>
    set((s) => {
      const at = s.steps.findIndex((x) => x.index === step.index)
      if (at === -1) return { steps: [...s.steps, step] }
      const steps = [...s.steps]
      steps[at] = step
      return { steps }
    }),

  setRisk: (risk) => set({ risk }),
  setDownload: (download) => set({ download }),

  // Both endings clear `risk`: a countdown cannot outlive the run it belongs
  // to, and a stale Cancel button after the fact would suggest the step could
  // still be stopped when it has already happened.
  finish: (summary) => set({ phase: 'done', summary, risk: null }),
  fail: (error) => set({ phase: 'error', error, risk: null }),
}))
