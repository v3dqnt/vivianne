import { useEffect, useState } from 'react'
import { Box, ButtonBase, CircularProgress, Collapse, Stack, Typography, alpha, useTheme } from '../../../ui'
import { ArrowDownLinear, TickCircleLinear, DangerLinear } from '../../../icons'
import { Markdown } from '../../ai/components/Markdown'
import { AgentMark } from '../../../ui/brand/AgentMark'
import { SUBAGENT_COLORS, SUBAGENT_COLOR_HEX } from '../../settings/lib/subagentProfiles'
import { useTeamRunStore, type TeamRunMember } from '../store/teamRunStore'

function hex(color: string | null): string {
  const key = color as (typeof SUBAGENT_COLORS)[number] | null
  return (key && SUBAGENT_COLOR_HEX[key]) || SUBAGENT_COLOR_HEX.blue
}

const STATUS: Record<TeamRunMember['status'], string> = {
  starting: 'Starting',
  working: 'Working',
  done: 'Done',
  failed: 'Failed',
  stopped: 'Stopped',
}

function MemberRow({ m }: { m: TeamRunMember }) {
  const [open, setOpen] = useState(false)
  const color = hex(m.color)
  return (
    <Box>
      <ButtonBase
        onClick={() => m.output && setOpen((v) => !v)}
        aria-expanded={m.output ? open : undefined}
        sx={{
          display: 'flex',
          width: '100%',
          justifyContent: 'flex-start',
          alignItems: 'center',
          gap: 1,
          px: 1,
          py: 0.75,
          borderRadius: '8px',
          textAlign: 'left',
          cursor: m.output ? 'pointer' : 'default',
          '&:hover': m.output ? { bgcolor: 'action.hover' } : undefined,
        }}
      >
        <AgentMark color={color} size={22} title={m.name} />
        <Stack sx={{ minWidth: 0, flex: 1 }}>
          <Typography variant="caption" sx={{ fontWeight: 600 }} noWrap>
            {m.name}
          </Typography>
          <Typography variant="caption" color="text.secondary" noWrap sx={{ fontSize: '0.6875rem' }}>
            {m.role}
          </Typography>
        </Stack>
        {m.status === 'working' || m.status === 'starting' ? (
          <CircularProgress size={12} thickness={5} />
        ) : m.status === 'done' ? (
          <Box sx={{ display: 'flex', color: 'success.main' }}>
            <TickCircleLinear size={14} color="currentColor" />
          </Box>
        ) : (
          <Box sx={{ display: 'flex', color: m.status === 'failed' ? 'error.main' : 'text.disabled' }}>
            <DangerLinear size={14} color="currentColor" />
          </Box>
        )}
        <Typography variant="caption" color="text.disabled" sx={{ width: 52, textAlign: 'right', flexShrink: 0 }}>
          {STATUS[m.status]}
        </Typography>
        {m.output && (
          <Box sx={{ display: 'flex', color: 'text.secondary', transform: open ? 'rotate(180deg)' : 'none', transition: 'transform 0.15s' }}>
            <ArrowDownLinear size={14} color="currentColor" />
          </Box>
        )}
      </ButtonBase>
      <Collapse in={open} unmountOnExit>
        <Box sx={{ ml: 4.5, mr: 1, mb: 1, p: 1.25, borderRadius: '8px', border: 1, borderColor: 'divider', fontSize: '0.8125rem' }}>
          <Markdown text={m.output ?? ''} />
        </Box>
      </Collapse>
    </Box>
  )
}

/**
 * A team at work, live: every member with its state, and the messages
 * they send each other as they go. Each finished member's report opens
 * from its row. Renders a `team_run` tool call in the chat.
 */
