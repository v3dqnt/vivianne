import { useEffect, useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import Divider from '@mui/material/Divider'
import CircularProgress from '@mui/material/CircularProgress'
import Tooltip from '@mui/material/Tooltip'
import { CommandSquareLinear, AddLinear, StopCircleLinear, UserOctagonLinear } from '../../../icons'
import { call } from '../../../ipc'
import { useSubagentStore } from '../../ai/store/subagentStore'
import { useActiveWorkspaceRoot } from '../useActiveWorkspace'
import { TerminalPanel } from './TerminalPanel'

interface TerminalSessionInfo {
  tabId: string
  shellId: string
  cwd: string
  startedAtEpochMs: number
}

function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() || path
}

function formatElapsed(ms: number): string {
  const s = Math.round(ms / 1000)
  if (s < 60) return `${s}s`
  return `${Math.floor(s / 60)}m ${s % 60}s`
}

/**
 * Background Tasks — every live terminal session (whether I opened it or the
 * agent's shell tools did) plus running/finished subagents, in one list.
 * Selecting a terminal session attaches the xterm view below it.
 */
export function BackgroundTasksPanel() {
  const [sessions, setSessions] = useState<TerminalSessionInfo[]>([])
  const [selected, setSelected] = useState<string | null>(null)
  const root = useActiveWorkspaceRoot()

  const subagents = useSubagentStore((s) => s.subagents)
  const refreshSubagents = useSubagentStore((s) => s.refresh)
  const killSubagent = useSubagentStore((s) => s.kill)

  const refreshSessions = async () => {
    const list = (await call<TerminalSessionInfo[]>('cmd_terminal_list_sessions')) ?? []
    setSessions(list)
    return list
  }

  useEffect(() => {
    void refreshSessions()
    void refreshSubagents()
    // Terminal sessions can appear without any frontend action (the agent's
    // shell tools spawn their own), so this polls rather than relying on an
    // event we don't emit.
    const id = setInterval(() => {
      void refreshSessions()
      void refreshSubagents()
    }, 2500)
    return () => clearInterval(id)
  }, [refreshSubagents])

  const openNewTerminal = () => {
    // Mounting a fresh TerminalPanel spawns its own PTY (it generates its own
    // tab id on mount) — selecting the sentinel is what mounts it.
    setSelected('__new__')
  }

  const closeTerminal = async (tabId: string) => {
    await call('cmd_terminal_close', { tabId })
    if (selected === tabId) setSelected(null)
    await refreshSessions()
  }

  return (
    <Stack sx={{ flex: 1, minHeight: 0, minWidth: 0 }}>
      <Box sx={{ flexShrink: 0, maxHeight: '45%', overflowY: 'auto', px: 1, py: 1 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, mb: 0.5 }}>
          <Typography variant="caption" sx={{ fontWeight: 600, color: 'text.secondary', textTransform: 'uppercase', letterSpacing: 0.4, fontSize: '0.625rem', flex: 1 }}>
            Terminals ({sessions.length})
          </Typography>
          <Tooltip title="New terminal">
            <IconButton size="small" onClick={openNewTerminal} sx={{ p: 0.25 }}>
              <AddLinear size={13} color="currentColor" />
            </IconButton>
          </Tooltip>
        </Stack>

        {sessions.length === 0 && selected !== '__new__' && (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', py: 0.5 }}>
            No terminal sessions. The agent's shell tools show up here too.
          </Typography>
        )}
        {sessions.map((s) => (
          <Stack
            key={s.tabId}
            direction="row"
            onClick={() => setSelected(s.tabId)}
            sx={{
              alignItems: 'center',
              gap: 0.75,
              px: 0.75,
              py: 0.5,
              borderRadius: 1,
              cursor: 'pointer',
              bgcolor: selected === s.tabId ? 'action.selected' : 'transparent',
              '&:hover': { bgcolor: 'action.hover' },
            }}
          >
            <CommandSquareLinear size={13} color="currentColor" style={{ flexShrink: 0 }} />
            <Stack sx={{ flex: 1, minWidth: 0 }}>
              <Typography variant="caption" noWrap sx={{ fontSize: '0.6875rem' }}>
                {s.shellId}
              </Typography>
              <Typography variant="caption" noWrap title={s.cwd} sx={{ color: 'text.disabled', fontSize: '0.625rem' }}>
                {basename(s.cwd)}
              </Typography>
            </Stack>
            <IconButton
              size="small"
              onClick={(e) => {
                e.stopPropagation()
                void closeTerminal(s.tabId)
              }}
              sx={{ p: 0.25, flexShrink: 0 }}
            >
              <StopCircleLinear size={13} color="currentColor" />
            </IconButton>
          </Stack>
        ))}

        {subagents.length > 0 && (
          <>
            <Divider sx={{ my: 1 }} />
            <Typography variant="caption" sx={{ fontWeight: 600, color: 'text.secondary', textTransform: 'uppercase', letterSpacing: 0.4, fontSize: '0.625rem', display: 'block', mb: 0.5 }}>
              Subagents ({subagents.length})
            </Typography>
            {subagents.map((a) => (
              <Stack key={a.id} direction="row" sx={{ alignItems: 'center', gap: 0.75, px: 0.75, py: 0.5 }}>
                {a.status === 'running' ? (
                  <CircularProgress size={11} thickness={5} sx={{ flexShrink: 0 }} />
                ) : (
                  <UserOctagonLinear size={13} color="currentColor" style={{ flexShrink: 0 }} />
                )}
                <Stack sx={{ flex: 1, minWidth: 0 }}>
                  <Typography variant="caption" noWrap sx={{ fontSize: '0.6875rem' }} title={a.description}>
                    {a.description}
                  </Typography>
                  <Typography variant="caption" sx={{ color: 'text.disabled', fontSize: '0.625rem' }}>
                    {a.status} · {formatElapsed(a.elapsedMs)}
                  </Typography>
                </Stack>
                {a.status === 'running' && (
                  <IconButton size="small" onClick={() => void killSubagent(a.id)} sx={{ p: 0.25, flexShrink: 0 }}>
                    <StopCircleLinear size={13} color="currentColor" />
                  </IconButton>
                )}
              </Stack>
            ))}
          </>
        )}
      </Box>

      <Divider />

      <Box sx={{ flex: 1, minHeight: 0, minWidth: 0, display: 'flex' }}>
        {selected ? (
          // Keyed so switching sessions remounts against the right tab id.
          <TerminalPanel key={selected} attachTabId={selected === '__new__' ? undefined : selected} cwd={root ?? undefined} />
        ) : (
          <Box sx={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', p: 2 }}>
            <Typography variant="caption" color="text.secondary" sx={{ textAlign: 'center' }}>
              Select a session above, or start a new terminal.
            </Typography>
          </Box>
        )}
      </Box>
    </Stack>
  )
}
