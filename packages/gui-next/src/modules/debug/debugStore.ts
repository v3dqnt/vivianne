import { create } from 'zustand'
import { call } from '../../ipc'
import type { DebugSession } from './components/DebugPanel'

interface DebugState {
  /** What the backend reports for the watched thread; null when it is not debugging anything. */
  session: DebugSession | null
  frame: number | null
  selectFrame: (thread: string, id: number) => void
  refresh: (thread: string) => Promise<void>
  control: (thread: string, action: string, at?: { file: string; line: number }) => Promise<void>
}

export const useDebug = create<DebugState>((set, get) => ({
  session: null,
  frame: null,
  selectFrame: (thread, id) => {
    set({ frame: id })
    void get().refresh(thread)
  },
  refresh: async (thread) => {
    try {
      const snap = await call<DebugSession | null>('cmd_debug_snapshot', { threadId: thread, frameId: get().frame })
      set({ session: snap ?? null })
      if (!snap) set({ frame: null })
    } catch {
      /* a failed read keeps what was shown */
    }
  },
  control: async (thread, action, at) => {
    try {
      await call('cmd_debug_control', { threadId: thread, action, file: at?.file, line: at?.line })
    } finally {
      // The program takes a moment to stop or end; the next polls pick it up, this one shows the click landed.
      window.setTimeout(() => void get().refresh(thread), 250)
    }
  },
}))

const WATCH_MS = 1500

/** Keep the panel's view of a thread's debug session fresh while something uses it. Returns a stop function. */
export function watchDebug(thread: string): () => void {
  void useDebug.getState().refresh(thread)
  const t = window.setInterval(() => void useDebug.getState().refresh(thread), WATCH_MS)
  return () => window.clearInterval(t)
}
