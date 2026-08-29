import { useEffect, useRef, useState } from 'react'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import { useTheme, alpha } from '@mui/material/styles'
import { Terminal, type ITheme } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { call, on } from '../../../ipc'
import { useWorkspaceStore } from '../../ai/store/workspaceStore'
import { usePanelStore } from '../store/panelStore'

interface ShellOption {
  id: string
  label: string
}

interface TerminalOutputPayload {
  tabId: string
  data: string
}

interface TerminalExitPayload {
  tabId: string
  exitCode: number | null
}

/** ANSI 16-color set, fixed per mode rather than derived from the palette —
 * a themed terminal should still show real red/green/etc for command output,
 * only its chrome (background/foreground/cursor) needs to track the app
 * theme. Two sets (light/dark) keeps both legible against either background. */
const ANSI = {
  dark: {
    black: '#27272a', red: '#f87171', green: '#4ade80', yellow: '#fbbf24',
    blue: '#60a5fa', magenta: '#a78bfa', cyan: '#22d3ee', white: '#e4e4e7',
    brightBlack: '#71717a', brightRed: '#fca5a5', brightGreen: '#86efac', brightYellow: '#fde047',
    brightBlue: '#93c5fd', brightMagenta: '#c4b5fd', brightCyan: '#67e8f9', brightWhite: '#fafafa',
  },
  light: {
    black: '#3f3f46', red: '#dc2626', green: '#16a34a', yellow: '#ca8a04',
    blue: '#2563eb', magenta: '#9333ea', cyan: '#0891b2', white: '#3f3f46',
    brightBlack: '#71717a', brightRed: '#ef4444', brightGreen: '#22c55e', brightYellow: '#eab308',
    brightBlue: '#3b82f6', brightMagenta: '#a855f7', brightCyan: '#06b6d4', brightWhite: '#18181b',
  },
} as const

function terminalTheme(mode: 'light' | 'dark', textPrimary: string, primary: string): ITheme {
  return {
    background: 'transparent',
    foreground: textPrimary,
    cursor: primary,
    cursorAccent: mode === 'dark' ? '#0a0a0a' : '#ffffff',
    selectionBackground: alpha(primary, 0.25),
    ...ANSI[mode],
  }
}

/** One live xterm session bound to a backend PTY. The tab is spawned on
 * mount (default shell, cwd = primary workspace folder), and all IO flows
 * through the backend's `cmd_terminal_*` + `terminal-output`/`terminal-exit`
 * events. Cleanup closes the PTY and unsubscribes listeners. */
/**
 * @param attachTabId Attach to an already-running PTY (from the Background
 *   Tasks list) instead of spawning a fresh one. The backend keeps sessions
 *   alive independently of this component, so reattaching just means not
 *   calling spawn — output resumes streaming via the same event listeners.
 * @param cwd Working directory for a newly-spawned shell. Defaults to the
 *   active workspace root.
 */
