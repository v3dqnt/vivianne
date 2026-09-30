import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Box, Stack, Typography, IconButton, InputBase, Tooltip } from '../../../ui'
import { ArrowLeftLinear, ArrowRightLinear, RefreshLinear, GlobalLinear } from '../../../icons'
import { call, hasTauri } from '../../../ipc'

/** Mirrors `live_browser::Rect` — CSS pixels relative to the window. */
interface Rect {
  x: number
  y: number
  width: number
  height: number
}

const HOME = 'https://duckduckgo.com'

/**
 * A real browser, embedded.
 *
 * Unlike the agent view — which is a ~3fps feed of PNGs, because camofox is
 * headless Firefox behind a REST API with no streaming route — this is an
 * actual webview doing a top-level navigation. It renders, scrolls and types
 * natively, and because it is not an iframe, `X-Frame-Options` does not apply:
 * the sites that forced the screenshot approach in the first place (Google,
 * login pages) load here normally.
 *
 * The catch is that a child webview is an OS-level view layered over the
 * window at a pixel rectangle. It knows nothing about CSS layout, so this
 * component's whole job is to keep telling the backend where it lives — on
 * mount, on every resize, and on unmount — and to move it off-screen the moment
 * it should not be visible. Get that wrong and a browser floats above the rest
 * of the UI.
 */
export function LiveBrowserPanel({ active }: { active: boolean }) {
  const slotRef = useRef<HTMLDivElement | null>(null)
  const [url, setUrl] = useState(HOME)
  const [draft, setDraft] = useState('')
  const [error, setError] = useState<string | null>(null)
  // Held in a ref, not state: the reposition loop reads it on every resize tick
  // and must not re-subscribe each time.
  const urlRef = useRef(url)
  urlRef.current = url

  const measure = useCallback((): Rect | null => {
    const el = slotRef.current
    if (!el) return null
    const r = el.getBoundingClientRect()
    // getBoundingClientRect is relative to the main webview's viewport, which
    // is exactly the window's content area — the same origin the backend
    // positions a child webview against.
    return { x: r.left, y: r.top, width: r.width, height: r.height }
  }, [])

  const place = useCallback(async () => {
    if (!hasTauri()) return
    const rect = measure()
    if (!rect) return
    try {
      await call('cmd_live_browser_show', { url: urlRef.current, rect })
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }, [measure])

  // Position on mount and whenever the slot changes size. `useLayoutEffect` so
  // the first placement uses the settled layout rather than a pre-layout rect
  // of zero, which would be treated as hidden.
  useLayoutEffect(() => {
    if (!active) {
      void call('cmd_live_browser_hide').catch(() => {})
      return
    }
    void place()
    const el = slotRef.current
    if (!el) return
    const observer = new ResizeObserver(() => void place())
    observer.observe(el)
    // A window resize moves the slot without changing its size, which a
    // ResizeObserver on the element alone would miss entirely.
    const onWindowResize = () => void place()
    window.addEventListener('resize', onWindowResize)
    return () => {
      observer.disconnect()
      window.removeEventListener('resize', onWindowResize)
    }
  }, [active, place])

  // Leaving the tab, closing the dock, or unmounting must park the view
  // off-screen — it is layered over the window, so it would otherwise keep
  // painting on top of whatever replaced this panel.
  useEffect(() => {
    return () => {
      void call('cmd_live_browser_hide').catch(() => {})
    }
  }, [])

  // The page navigates itself as links are clicked, so the address bar has to
  // read the real URL back rather than trust what we last asked for.
  useEffect(() => {
    if (!active || !hasTauri()) return
    const id = setInterval(() => {
      void call<string | null>('cmd_live_browser_url')
        .then((u) => {
          if (u && u !== urlRef.current) setUrl(u)
        })
        .catch(() => {})
    }, 1000)
    return () => clearInterval(id)
  }, [active])

  const go = (raw: string) => {
    const t = raw.trim()
    if (!t) return
    const next = /^https?:\/\//.test(t) ? t : `https://${t}`
    setUrl(next)
    urlRef.current = next
    void place()
    setDraft('')
  }

  const nav = (js: string) => () => {
    void call('cmd_live_browser_eval', { js }).catch(() => {})
  }

  return (
    <Stack sx={{ flex: 1, minHeight: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5, p: 1, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        <Tooltip title="Back">
          <IconButton size="small" onClick={nav('history.back()')}>
            <ArrowLeftLinear size={14} color="currentColor" />
          </IconButton>
        </Tooltip>
        <Tooltip title="Forward">
          <IconButton size="small" onClick={nav('history.forward()')}>
            <ArrowRightLinear size={14} color="currentColor" />
          </IconButton>
        </Tooltip>
        <Tooltip title="Reload">
          <IconButton size="small" onClick={nav('location.reload()')}>
            <RefreshLinear size={13} color="currentColor" />
          </IconButton>
        </Tooltip>
        <GlobalLinear size={12} color="currentColor" />
        <InputBase
          fullWidth
          value={draft || url}
          onChange={(e) => setDraft(e.target.value)}
          onFocus={() => setDraft(url)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') go(draft)
          }}
          sx={{ fontSize: '0.8125rem', fontFamily: '"Geist Mono", ui-monospace, monospace', bgcolor: 'action.hover', borderRadius: 999, px: 1.5, py: 0.375 }}
        />
      </Stack>

      {error && (
        <Typography variant="caption" color="error.main" sx={{ px: 1.5, py: 0.75 }}>
          {error}
        </Typography>
      )}

      {/* The webview is layered over this box by the OS, not rendered into it.
          Nothing drawn here will ever be visible while the view is placed, so
          it holds only the fallback for when it isn't. */}
      <Box ref={slotRef} sx={{ flex: 1, minHeight: 0, bgcolor: 'background.default' }}>
        {!hasTauri() && (
          <Stack sx={{ height: '100%', alignItems: 'center', justifyContent: 'center', p: 3 }}>
            <Typography variant="caption" color="text.secondary" sx={{ textAlign: 'center' }}>
              The embedded browser needs the desktop app.
            </Typography>
          </Stack>
        )}
      </Box>
    </Stack>
  )
}
