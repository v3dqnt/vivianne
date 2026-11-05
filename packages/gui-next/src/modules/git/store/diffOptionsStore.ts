import { create } from 'zustand'
import { loadPersisted, savePersisted } from '../../../lib/persist'
import { DEFAULT_DIFF_OPTIONS, type DiffOptions } from '../lib/diffView'

const KEY = 'viv.git.diffOptions'

interface DiffOptionsState extends DiffOptions {
  set: (patch: Partial<DiffOptions>) => void
}

/** The diff view's settings, kept across restarts: a choice made once about
 * how diffs read should not have to be made again. */
export const useDiffOptions = create<DiffOptionsState>((set, get) => ({
  ...DEFAULT_DIFF_OPTIONS,
  ...loadPersisted<Partial<DiffOptions>>(KEY, {}),
  set: (patch) => {
    set(patch)
    const { set: _set, ...options } = get()
    savePersisted(KEY, options)
  },
}))
