import { useEffect, useRef, useState } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { Box, ButtonBase, Stack, Typography, useTheme } from '../../ui'
import { call, on } from '../../ipc'
import { useWorkspaceStore } from '../ai/store/workspaceStore'

/** xterm measures its cells in pixels, not rem. */
const TERMINAL_FONT_PX = 12

interface ShellOption {
  id: string
  label: string
}

/** A terminal's id for a folder: one shell per project folder, kept while the app runs. */
export function terminalIdFor(folder: string): string {
  let h = 0
  for (const ch of folder.toLowerCase()) h = (Math.imul(h, 31) + ch.charCodeAt(0)) | 0
  return `rail-${(h >>> 0).toString(36)}`
}

/**
 * A real shell in the project's folder, drawn with xterm. It stays mounted once opened (the rail hides it rather than unmounting it), so
 * what it printed while another tab was showing is still there when you come back.
 */
export function TerminalTab({ visible }: { visible: boolean }) {
  const folder = useWorkspaceStore((s) => s.folders[0] ?? null)
  const theme = useTheme()
  const host = useRef<HTMLDivElement | null>(null)
  const fit = useRef<FitAddon | null>(null)
  const [exited, setExited] = useState<number | null | undefined>(undefined)
  const [error, setError] = useState<string | null>(null)
  const [generation, setGeneration] = useState(0)
  const id = folder ? terminalIdFor(folder) : null

  useEffect(() => {
    if (!host.current || !folder || !id) return
    const dark = theme.palette.mode === 'dark'
    const term = new Terminal({
      fontFamily: '"Geist Mono", ui-monospace, monospace',
      fontSize: TERMINAL_FONT_PX,
      lineHeight: 1.25,
      cursorBlink: true,
      allowTransparency: true,
      scrollback: 5000,
      theme: {
        background: '#00000000',
        foreground: theme.palette.text.primary,
        cursor: theme.palette.text.primary,
        selectionBackground: dark ? '#ffffff30' : '#00000020',
      },
    })
    const fitter = new FitAddon()
    term.loadAddon(fitter)
    term.open(host.current)
    fit.current = fitter
    setExited(undefined)
    setError(null)

    const offOut = on<{ tabId: string; data: string }>('terminal-output', (e) => {
      if (e.tabId === id) term.write(e.data)
    })
    const offExit = on<{ tabId: string; exitCode: number | null }>('terminal-exit', (e) => {
      if (e.tabId === id) setExited(e.exitCode)
    })
    const typed = term.onData((data) => void call('cmd_terminal_write', { tabId: id, data }))
    const sized = term.onResize(({ rows, cols }) => void call('cmd_terminal_resize', { tabId: id, rows, cols }))

    // Join the folder's shell if it is already running, else start one.
    void (async () => {
      const live = (await call<{ tabId: string }[]>('cmd_terminal_list_sessions').catch(() => [])) ?? []
      if (!live.some((s) => s.tabId === id)) {
        const shells = (await call<ShellOption[]>('cmd_terminal_list_shells').catch(() => [])) ?? []
        const shell = shells[0]?.id ?? (navigator.userAgent.includes('Windows') ? 'powershell' : 'bash')
        await call('cmd_terminal_spawn', { tabId: id, shellId: shell, cwd: folder }).catch((e) => setError(String(e instanceof Error ? e.message : e)))
      }
      try {
        fitter.fit()
      } catch {
        /* not laid out yet */
      }
    })()

    const ro = new ResizeObserver(() => {
      try {
        fitter.fit()
      } catch {
        /* hidden */
      }
    })
    ro.observe(host.current)
    return () => {
      ro.disconnect()
      typed.dispose()
      sized.dispose()
      void offOut.then((f) => f())
      void offExit.then((f) => f())
      term.dispose()
      fit.current = null
    }
  }, [folder, id, generation, theme.palette.mode, theme.palette.text.primary])

  // Coming back into view: fit to the space it now has.
  useEffect(() => {
    if (!visible) return
    const t = setTimeout(() => {
      try {
        fit.current?.fit()
      } catch {
        /* not laid out */
      }
    }, 300)
    return () => clearTimeout(t)
  }, [visible])

  if (!folder) return <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary', px: '8px' }}>This thread has no folder.</Typography>
  const restart = async () => {
    if (id) await call('cmd_terminal_close', { tabId: id }).catch(() => undefined)
    setGeneration((g) => g + 1)
  }
  return (
    <Stack sx={{ minHeight: 0, flex: 1, gap: '8px' }}>
      <Box ref={host} sx={{ flex: 1, minHeight: 0, px: '4px', '& .xterm-viewport': { bgcolor: 'transparent !important' } }} />
      {(exited !== undefined || error) && (
        <Stack direction="row" sx={{ alignItems: 'center', gap: '8px', px: '8px', flexShrink: 0 }}>
          <Typography sx={{ fontSize: '0.75rem', color: error ? 'error.main' : 'text.secondary', flex: 1 }}>
            {error ?? `The shell exited${exited != null ? ` (code ${exited})` : ''}.`}
          </Typography>
          <ButtonBase onClick={() => void restart()} sx={{ fontSize: '0.75rem', px: '8px', height: 24, borderRadius: '6px', color: 'text.primary', bgcolor: 'action.hover' }}>
            Start again
          </ButtonBase>
        </Stack>
      )}
    </Stack>
  )
}
