import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Box, Stack, Typography, IconButton, InputBase, Tooltip } from '../../../ui'
import { ArrowLeftLinear, ArrowRightLinear, RefreshLinear, GlobalLinear } from '../../../icons'
import { call, hasTauri } from '../../../ipc'
import { useBrowserStore } from './browserStore'
import type { BrowserTab } from './tabs'

export type WebTabModel = Extract<BrowserTab, { kind: 'web' }>

interface Rect {
  x: number
  y: number
  width: number
  height: number
}

/**
 * One web tab: a child webview laid over the slot at a pixel rectangle, which
 * this component keeps telling the backend about. Only the active tab is ever
 * placed; every other tab is parked off-screen and never destroyed, so its
 * page, scroll position and form state survive a tab switch.
 */
export function WebTab({ tab, active }: { tab: WebTabModel; active: boolean }) {
  const slotRef = useRef<HTMLDivElement | null>(null)
  const [error, setError] = useState<string | null>(null)

  const place = useCallback(async () => {
    if (!hasTauri()) return
    const r = slotRef.current?.getBoundingClientRect()
    if (!r || r.width < 8 || r.height < 8) {
      await call('cmd_live_browser_hide', { label: tab.webviewLabel }).catch(() => {})
      return
    }
    const rect: Rect = { x: r.left, y: r.top, width: r.width, height: r.height }
    try {
      await call('cmd_live_browser_show', { label: tab.webviewLabel, url: tab.url, rect })
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }, [tab.url, tab.webviewLabel])

  const placeRef = useRef(place)
  placeRef.current = place

  // Placement follows visibility and size, not the address: navigating is the
  // separate effect below, so a reload never tears the view down.
  useLayoutEffect(() => {
    if (!active) {
      void call('cmd_live_browser_hide', { label: tab.webviewLabel }).catch(() => {})
      return
    }
    void placeRef.current()
    const el = slotRef.current
    if (!el) return
    const ro = new ResizeObserver(() => void placeRef.current())
    ro.observe(el)
    const onResize = () => void placeRef.current()
    window.addEventListener('resize', onResize)
    return () => {
      ro.disconnect()
      window.removeEventListener('resize', onResize)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, tab.webviewLabel])

  useEffect(() => {
    if (active) void placeRef.current()
  }, [active, tab.url])

  // The view is an OS-level layer: leaving it behind would paint a page over
  // whatever replaced this tab.
  useEffect(() => {
    return () => {
      void call('cmd_live_browser_hide', { label: tab.webviewLabel }).catch(() => {})
    }
  }, [tab.webviewLabel])

  // The page navigates itself as links are clicked, so the tab's label and
  // address are read back rather than assumed.
  useEffect(() => {
    if (!active || !hasTauri()) return
    let alive = true
    const id = window.setInterval(() => {
      void (async () => {
        const url = await call<string | null>('cmd_live_browser_url', { label: tab.webviewLabel }).catch(() => null)
        if (!alive) return
        if (url) useBrowserStore.getState().setUrl(tab.id, url)
        const title = await call<string | null>('cmd_live_browser_eval', { label: tab.webviewLabel, js: 'document.title' }).catch(() => null)
        if (!alive) return
        if (title) useBrowserStore.getState().setTitle(tab.id, title)
      })()
    }, 1000)
    return () => {
      alive = false
      window.clearInterval(id)
    }
  }, [active, tab.id, tab.webviewLabel])

  return (
    <>
      {error && (
        <Typography variant="caption" color="error.main" sx={{ position: 'absolute', top: 4, left: 8, right: 8, zIndex: 1 }}>
          {error}
        </Typography>
      )}
      {/* The webview is layered over this box by the OS. Nothing drawn here is
          visible while it is placed, so it holds only the browser-less case. */}
      <Box ref={slotRef} sx={{ position: 'absolute', inset: 0, display: active ? 'block' : 'none' }}>
        {!hasTauri() && (
          <Stack sx={{ height: '100%', alignItems: 'center', justifyContent: 'center', p: 3 }}>
            <Typography variant="caption" color="text.secondary" sx={{ textAlign: 'center' }}>
              The embedded browser needs the desktop app.
            </Typography>
          </Stack>
        )}
      </Box>
    </>
  )
}

/** The toolbar above the slot: history, reload, address, open externally. */
export function WebToolbar({ tab }: { tab: WebTabModel }) {
  const [draft, setDraft] = useState<string | null>(null)
  const setUrl = useBrowserStore((s) => s.setUrl)

  const nav = (js: string) => () => {
    void call('cmd_live_browser_eval', { label: tab.webviewLabel, js }).catch(() => {})
  }

  const go = (raw: string) => {
    const t = raw.trim()
    if (!t) return
    setUrl(tab.id, /^https?:\/\//.test(t) ? t : `https://${t}`)
    setDraft(null)
  }

  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 0.25, px: 1, py: 0.75, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
      <Tooltip title="Back">
        <IconButton size="small" aria-label="Back" onClick={nav('history.back()')} sx={{ width: 24, height: 24 }}>
          <ArrowLeftLinear size={13} color="currentColor" />
        </IconButton>
      </Tooltip>
      <Tooltip title="Forward">
        <IconButton size="small" aria-label="Forward" onClick={nav('history.forward()')} sx={{ width: 24, height: 24 }}>
          <ArrowRightLinear size={13} color="currentColor" />
        </IconButton>
      </Tooltip>
      <Tooltip title="Reload">
        <IconButton size="small" aria-label="Reload" onClick={nav('location.reload()')} sx={{ width: 24, height: 24 }}>
          <RefreshLinear size={12} color="currentColor" />
        </IconButton>
      </Tooltip>
      <GlobalLinear size={12} color="currentColor" />
      <InputBase
        fullWidth
        value={draft ?? tab.url}
        onChange={(e) => setDraft(e.target.value)}
        onFocus={() => setDraft(tab.url)}
        onBlur={() => setDraft(null)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') go(draft ?? tab.url)
          if (e.key === 'Escape') setDraft(null)
        }}
        sx={{ fontSize: '0.75rem', fontFamily: '"Geist Mono", ui-monospace, monospace', bgcolor: 'action.hover', borderRadius: 999, px: 1.5, py: 0.375 }}
      />
      <Tooltip title="Open in your browser">
        <IconButton size="small" aria-label="Open externally" onClick={() => void call('cmd_open_path', { path: tab.url })} sx={{ width: 24, height: 24 }}>
          <GlobalLinear size={12} color="currentColor" />
        </IconButton>
      </Tooltip>
    </Stack>
  )
}
