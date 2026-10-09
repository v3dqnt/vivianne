import { useEffect, useState } from 'react'
import { Box, Stack, Typography, Button, ButtonBase, IconButton, InputBase, Collapse, Tooltip, CircularProgress, Orb, alpha, useTheme } from '../../../ui'
import { ArrowLeftLinear, EditLinear, TrashLinear, SendLinear } from '../../../icons'
import { call } from '../../../ipc'
import type { AgentMessage } from '../../ai/lib/types'
import { MessageList } from '../../ai/components/MessageList'
import { Markdown } from '../../ai/components/Markdown'
import { useTeamStore, type AgentRun, type TeamAgent } from '../store/teamStore'
import { Avatar, Tag, SectionLabel, colorOf } from './bits'

const EMPTY_RUNS: AgentRun[] = []

function ago(ms: number): string {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000))
  if (s < 60) return 'just now'
  if (s < 3600) return `${Math.round(s / 60)} min ago`
  if (s < 86_400) return `${Math.round(s / 3600)} h ago`
  return new Date(ms).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

function duration(run: AgentRun): string {
  const end = run.endedAt ?? Date.now()
  const s = Math.max(0, Math.round((end - run.startedAt) / 1000))
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`
}

const STATUS: Record<AgentRun['status'], { label: string; color: 'primary' | 'success' | 'error' | 'text' }> = {
  running: { label: 'Working', color: 'primary' },
  completed: { label: 'Done', color: 'success' },
  failed: { label: 'Failed', color: 'error' },
  cancelled: { label: 'Stopped', color: 'text' },
  interrupted: { label: 'Interrupted', color: 'text' },
}

function StatusPill({ status }: { status: AgentRun['status'] }) {
  const theme = useTheme()
  const { label, color } = STATUS[status]
  const c = color === 'text' ? theme.palette.text.secondary : theme.palette[color].main
  return (
    <Stack
      direction="row"
      sx={{ alignItems: 'center', gap: 0.5, px: 0.75, borderRadius: '6px', bgcolor: alpha(c, 0.12), color: c, flexShrink: 0 }}
    >
      {status === 'running' ? (
        <Orb state="working" px={16} label={label} />
      ) : (
        <Box sx={{ width: 6, height: 6, borderRadius: '50%', bgcolor: c }} />
      )}
      <Typography component="span" sx={{ fontSize: '0.6875rem', fontWeight: 600, lineHeight: '18px' }}>
        {label}
      </Typography>
    </Stack>
  )
}

/**
 * One specialist, read-only: what it is, what it remembers, and every task it
 * has run. The user doesn't talk to specialists — the master does. What the
 * user can do is comment on a run, which steers it if it is still working
 * and becomes a standing note it gets on every future task.
 */
export function AgentPage({ root, agent, onBack, onEdit }: { root: string; agent: TeamAgent; onBack: () => void; onEdit: () => void }) {
  const runs = useTeamStore((s) => s.runs[`${root}::${agent.id}`] ?? EMPTY_RUNS)
  const loadRuns = useTeamStore((s) => s.loadRuns)
  const save = useTeamStore((s) => s.save)
  const [openRun, setOpenRun] = useState<string | null>(null)

  useEffect(() => {
    void loadRuns(root, agent.id).then((r) => {
      // Open the one that is working, or else the latest, so there is
      // something to read straight away.
      setOpenRun((cur) => cur ?? (r.find((x) => x.status === 'running') ?? r[0])?.id ?? null)
    })
  }, [root, agent.id, loadRuns])

  const forget = (index: number) => {
    void save(root, { ...agent, feedback: agent.feedback.filter((_, i) => i !== index) }, agent.id)
  }

  return (
    <Box sx={{ height: '100%', overflow: 'auto' }}>
      <Stack sx={{ p: 1.5, gap: 2 }}>
        <ButtonBase onClick={onBack} sx={{ alignSelf: 'flex-start', gap: 0.5, color: 'text.secondary', fontSize: '0.75rem', borderRadius: '6px', px: 0.5 }}>
          <ArrowLeftLinear size={13} color="currentColor" />
          All agents
        </ButtonBase>

        <Stack direction="row" sx={{ alignItems: 'flex-start', gap: 1.25 }}>
          <Avatar name={agent.name} color={colorOf(agent)} size={34} />
          <Stack sx={{ flex: 1, minWidth: 0, gap: 0.5 }}>
            <Typography variant="body2" sx={{ fontWeight: 600 }}>
              {agent.name}
            </Typography>
            {agent.role && (
              <Typography variant="caption" color="text.secondary">
                {agent.role}
              </Typography>
            )}
            <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap' }}>
              {agent.owns.map((p) => (
                <Tag key={p}>{p}</Tag>
              ))}
              {agent.model && <Tag>{agent.model.name}</Tag>}
              <Tag>{agent.allowedTools ? `${agent.allowedTools.length} tools` : 'all tools'}</Tag>
            </Stack>
          </Stack>
          <Tooltip title="Edit its instructions and settings">
            <IconButton size="small" onClick={onEdit}>
              <EditLinear size={15} color="currentColor" />
            </IconButton>
          </Tooltip>
        </Stack>

        <Typography variant="caption" color="text.secondary" sx={{ px: 0.25, lineHeight: 1.5 }}>
          The master gives this agent its tasks. You can’t chat with it directly — comment on a run instead. A comment on
          a run that’s still working redirects it straight away, and every comment is kept as a note it gets on future
          tasks.
        </Typography>

        {agent.feedback.length > 0 && (
          <Stack sx={{ gap: 0.75 }}>
            <SectionLabel>What it remembers from you · {agent.feedback.length}</SectionLabel>
            <Stack sx={{ gap: 0.5 }}>
              {agent.feedback
                .map((f, i) => ({ f, i }))
                .reverse()
                .map(({ f, i }) => (
                  <Stack
                    key={`${f.date}-${i}`}
                    direction="row"
                    sx={{ gap: 1, alignItems: 'flex-start', px: 1, py: 0.75, borderRadius: '8px', bgcolor: 'action.hover', '&:hover .forget': { opacity: 1 } }}
                  >
                    <Stack sx={{ flex: 1, minWidth: 0 }}>
                      <Typography variant="body2" sx={{ fontSize: '0.8125rem', whiteSpace: 'pre-wrap' }}>
                        {f.text}
                      </Typography>
                      <Typography variant="caption" color="text.disabled" sx={{ fontSize: '0.6875rem' }}>
                        {f.date}
                        {f.task ? ` · on “${f.task}”` : ''}
                      </Typography>
                    </Stack>
                    <Tooltip title="Forget this note">
                      <IconButton size="small" className="forget" onClick={() => forget(i)} sx={{ opacity: 0, transition: 'opacity 0.12s', p: 0.25 }}>
                        <TrashLinear size={13} color="currentColor" />
                      </IconButton>
                    </Tooltip>
                  </Stack>
                ))}
            </Stack>
          </Stack>
        )}

        <Stack sx={{ gap: 0.75 }}>
          <SectionLabel>Runs{runs.length ? ` · ${runs.length}` : ''}</SectionLabel>
          {runs.length === 0 ? (
            <Typography variant="caption" color="text.secondary" sx={{ px: 0.5 }}>
              No tasks yet. When the master delegates to {agent.name}, the work shows up here as it happens.
            </Typography>
          ) : (
            runs.map((r) => <RunCard key={r.id} run={r} open={openRun === r.id} onToggle={() => setOpenRun((cur) => (cur === r.id ? null : r.id))} />)
          )}
        </Stack>
      </Stack>
    </Box>
  )
}

function RunCard({ run, open, onToggle }: { run: AgentRun; open: boolean; onToggle: () => void }) {
  const live = run.status === 'running'
  return (
    <Box sx={{ borderRadius: '10px', border: 1, borderColor: live ? 'primary.main' : 'divider', overflow: 'hidden' }}>
      <ButtonBase
        onClick={onToggle}
        aria-expanded={open}
        sx={{ display: 'flex', width: '100%', alignItems: 'flex-start', justifyContent: 'flex-start', textAlign: 'left', gap: 1, px: 1.25, py: 1, '&:hover': { bgcolor: 'action.hover' } }}
      >
        <Stack sx={{ flex: 1, minWidth: 0, gap: 0.25 }}>
          <Typography variant="body2" sx={{ fontWeight: 600, fontSize: '0.8125rem' }} noWrap>
            {run.description || 'Task'}
          </Typography>
          <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem' }}>
            {ago(run.startedAt)} · {duration(run)} · {run.toolCalls} tool call{run.toolCalls === 1 ? '' : 's'}
            {run.comments.length ? ` · ${run.comments.length} comment${run.comments.length === 1 ? '' : 's'}` : ''}
          </Typography>
        </Stack>
        <StatusPill status={run.status} />
      </ButtonBase>
      <Collapse in={open} unmountOnExit>
        <RunBody run={run} />
      </Collapse>
    </Box>
  )
}

function RunBody({ run }: { run: AgentRun }) {
  const theme = useTheme()
  const comment = useTeamStore((s) => s.comment)
  const [messages, setMessages] = useState<AgentMessage[] | null>(null)
  const [showTask, setShowTask] = useState(false)
  const [draft, setDraft] = useState('')
  const [sending, setSending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const live = run.status === 'running'

  // The transcript streams in while the run works; afterwards it is final.
  useEffect(() => {
    let alive = true
    const read = () =>
      void call<AgentMessage[]>('cmd_read_run_messages', { id: run.id }).then((m) => {
        if (alive) setMessages(m ?? [])
      })
    read()
    if (!live) return () => void (alive = false)
    const t = window.setInterval(read, 1500)
    return () => {
      alive = false
      window.clearInterval(t)
    }
  }, [run.id, live])

  const send = async () => {
    const text = draft.trim()
    if (!text) return
    setSending(true)
    setError(null)
    try {
      await comment(run, text)
      setDraft('')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSending(false)
    }
  }

  // The first message is the task itself, shown separately above.
  const transcript = messages ? (messages[0]?.type === 'user' ? messages.slice(1) : messages) : null

  return (
    <Stack sx={{ gap: 1.25, px: 1.25, pb: 1.25, borderTop: 1, borderColor: 'divider', pt: 1.25 }}>
      <Box>
        <ButtonBase onClick={() => setShowTask((v) => !v)} sx={{ fontSize: '0.75rem', color: 'text.secondary', borderRadius: '6px', px: 0.5 }}>
          {showTask ? 'Hide' : 'Show'} what it was asked
        </ButtonBase>
        <Collapse in={showTask} unmountOnExit>
          <Box sx={{ mt: 0.75, p: 1.25, borderRadius: '8px', bgcolor: alpha(theme.palette.text.primary, 0.035), fontSize: '0.8125rem', maxHeight: 260, overflow: 'auto' }}>
            <Markdown text={run.prompt} />
          </Box>
        </Collapse>
      </Box>

      <Box sx={{ height: 360, display: 'flex', flexDirection: 'column', borderRadius: '8px', border: 1, borderColor: 'divider', overflow: 'hidden' }}>
        {transcript === null ? (
          <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center' }}>
            <CircularProgress size={16} />
          </Stack>
        ) : (
          <MessageList
            messages={transcript}
            readOnly
            emptyText={live ? 'Starting…' : run.status === 'interrupted' ? 'The app closed while this ran; no transcript was saved.' : 'No transcript.'}
          />
        )}
      </Box>

      {run.comments.length > 0 && (
        <Stack sx={{ gap: 0.5 }}>
          {run.comments.map((c, i) => (
            <Stack key={i} sx={{ px: 1, py: 0.75, borderRadius: '8px', bgcolor: alpha(theme.palette.primary.main, 0.08) }}>
              <Typography variant="body2" sx={{ fontSize: '0.8125rem', whiteSpace: 'pre-wrap' }}>
                {c.text}
              </Typography>
              <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem' }}>
                You · {ago(c.at)} · {c.duringRun ? 'sent while it worked' : 'after it finished'} · remembered
              </Typography>
            </Stack>
          ))}
        </Stack>
      )}

      <Stack direction="row" sx={{ gap: 0.75, alignItems: 'flex-end' }}>
        <InputBase
          multiline
          maxRows={6}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              void send()
            }
          }}
          placeholder={live ? 'Steer it — it reads this on its next step…' : 'Comment on this work — it will remember next time…'}
          sx={{ flex: 1, fontSize: '0.8125rem', px: 1.25, py: 0.75, border: 1, borderColor: 'divider', borderRadius: '8px', '&.ui-focused': { borderColor: 'primary.main' } }}
        />
        <Button
          variant="contained"
          disableElevation
          disabled={sending || !draft.trim()}
          onClick={() => void send()}
          sx={{ minWidth: 0, px: 1.25, height: 34, borderRadius: '8px' }}
          aria-label={live ? 'Send steering comment' : 'Send comment'}
        >
          {sending ? <CircularProgress size={14} /> : <SendLinear size={14} color="currentColor" />}
        </Button>
      </Stack>
      {error && (
        <Typography variant="caption" color="error.main">
          {error}
        </Typography>
      )}
      {run.output && !live && (
        <Typography variant="caption" color="text.disabled" sx={{ fontSize: '0.6875rem' }}>
          Its final report went back to the master.
        </Typography>
      )}
    </Stack>
  )
}
