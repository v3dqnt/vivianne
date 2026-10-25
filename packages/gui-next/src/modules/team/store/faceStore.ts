import { create } from 'zustand'
import { loadPersisted, savePersisted } from '../../../lib/persist'

/** An agent's head beyond its colour: outline, trim (ears, antenna collar)
 * and LED colour. Mirrors `agent::team::FaceStyle`; unset parts fall back to
 * defaults picked from the agent's name. */
export interface FaceStyle {
  shape?: 'rectangle' | 'square' | 'circle' | 'hexagon'
  trim?: string
  screen?: string
}

/** How every head behaves, and Vivianne's own (Settings → Appearance). */
interface FacePrefs {
  /** Multiplies every avatar's size: 0.85–1.25. */
  scale: number
  /** When heads move: only while busy (default), always, or never. */
  motion: 'busy' | 'always' | 'never'
  /** Vivianne's head, in the notch and as the lead. */
  viv: FaceStyle & { color?: string }
  update: (patch: Partial<Pick<FacePrefs, 'scale' | 'motion' | 'viv'>>) => void
}

const KEY = 'viv.facePrefs'
type Saved = Partial<Pick<FacePrefs, 'scale' | 'motion' | 'viv'>>
const saved = loadPersisted<Saved>(KEY, {})

/** What's persisted, with its defaults, from whatever was saved. */
const withDefaults = (s: Saved) => ({ scale: s.scale ?? 1, motion: s.motion ?? ('busy' as const), viv: s.viv ?? {} })

export const useFacePrefs = create<FacePrefs>((set, get) => ({
  ...withDefaults(saved),
  update: (patch) => {
    set(patch)
    const { scale, motion, viv } = { ...get(), ...patch }
    savePersisted(KEY, { scale, motion, viv })
  },
}))

// The notch is another window: follow changes made in Settings.
if (typeof window !== 'undefined') {
  window.addEventListener('storage', (e) => {
    if (e.key !== KEY) return
    useFacePrefs.setState(withDefaults(loadPersisted<Saved>(KEY, {})))
  })
}
