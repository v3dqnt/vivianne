import { useEffect, useState } from 'react'
import { Box, CircularProgress, IconButton, Stack, Typography } from '../../../ui'
import { StopCircleLinear } from '../../../icons'
import { on } from '../../../ipc'
import { activeThreadId } from '../store/activeThread'
import { useJobsStore, type Job } from '../store/jobsStore'

const MONO = '"Geist Mono", ui-monospace, monospace'

function JobRow({ job }: { job: Job }) {
  const stop = useJobsStore((s) => s.stop)
  const [confirm, setConfirm] = useState(false)
  const [open, setOpen] = useState(false)
  // Stopping takes two presses: the first asks, and lapses after a few seconds.
  useEffect(() => {
    if (!confirm) return
    const t = setTimeout(() => setConfirm(false), 3000)
    return () => clearTimeout(t)
  }, [confirm])
  const running = job.status === 'running'
  return (
    <Stack>
      <Stack direction="row" onClick={() => job.outputTail && setOpen((v) => !v)} sx={{ alignItems: 'center', gap: 1, py: 0.5, cursor: job.outputTail ? 'pointer' : 'default' }}>
        {running && <CircularProgress size={11} thickness={5} />}
        <Typography noWrap variant="caption" sx={{ fontFamily: MONO, flex: 1, minWidth: 0 }}>
          {job.label || job.tool}
        </Typography>
        <Typography variant="caption" color={job.status === 'failed' ? 'error.main' : 'text.secondary'}>
          {job.status}
        </Typography>
        {running && (
          <IconButton
            size="small"
            title={confirm ? 'Press again to stop it' : 'Stop'}
            onClick={(e) => {
              e.stopPropagation()
              if (!confirm) return setConfirm(true)
              setConfirm(false)
              void stop(job.callId)
            }}
            sx={confirm ? { color: 'error.main', width: 'auto', px: 0.75, borderRadius: '6px' } : undefined}
          >
            {confirm ? <Typography variant="caption">Stop?</Typography> : <StopCircleLinear size={14} color="currentColor" />}
          </IconButton>
        )}
      </Stack>
      {open && job.outputTail && (
        <Box component="pre" sx={{ m: 0, mb: 0.5, p: 1, borderRadius: '8px', bgcolor: 'action.hover', fontFamily: MONO, fontSize: '0.6875rem', lineHeight: 1.5, whiteSpace: 'pre-wrap', wordBreak: 'break-word', maxHeight: 180, overflowY: 'auto' }}>
          {job.outputTail}
        </Box>
      )}
    </Stack>
  )
}

/** Commands and searches the user sent to the background, with their state and a way to stop them. Shown only while there are some. */
export function JobsPanel() {
  const jobs = useJobsStore((s) => s.jobs)
  const refresh = useJobsStore((s) => s.refresh)
  const clearEnded = useJobsStore((s) => s.clearEnded)
  const anyRunning = jobs.some((j) => j.status === 'running')

  useEffect(() => {
    void refresh(activeThreadId())
    const off = on('tool:background_done', () => void refresh(activeThreadId()))
    return () => void off.then((f) => f())
  }, [refresh])
  // A press of Background starts a job with no event of its own, so look again while one runs.
  useEffect(() => {
    if (!anyRunning) return
    const t = setInterval(() => void refresh(activeThreadId()), 3000)
    return () => clearInterval(t)
  }, [anyRunning, refresh])

  if (jobs.length === 0) return null
  return (
    <Box sx={{ border: 1, borderColor: 'divider', borderRadius: 1.6, bgcolor: 'background.paper', px: 1.5, py: 0.5 }}>
      <Stack direction="row" sx={{ alignItems: 'center', py: 0.25 }}>
        <Typography variant="caption" sx={{ fontWeight: 500, flex: 1 }}>
          Background {anyRunning ? `· ${jobs.filter((j) => j.status === 'running').length} running` : ''}
        </Typography>
        {!anyRunning && (
          <Typography variant="caption" color="text.secondary" onClick={() => void clearEnded()} sx={{ cursor: 'pointer', '&:hover': { color: 'text.primary' } }}>
            Clear
          </Typography>
        )}
      </Stack>
      {jobs.map((j) => (
        <JobRow key={j.callId} job={j} />
      ))}
    </Box>
  )
}
