import { useEffect, useMemo, useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import InputBase from '@mui/material/InputBase'
import Button from '@mui/material/Button'
import Tooltip from '@mui/material/Tooltip'
import CircularProgress from '@mui/material/CircularProgress'
import { RefreshLinear, MonitorLinear, StopCircleLinear, PlayCircleLinear, GlobalLinear } from '../../../icons'
import { useActiveWorkspaceRoot } from '../useActiveWorkspace'
import { useDevServerStore } from '../store/devServerStore'
import { DEFAULT_DEV_COMMAND, devServerId } from '../lib/devServer'

/** Preset content widths — `null` means "fill the available panel width"
 * rather than a fixed device size. */
const BREAKPOINTS: { label: string; width: number | null }[] = [
  { label: 'Mobile', width: 390 },
  { label: 'Tablet', width: 768 },
  { label: 'Desktop', width: 1280 },
  { label: 'Fit', width: null },
]

/** Live preview dock panel — starts the workspace's own dev command (`npm
 * run dev` by default, editable) via `DevServerManager`, waits for the URL
 * it prints on startup, and embeds it in an iframe with responsive
 * breakpoint presets and a reload control. One config per workspace root
 * (`devServerId`), so switching threads/workspaces shows that project's own
 * preview instead of leaking another project's running server across. */
export function LivePreviewPanel() {
  const root = useActiveWorkspaceRoot()
  const servers = useDevServerStore((s) => s.servers)
  const refresh = useDevServerStore((s) => s.refresh)
  const register = useDevServerStore((s) => s.register)
  const start = useDevServerStore((s) => s.start)
  const stop = useDevServerStore((s) => s.stop)

  const [command, setCommand] = useState(DEFAULT_DEV_COMMAND)
  const [breakpoint, setBreakpoint] = useState<number | null>(null)
  const [reloadKey, setReloadKey] = useState(0)
  const [starting, setStarting] = useState(false)

  useEffect(() => {
    void refresh()
  }, [refresh])

  const id = root ? devServerId(root) : null
  const entry = useMemo(() => (id ? servers.find(([c]) => c.id === id) : undefined), [servers, id])
  const status = entry?.[1] ?? { kind: 'stopped' as const }

  const doStart = async () => {
    if (!root || !id) return
    setStarting(true)
    try {
      await register({ id, name: root.split(/[\\/]/).filter(Boolean).pop() || 'dev', cwd: root, command: command.trim() || DEFAULT_DEV_COMMAND })
      await start(id)
    } finally {
      setStarting(false)
    }
  }

  if (!root) {
    return (
      <Stack sx={{ height: '100%', alignItems: 'center', justifyContent: 'center', gap: 1, p: 2 }}>
        <MonitorLinear size={24} color="currentColor" />
        <Typography variant="caption" color="text.secondary">
          Open a workspace to preview its dev server.
        </Typography>
      </Stack>
    )
  }

  return (
    <Stack sx={{ height: '100%', minHeight: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, p: 1, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        {status.kind === 'running' ? (
          <>
            <Tooltip title="Reload">
              <IconButton size="small" onClick={() => setReloadKey((k) => k + 1)}>
                <RefreshLinear size={14} color="currentColor" />
              </IconButton>
            </Tooltip>
            <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5, flex: 1, minWidth: 0, px: 1, py: 0.375, bgcolor: 'action.hover', borderRadius: 999 }}>
              <GlobalLinear size={12} color="currentColor" />
              <Typography variant="caption" sx={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace' }} noWrap>
                {status.url}
              </Typography>
            </Stack>
            {BREAKPOINTS.map((bp) => (
              <Tooltip key={bp.label} title={bp.label}>
                <Button
                  size="small"
                  onClick={() => setBreakpoint(bp.width)}
                  sx={{
                    minWidth: 0,
                    px: 0.875,
                    fontSize: '0.6875rem',
                    color: breakpoint === bp.width ? 'primary.main' : 'text.secondary',
                    bgcolor: breakpoint === bp.width ? 'action.selected' : 'transparent',
                  }}
                >
                  {bp.label.charAt(0)}
                </Button>
              </Tooltip>
            ))}
            <Tooltip title="Stop dev server">
              <IconButton size="small" onClick={() => id && void stop(id)}>
                <StopCircleLinear size={15} color="currentColor" />
              </IconButton>
            </Tooltip>
          </>
        ) : (
          <>
            <InputBase
              fullWidth
              placeholder={DEFAULT_DEV_COMMAND}
              value={command}
              onChange={(e) => setCommand(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && void doStart()}
              sx={{ fontSize: '0.8125rem', fontFamily: '"JetBrains Mono", ui-monospace, monospace', bgcolor: 'action.hover', borderRadius: 999, px: 1.5, py: 0.5 }}
            />
            <Button
              size="small"
              variant="contained"
              disableElevation
              onClick={() => void doStart()}
              disabled={starting || status.kind === 'starting'}
              startIcon={starting || status.kind === 'starting' ? <CircularProgress size={12} /> : <PlayCircleLinear size={14} color="currentColor" />}
              sx={{ textTransform: 'none', flexShrink: 0 }}
            >
              Start
            </Button>
          </>
        )}
      </Stack>

      <Box sx={{ flex: 1, minHeight: 0, overflow: 'auto', display: 'flex', justifyContent: 'center', bgcolor: 'action.hover' }}>
        {status.kind === 'running' ? (
          <Box sx={{ width: breakpoint ?? '100%', flexShrink: 0, bgcolor: 'background.paper' }}>
            <iframe key={reloadKey} src={status.url} title="Live preview" style={{ width: '100%', height: '100%', border: 'none', display: 'block', minHeight: '100%' }} />
          </Box>
        ) : status.kind === 'crashed' ? (
          <Stack sx={{ p: 2, gap: 0.5, alignSelf: 'flex-start', maxWidth: '100%' }}>
            <Typography variant="caption" color="error.main" sx={{ fontFamily: 'monospace', whiteSpace: 'pre-wrap' }}>
              {status.error}
            </Typography>
          </Stack>
        ) : (
          <Stack sx={{ alignItems: 'center', justifyContent: 'center', gap: 0.5 }}>
            <Typography variant="caption" color="text.secondary">
              {status.kind === 'starting' ? 'Waiting for the dev server to print its URL…' : 'Not running.'}
            </Typography>
          </Stack>
        )}
      </Box>
    </Stack>
  )
}