export function TeamRunCard({ runId, pending }: { runId: string | null; pending: boolean }) {
  const theme = useTheme()
  const latest = useTeamRunStore((s) => s.latest)
  const id = runId ?? (pending ? latest : null)
  const run = useTeamRunStore((s) => (id ? s.runs[id] : undefined))
  const fetch = useTeamRunStore((s) => s.fetch)
  const [showTalk, setShowTalk] = useState(true)

  // A run finished in an earlier session isn't in memory: ask for it.
  useEffect(() => {
    if (id && !run) void fetch(id)
  }, [id, run, fetch])

  if (!run) {
    return (
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 1.5, py: 1, border: 1, borderColor: 'divider', borderRadius: '12px', maxWidth: '80%' }}>
        {pending && <CircularProgress size={12} thickness={5} />}
        <Typography variant="caption" color="text.secondary">
          {pending ? 'Starting the team…' : 'The team worked on this task (details are no longer in memory).'}
        </Typography>
      </Stack>
    )
  }

  const nameOf = (id: string) => (id === 'all' ? 'everyone' : (run.members.find((m) => m.id === id)?.name ?? id))
  const colorOf = (id: string) => hex(run.members.find((m) => m.id === id)?.color ?? null)
  const done = run.members.filter((m) => m.status === 'done').length

  return (
    <Box sx={{ border: 1, borderColor: 'divider', borderRadius: '12px', bgcolor: 'background.paper', maxWidth: '80%', overflow: 'hidden' }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 1.5, py: 1, borderBottom: 1, borderColor: 'divider' }}>
        <Stack direction="row" sx={{ gap: 0.25 }}>
          {run.members.slice(0, 5).map((m) => (
            <AgentMark key={m.id} color={hex(m.color)} size={18} />
          ))}
        </Stack>
        <Typography variant="caption" sx={{ fontWeight: 600, flex: 1, minWidth: 0 }} noWrap title={run.task}>
          {run.status === 'running' ? 'The team is working' : run.status === 'stopped' ? 'The team was stopped' : 'The team finished'}
          <Box component="span" sx={{ color: 'text.secondary', fontWeight: 400 }}>
            {' '}
            — {run.task}
          </Box>
        </Typography>
        <Typography variant="caption" color="text.disabled" sx={{ flexShrink: 0 }}>
          {done}/{run.members.length} done
        </Typography>
      </Stack>
      <Stack sx={{ p: 0.5 }}>
        {run.members.map((m) => (
          <MemberRow key={m.id} m={m} />
        ))}
      </Stack>
      {run.messages.length > 0 && (
        <Box sx={{ borderTop: 1, borderColor: 'divider' }}>
          <ButtonBase onClick={() => setShowTalk((v) => !v)} sx={{ gap: 0.5, px: 1.5, py: 0.75, color: 'text.secondary', width: '100%', justifyContent: 'flex-start' }}>
            <Typography variant="caption" sx={{ fontWeight: 600 }}>
              {run.messages.length} message{run.messages.length === 1 ? '' : 's'} between them
            </Typography>
            <Box sx={{ display: 'flex', transform: showTalk ? 'rotate(180deg)' : 'none' }}>
              <ArrowDownLinear size={12} color="currentColor" />
            </Box>
          </ButtonBase>
          <Collapse in={showTalk} unmountOnExit>
            <Stack sx={{ gap: 0.75, px: 1.5, pb: 1.25, maxHeight: 260, overflowY: 'auto' }}>
              {run.messages.map((msg, i) => (
                <Stack key={i} direction="row" sx={{ gap: 1, alignItems: 'flex-start' }}>
                  <Box sx={{ pt: 0.25 }}>
                    <AgentMark color={colorOf(msg.from)} size={16} />
                  </Box>
                  <Box sx={{ minWidth: 0, px: 1, py: 0.5, borderRadius: '8px', bgcolor: alpha(theme.palette.text.primary, 0.04) }}>
                    <Typography variant="caption" color="text.secondary" sx={{ display: 'block', fontSize: '0.6875rem' }}>
                      {nameOf(msg.from)} → {nameOf(msg.to)}
                    </Typography>
                    <Typography variant="caption" sx={{ display: 'block', whiteSpace: 'pre-wrap', lineHeight: 1.5 }}>
                      {msg.text}
                    </Typography>
                  </Box>
                </Stack>
              ))}
            </Stack>
          </Collapse>
        </Box>
      )}
    </Box>
  )
}
