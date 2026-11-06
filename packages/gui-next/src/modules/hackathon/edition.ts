import { create } from 'zustand'
import { loadPersisted, savePersisted } from '../../lib/persist'
import type { DiscoveredModel } from '../ai/lib/types'

/**
 * The NVIDIA × Nebius hackathon edition. Everything specific to it lives
 * under `modules/hackathon`, so the rest of the app is the same app and the
 * edition can be lifted out without touching it.
 */
export const EDITION = { name: 'NVIDIA × Nebius edition', short: 'NVIDIA × Nebius' } as const

const KEY = 'viv.hackathon'

interface QuickstartState {
  open: boolean
  /** The user finished (or dismissed) the quickstart. */
  done: boolean
  show: () => void
  close: (finished?: boolean) => void
}

const saved = loadPersisted<{ done?: boolean }>(KEY, {})

export const useQuickstart = create<QuickstartState>((set) => ({
  open: false,
  done: !!saved.done,
  show: () => set({ open: true }),
  close: (finished) => {
    set((s) => ({ open: false, done: finished ? true : s.done }))
    if (finished) savePersisted(KEY, { done: true })
  },
}))

/** Models, by role, from what a Nebius account lists. Ids come from the
 * account (they vary), so roles are found by name. */
export function pickNemotron(models: DiscoveredModel[]): { smart: DiscoveredModel | null; quick: DiscoveredModel | null; super: DiscoveredModel | null } {
  const nebius = models.filter((m) => m.provider === 'nebius')
  const find = (re: RegExp, not?: RegExp) => nebius.find((m) => re.test(m.id) && !(not && not.test(m.id))) ?? null
  const ultra = find(/nemotron[^/]*ultra/i)
  const sup = find(/nemotron[^/]*super/i)
  const nano = find(/nemotron[^/]*nano/i, /omni|vl/i)
  const anyNemotron = find(/nemotron/i, /omni|vl|embed|rerank|guard|safety/i)
  return {
    smart: ultra ?? sup ?? anyNemotron,
    quick: nano ?? (sup && sup !== (ultra ?? sup) ? sup : null) ?? null,
    super: sup,
  }
}