export function TerminalPanel({ attachTabId, cwd }: { attachTabId?: string; cwd?: string } = {}) {
  const containerRef = useRef<HTMLDivElement>(null)
  const tabIdRef = useRef<string>(attachTabId ?? `term-${crypto.randomUUID()}`)
  const termRef = useRef<Terminal | null>(null)
  const fitAddonRef = useRef<FitAddon | null>(null)
  const folders = useWorkspaceStore((s) => s.folders)
  const [status, setStatus] = useState<string | null>(attachTabId ? null : 'Spawning shell…')
  const theme = useTheme()

  // True while a ResizeHandle drag (right panel or bottom terminal, whichever
  // this instance happens to live inside) is in flight. The panel's real DOM
  // size changes every frame of the drag regardless of that — flexbox lays
  // out `containerRef` continuously — so the ResizeObserver below would
  // still fire `fit()` + a resize IPC call per frame even though the drag
  // itself no longer touches React state. Mirrored into a ref because the
  // observer callback is created once, in the mount effect below.
  const panelResizing = usePanelStore((s) => s.panelResizing)
  const panelResizingRef = useRef(false)
  useEffect(() => {
    panelResizingRef.current = panelResizing
    if (!panelResizing) {
      try {
        fitAddonRef.current?.fit()
      } catch {
        // element may be detached
      }
    }
  }, [panelResizing])

  // Live theme updates (mode/palette switch) — xterm supports patching
  // `.options.theme` on an existing instance, no need to respawn the PTY.
  useEffect(() => {
    if (termRef.current) {
      termRef.current.options.theme = terminalTheme(theme.palette.mode, theme.palette.text.primary, theme.palette.primary.main)
    }
  }, [theme.palette.mode, theme.palette.text.primary, theme.palette.primary.main])

  useEffect(() => {
    const tabId = tabIdRef.current
    const container = containerRef.current
    if (!container) return

    const term = new Terminal({
      cursorBlink: true,
      cursorStyle: 'bar',
      fontSize: 12.5,
      fontFamily: '"JetBrains Mono", ui-monospace, monospace',
      lineHeight: 1.5,
      letterSpacing: 0.1,
      theme: terminalTheme(theme.palette.mode, theme.palette.text.primary, theme.palette.primary.main),
      allowTransparency: true,
      scrollback: 5000,
    })
    termRef.current = term
    const fitAddon = new FitAddon()
    fitAddonRef.current = fitAddon
    term.loadAddon(fitAddon)
    term.open(container)
    fitAddon.fit()

    const ro = new ResizeObserver(() => {
      // Skip mid-drag — the effect above runs one `fit()` when the drag
      // ends instead of one per frame while it's in flight.
      if (panelResizingRef.current) return
      try {
        fitAddon.fit()
      } catch {
        // element may be detached
      }
    })
    ro.observe(container)

    term.onData((data) => {
      void call('cmd_terminal_write', { tabId, data })
    })
    // The ResizeObserver's first `fit()` fires onResize during mount, before
    // `cmd_terminal_spawn` has resolved — resizing a PTY that doesn't exist
    // yet threw "no terminal session for tab …" on every terminal that
    // started life hidden (which is every dock tab that isn't the active
    // one). Hold the last requested size instead and replay it once the PTY
    // is actually there. An attached session is ready by definition.
    // Two plain numbers rather than a nullable object: the only writes are
    // inside the onResize callback, which the spawn flow below can't see, so
    // TS narrows a captured `{...} | null` to `never` at the replay site.
    let ptyReady = !!attachTabId
    let pendingRows = 0
    let pendingCols = 0
    term.onResize(({ cols, rows }) => {
      if (!ptyReady) {
        pendingRows = rows
        pendingCols = cols
        return
      }
      void call('cmd_terminal_resize', { tabId, rows, cols })
    })

    let disposed = false
    let unlistenOutput: (() => void) | null = null
    let unlistenExit: (() => void) | null = null

    // Synchronous promise chain: each unlisten is stored the moment its
    // listener resolves, before the next await — so an unmount can never
    // slip between "listener attached" and "unlisten stored" (the exact
    // leak class the old app fixed with the same shape). If unmount wins the
    // race entirely — disposed before a promise resolves — the late listener
    // registers just long enough to unregister itself, instead of living
    // forever with its unlisten never captured.
    on<TerminalOutputPayload>('terminal-output', (event) => {
      if (!disposed && event.tabId === tabId) term.write(event.data)
    }).then((u) => {
      if (disposed) {
        u()
        return null
      }
      unlistenOutput = u
      return on<TerminalExitPayload>('terminal-exit', (event) => {
        if (!disposed && event.tabId === tabId) {
          const code = event.exitCode
          term.write(`\r\n\x1b[90m[Process exited${code !== null ? ` (code ${code})` : ''}]\x1b[0m\r\n`)
        }
      })
    }).then((u) => {
      if (!u) return
      if (disposed) {
        u()
        return
      }
      unlistenExit = u
      setStatus(null)
    })

    // Attaching to an existing session must NOT spawn — the PTY is already
    // running (possibly started by the agent's own shell tools).
    if (!attachTabId) {
      void (async () => {
        const shells = (await call<ShellOption[]>('cmd_terminal_list_shells')) ?? []
        const shellId = shells[0]?.id ?? ''
        const spawnCwd = cwd ?? folders[0] ?? ''
        await call('cmd_terminal_spawn', { tabId, shellId, cwd: spawnCwd })
        if (disposed) return
        ptyReady = true
        // Replay whatever size the terminal settled on while the PTY was
        // still starting, so the shell's view matches the actual panel
        // instead of the 80x24 default it spawned with.
        if (pendingRows > 0 && pendingCols > 0) {
          void call('cmd_terminal_resize', { tabId, rows: pendingRows, cols: pendingCols })
          pendingRows = 0
          pendingCols = 0
        }
        setStatus(null)
      })().catch(() => {
        if (!disposed) setStatus('Failed to spawn shell')
      })
    }

    return () => {
      disposed = true
      ro.disconnect()
      term.dispose()
      termRef.current = null
      fitAddonRef.current = null
      unlistenOutput?.()
      unlistenExit?.()
      // Only kill a PTY this component actually spawned — closing an attached
      // session on unmount would kill the agent's shell out from under it just
      // because the user switched dock tabs.
      if (!attachTabId) void call('cmd_terminal_close', { tabId })
    }
    // `folders` and `theme` are intentionally excluded — re-spawning a PTY on
    // every workspace/theme change would wipe scrollback for no benefit; the
    // theme effect above patches the live instance instead.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  return (
    // Flush to its container: the dock card already clips to its own radius,
    // so padding here just floated a hard-edged black rectangle inside a
    // rounded panel with glass visible around it.
    <Box sx={{ position: 'relative', flex: 1, minHeight: 0, px: 1, pt: 0.75, pb: 0.5 }}>
      {status && (
        <Typography variant="caption" color="text.secondary" sx={{ position: 'absolute', top: 8, left: 12, zIndex: 2 }}>
          {status}
        </Typography>
      )}
      <Box ref={containerRef} sx={{ width: '100%', height: '100%', minHeight: 0, background: 'transparent' }} />
    </Box>
  )
}
