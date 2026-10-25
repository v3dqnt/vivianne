import { useEffect, useMemo, useRef, useState } from 'react'
import { Box, Stack, Typography, Button } from '../../../ui'
import { isWindows } from '../../../lib/platform'
import { useThemeStore, themeOptions, activePaletteId } from '../../settings/store/themeStore'
import { themeTokens, FONTS, MONO_FAMILY } from '../../../theme'
import { useModsStore } from '../store/modsStore'
import { ModConnection, appHost, type HostApi } from './bridge'
import type { ModInfo, ModSlot } from '../lib/types'

/** A mod file's URL under the `vivmod` protocol on this platform. */
export function modUrl(key: string, entry: string): string {
  const path = entry.split('/').map(encodeURIComponent).join('/')
  return isWindows ? `http://vivmod.localhost/${key}/${path}` : `vivmod://localhost/${key}/${path}`
}

/** The theme as mod pages receive it. */
export function useModTheme() {
  const s = useThemeStore()
  const opts = themeOptions(s)
  const paletteId = activePaletteId(s)
  const key = JSON.stringify([s.mode, paletteId, opts.accent, opts.font, opts.radius, opts.custom])
  return useMemo(
    () => ({ mode: s.mode, tokens: themeTokens(s.mode, paletteId, opts), radius: opts.radius, font: FONTS[opts.font].family, mono: MONO_FAMILY }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [key],
  )
}

let hostPromise: Promise<HostApi> | null = null
const host = () => (hostPromise ??= appHost())

/** How long a page has to load the SDK and say hello. */
const START_TIMEOUT_MS = 10_000

interface Props {
  mod: ModInfo
  slot: ModSlot
  /** A page from the mod. */
  entry?: string
  /** Or a page the user runs locally (no SDK, no bridge). */
  url?: string
  /** Passed to the page as `context.data` (a chat block's text); updates live. */
  data?: unknown
  /** Fill the parent (panels) instead of sizing to the content. */
  fill?: boolean
  minHeight?: number
  maxHeight?: number
}

/**
 * One mod page, sandboxed: an opaque-origin iframe (scripts only — no
 * same-origin, no popups, no top navigation) talking to the app through
 * `bridge.ts`. A page that crashes or never starts is contained here, with
 * Reload and Turn off.
 */
export function ModFrame({ mod, slot, entry, url, data, fill, minHeight = 32, maxHeight = 640 }: Props) {
  const ref = useRef<HTMLIFrameElement>(null)
  const theme = useModTheme()
  const version = useModsStore((s) => s.fingerprint[mod.key] ?? 0)
  const setEnabled = useModsStore((s) => s.setEnabled)
  const [height, setHeight] = useState(minHeight)
  const [state, setState] = useState<'starting' | 'running' | 'stalled'>(url ? 'running' : 'starting')
  const [reloads, setReloads] = useState(0)
  const latest = useRef({ theme, data })
  latest.current = { theme, data }

  const src = useMemo(() => {
    if (url) return url
    const base = modUrl(mod.key, entry ?? 'index.html')
    return `${base}?v=${version}-${reloads}`
  }, [url, mod.key, entry, version, reloads])

  // The bridge: one connection per page load.
  useEffect(() => {
    if (url) return
    setState('starting')
    const frame = ref.current
    const post = (msg: unknown) => frame?.contentWindow?.postMessage(msg, '*')
    let conn: ModConnection | null = null
    const connected = host().then((h) => {
      conn = new ModConnection(mod, slot, h, (name, payload) => post({ type: 'viv:event', name, payload }))
      return conn
    })
    let started = false
    const onMessage = (e: MessageEvent) => {
      if (!frame || e.source !== frame.contentWindow) return
      const m = e.data as { type?: string; id?: number; method?: string; args?: Record<string, unknown>; height?: number }
      if (!m || typeof m !== 'object') return
      if (m.type === 'viv:hello') {
        started = true
        setState('running')
        post({
          type: 'viv:init',
          mod: { key: mod.key, id: mod.manifest?.id, name: mod.manifest?.name },
          slot,
          data: latest.current.data ?? null,
          permissions: mod.manifest?.permissions ?? [],
          theme: latest.current.theme,
        })
      } else if (m.type === 'viv:resize' && typeof m.height === 'number') {
        setHeight(Math.min(maxHeight, Math.max(minHeight, m.height)))
      } else if (m.type === 'viv:call' && typeof m.id === 'number' && typeof m.method === 'string') {
        const id = m.id
        connected
          .then((c) => c.handle(m.method!, m.args ?? null))
          .then(
            (value) => post({ type: 'viv:result', id, ok: true, value: value ?? null }),
            (err: unknown) => post({ type: 'viv:result', id, ok: false, error: err instanceof Error ? err.message : String(err) }),
          )
      }
    }
    window.addEventListener('message', onMessage)
    const timer = window.setTimeout(() => {
      if (!started) setState('stalled')
    }, START_TIMEOUT_MS)
    return () => {
      window.removeEventListener('message', onMessage)
      window.clearTimeout(timer)
      void connected.then((c) => c.dispose())
      conn = null
    }
    // The connection follows the page load (`src`) and the mod's grants.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [src, mod.status, mod.manifest?.permissions.join(',')])

  // Live theme and data.
  useEffect(() => {
    ref.current?.contentWindow?.postMessage({ type: 'viv:event', name: 'theme', payload: theme }, '*')
  }, [theme])
  useEffect(() => {
    if (data !== undefined) ref.current?.contentWindow?.postMessage({ type: 'viv:event', name: 'data', payload: data }, '*')
  }, [data])

  const name = mod.manifest?.name ?? mod.key
  return (
    <Box sx={{ position: 'relative', width: '100%', ...(fill ? { flex: 1, minHeight: 0, display: 'flex' } : {}) }}>
      <Box
        component="iframe"
        ref={ref}
        src={src}
        title={name}
        // Scripts and forms only: an opaque origin, so the page can't reach
        // the app, its storage, or open windows. `vivmod` adds a CSP.
        sandbox={url ? 'allow-scripts allow-forms allow-same-origin' : 'allow-scripts allow-forms'}
        sx={{
          border: 0,
          width: '100%',
          display: 'block',
          bgcolor: 'transparent',
          colorScheme: theme.mode,
          ...(fill ? { flex: 1, height: '100%', minHeight: 0 } : { height }),
          visibility: state === 'stalled' ? 'hidden' : 'visible',
        }}
      />
      {state === 'stalled' && (
        <Stack sx={{ position: 'absolute', inset: 0, alignItems: 'center', justifyContent: 'center', gap: 1, p: 1.5, textAlign: 'center' }}>
          <Typography variant="caption" color="text.secondary">
            {name} didn't start. Its page must load <code>/_sdk/v1.js</code>.
          </Typography>
          <Stack direction="row" sx={{ gap: 1 }}>
            <Button size="small" onClick={() => setReloads((n) => n + 1)} sx={{ textTransform: 'none' }}>
              Reload
            </Button>
            <Button size="small" color="error" onClick={() => void setEnabled(mod.key, false)} sx={{ textTransform: 'none' }}>
              Turn off
            </Button>
          </Stack>
        </Stack>
      )}
    </Box>
  )
}
