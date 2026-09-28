import { create } from 'zustand'
import { call } from '../../../ipc'

interface WebPreviewResult {
  screenshotBase64: string | null
}

interface WebPreviewStoreState {
  open: boolean
  url: string
  screenshotBase64: string | null
  error: string | null
  loading: boolean
  /** Set by `agentStore` when the `web_preview` tool fires — opens the panel. */
  requested: (url: string, screenshotBase64: string | null, error: string | null) => void
  close: () => void
  navigate: (url: string) => Promise<void>
  back: () => Promise<void>
  forward: () => Promise<void>
  refresh: () => Promise<void>
  /** Pull a fresh screenshot without touching the page. Safe to call on a
   * timer; see `startLivePolling`. */
  pollScreenshot: () => Promise<void>
  /** Begin/stop the live refresh loop. Reference-counted so two mounted
   * viewers don't fight over one interval. */
  startLivePolling: () => void
  stopLivePolling: () => void
  /** Click at a fractional position in the viewport (0..1 on each axis). */
  clickAt: (fx: number, fy: number) => Promise<void>
  /** Type into whatever the last click focused. */
  typeText: (text: string) => Promise<void>
  pressKey: (key: string) => Promise<void>
  scrollBy: (direction: 'up' | 'down' | 'left' | 'right', amount?: number) => Promise<void>
}

/** How often the panel pulls a new frame while it is open.
 *
 * The panel was purely event-driven: it only ever updated when a browser tool
 * happened to fire. Between calls — while a page loads, a redirect resolves, or
 * the model spends twenty seconds thinking — it showed a frozen frame, which
 * reads as the browser having hung.
 *
 * 1200ms is a compromise: fast enough to feel live, slow enough that a
 * screenshot round-trip per tick is not a meaningful load, and each frame
 * replaces the last so they cannot queue up. */
const LIVE_POLL_MS = 1200

let pollHandle: ReturnType<typeof setInterval> | null = null
let viewers = 0
/** Guards against overlapping polls: a slow screenshot must not stack up
 * behind the interval and arrive out of order. */
let polling = false

export const useWebPreviewStore = create<WebPreviewStoreState>((set, get) => ({
  open: false,
  url: '',
  screenshotBase64: null,
  error: null,
  loading: false,

  requested: (url, screenshotBase64, error) => {
    set({ open: true, url, screenshotBase64, error })
  },

  close: () => set({ open: false }),

  navigate: async (url) => {
    set({ loading: true, error: null })
    try {
      const result = await call<WebPreviewResult>('cmd_web_preview_navigate', { url })
      set({ url, screenshotBase64: result?.screenshotBase64 ?? get().screenshotBase64 })
    } catch (err) {
      set({ error: err instanceof Error ? err.message : String(err) })
    } finally {
      set({ loading: false })
    }
  },

  back: async () => {
    set({ loading: true })
    try {
      const result = await call<WebPreviewResult>('cmd_web_preview_back')
      if (result?.screenshotBase64) set({ screenshotBase64: result.screenshotBase64 })
    } finally {
      set({ loading: false })
    }
  },

  forward: async () => {
    set({ loading: true })
    try {
      const result = await call<WebPreviewResult>('cmd_web_preview_forward')
      if (result?.screenshotBase64) set({ screenshotBase64: result.screenshotBase64 })
    } finally {
      set({ loading: false })
    }
  },

  refresh: async () => {
    set({ loading: true })
    try {
      const result = await call<WebPreviewResult>('cmd_web_preview_refresh')
      if (result?.screenshotBase64) set({ screenshotBase64: result.screenshotBase64 })
    } finally {
      set({ loading: false })
    }
  },

  pollScreenshot: async () => {
    if (polling) return
    polling = true
    try {
      const shot = await call<string | null>('cmd_web_preview_screenshot')
      // Only replace on a real frame. A null means no tab is open or the
      // browser isn't up yet; blanking the panel for that would throw away the
      // last good view and look like a crash.
      if (shot) set({ screenshotBase64: shot })
    } catch {
      // Swallowed on purpose. This runs on a timer, so surfacing a transient
      // failure would replace the page with an error banner once a second.
    } finally {
      polling = false
    }
  },

  startLivePolling: () => {
    viewers += 1
    if (pollHandle) return
    pollHandle = setInterval(() => {
      // Don't poll a hidden window: a backgrounded app has no viewer to serve
      // and the browser throttles the timer anyway.
      if (typeof document !== 'undefined' && document.hidden) return
      void get().pollScreenshot()
    }, LIVE_POLL_MS)
  },

  stopLivePolling: () => {
    viewers = Math.max(0, viewers - 1)
    if (viewers === 0 && pollHandle) {
      clearInterval(pollHandle)
      pollHandle = null
    }
  },

  // Human input shares the agent's tab, so anything done here is visible to
  // the agent's next snapshot. Each call returns a fresh frame; applying it
  // immediately makes the click feel direct instead of waiting for the poll.
  clickAt: async (fx, fy) => {
    set({ loading: true })
    try {
      const shot = await call<string | null>('cmd_web_preview_click_at', { fx, fy })
      if (shot) set({ screenshotBase64: shot })
    } catch (err) {
      set({ error: err instanceof Error ? err.message : String(err) })
    } finally {
      set({ loading: false })
    }
  },

  typeText: async (text) => {
    if (!text) return
    try {
      const shot = await call<string | null>('cmd_web_preview_type', { text })
      if (shot) set({ screenshotBase64: shot })
    } catch (err) {
      set({ error: err instanceof Error ? err.message : String(err) })
    }
  },

  pressKey: async (key) => {
    try {
      const shot = await call<string | null>('cmd_web_preview_press', { key })
      if (shot) set({ screenshotBase64: shot })
    } catch (err) {
      set({ error: err instanceof Error ? err.message : String(err) })
    }
  },

  scrollBy: async (direction, amount = 400) => {
    try {
      const shot = await call<string | null>('cmd_web_preview_scroll', { direction, amount })
      if (shot) set({ screenshotBase64: shot })
    } catch (err) {
      set({ error: err instanceof Error ? err.message : String(err) })
    }
  },
}))
