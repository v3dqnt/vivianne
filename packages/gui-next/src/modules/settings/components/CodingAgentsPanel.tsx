import { useEffect, useState } from 'react'
import { Box, Button, CircularProgress, Stack, Typography } from '../../../ui'
import { call } from '../../../ipc'
import { statusOf, useHarnessStore, type Harness } from '../../ai/store/harnessStore'

const MONO = '"Geist Mono", ui-monospace, monospace'

const STATUS_LABEL = { ready: 'Ready', setup: 'Needs setup', missing: 'Not installed' } as const
// Only "ready" gets a colour: a list of 25 agents that all say "needs setup" in yellow is noise.
const STATUS_COLOR = { ready: 'success.main', setup: 'text.secondary', missing: 'text.disabled' } as const
/** How many agents show before "Show all". The backend sends the most used first. */
const FEATURED = 6

/** Settings → Providers → Coding agents: which ACP agents can run, set one up,
 * and test it step by step when a turn won't start. */
export function CodingAgentsPanel() {
  const { list, error, refresh } = useHarnessStore()
  const [trace, setTrace] = useState<string[] | null>(null)
  const [all, setAll] = useState(false)

  useEffect(() => {
    void refresh()
  }, [refresh])

  const missingLauncher = list?.some((h) => !h.available) ?? false

  return (
    <Stack sx={{ gap: 1.5, maxWidth: 560 }}>
      <Typography variant="body2" color="text.secondary">
        Other coding agents, run through the Agent Client Protocol. A thread can use one from the model picker. Each starts through Node (<code>npx</code>) or uv (<code>uvx</code>), and
        downloads the first time it is set up.
      </Typography>
      {missingLauncher && (
        <Typography variant="caption" sx={{ color: 'warning.main' }}>
          Node.js or uv isn't installed on this computer, so the agents marked Not installed can't start. Install Node.js from nodejs.org, then reopen this page.
        </Typography>
      )}
      {!list && <Typography variant="body2" color="text.secondary">Looking up coding agents…</Typography>}
      {error && <Typography variant="body2" sx={{ color: 'error.main' }}>Could not load the agent list.</Typography>}
      {(all ? list : list?.slice(0, FEATURED))?.map((h) => <AgentRow key={h.id} h={h} />)}
      {list && list.length > FEATURED && (
        <Button size="small" variant="text" onClick={() => setAll((v) => !v)} sx={{ alignSelf: 'flex-start' }}>
          {all ? 'Show fewer' : `Show all ${list.length}`}
        </Button>
      )}
      <Stack direction="row" sx={{ gap: 1, pt: 1 }}>
        <Button
          size="small"
          variant="outlined"
          onClick={async () => setTrace(await call<string[]>('cmd_harness_trace'))}
        >
          Show log
        </Button>
        <Button
          size="small"
          variant="outlined"
          onClick={async () => {
            await call('cmd_harness_reset')
            setTrace(await call<string[]>('cmd_harness_trace'))
          }}
        >
          Stop running agents
        </Button>
      </Stack>
      {trace && <Log lines={trace} />}
    </Stack>
  )
}

function AgentRow({ h }: { h: Harness }) {
  const { busy, failures, reports, progress, install, debug } = useHarnessStore()
  const status = statusOf(h)
  const doing = busy[h.id]
  const report = reports[h.id]
  const failure = failures[h.id]
  return (
    <Stack sx={{ gap: 0.75, p: 1.25, borderRadius: '8px', border: 1, borderColor: 'divider' }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
        <Stack sx={{ flex: 1, minWidth: 0 }}>
          <Typography sx={{ fontSize: '0.875rem', fontWeight: 600 }}>{h.name}</Typography>
          <Typography noWrap sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>{h.description}</Typography>
        </Stack>
        <Typography sx={{ fontSize: '0.75rem', fontWeight: 500, color: STATUS_COLOR[status], flexShrink: 0 }}>{STATUS_LABEL[status]}</Typography>
        {status === 'setup' && (
          <Button size="small" variant="outlined" disabled={!!doing} onClick={() => void install(h.id)}>
            {doing === 'install' ? <CircularProgress size={14} /> : 'Set up'}
          </Button>
        )}
        {status === 'ready' && (
          <Button size="small" variant="text" disabled={!!doing} onClick={() => void debug(h.id)}>
            {doing === 'debug' ? <CircularProgress size={14} /> : 'Test'}
          </Button>
        )}
      </Stack>
      {doing === 'install' && (
        <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>
          {progress[h.id] ? `${(progress[h.id].bytes / 1e6).toFixed(0)} MB downloaded · ${progress[h.id].line}` : `Starting the download of ${h.name}…`}
          {' '}This agent can be a few hundred MB the first time.
        </Typography>
      )}
      {doing === 'debug' && <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>Starting it, opening a session, and sending one short prompt…</Typography>}
      {failure && <Typography sx={{ fontSize: '0.75rem', color: 'error.main', whiteSpace: 'pre-wrap' }}>{failure}</Typography>}
      {report && !doing && (
        <Stack sx={{ gap: 0.25 }}>
          {report.steps.map((s) => (
            <Typography key={s.name} sx={{ fontSize: '0.75rem', color: s.ok ? 'text.secondary' : 'error.main', whiteSpace: 'pre-wrap' }}>
              {s.ok ? '✓' : '✗'} {s.name} ({(s.ms / 1000).toFixed(1)}s): {s.detail}
            </Typography>
          ))}
          {report.reply && <Typography sx={{ fontSize: '0.75rem' }}>It said: “{report.reply}”</Typography>}
          <Log lines={report.trace.filter((l) => l.includes(`[${h.name}]`) || l.includes('[harness]')).slice(-25)} />
        </Stack>
      )}
    </Stack>
  )
}

function Log({ lines }: { lines: string[] }) {
  return (
    <Box sx={{ fontFamily: MONO, fontSize: '0.6875rem', color: 'text.secondary', bgcolor: 'action.hover', borderRadius: '8px', p: 1, maxHeight: 220, overflow: 'auto', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
      {lines.length ? lines.join('\n') : 'Nothing logged yet.'}
    </Box>
  )
}
