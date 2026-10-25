import { useState } from 'react'
import { Box, ButtonBase, CircularProgress, Stack, Typography, alpha, useTheme } from '../../../ui'
import { CheckLinear, CloseLinear, ChevronRightLinear } from '../../../icons'
import type { BotStep } from '../store/botsStore'
import type { TeamAgent } from '../store/teamStore'
import { Avatar, colorOf } from './bits'
import { faceState } from './AgentFace'

/** How many steps show before "N earlier" folds the rest away. */
const SHOWN = 8

function StepRow({ step }: { step: BotStep }) {
  const theme = useTheme()
  return (
    <Stack direction="row" sx={{ gap: 1, alignItems: 'flex-start', py: 0.375 }}>
      <Box sx={{ width: 14, height: 18, flexShrink: 0, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        {step.state === 'running' ? (
          <CircularProgress size={11} />
        ) : step.state === 'error' ? (
          <CloseLinear size={12} color={theme.palette.error.main} />
        ) : (
          <CheckLinear size={12} color={theme.palette.success.main} />
        )}
      </Box>
      <Stack sx={{ minWidth: 0, flex: 1 }}>
        <Typography variant="caption" sx={{ fontSize: '0.75rem', color: step.state === 'running' ? 'text.primary' : 'text.secondary', wordBreak: 'break-word' }}>
          {step.label}
        </Typography>
        {step.detail && (
          <Typography
            variant="caption"
            sx={{
              fontSize: '0.6875rem',
              fontFamily: '"Geist Mono", ui-monospace, monospace',
              color: step.state === 'error' ? 'error.main' : 'text.disabled',
              whiteSpace: 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
            }}
          >
            {step.detail}
          </Typography>
        )}
      </Stack>
    </Stack>
  )
}

/** A run's steps, newest last; long runs fold their early steps. */
export function StepList({ steps }: { steps: BotStep[] }) {
  const [all, setAll] = useState(false)
  const hidden = all ? 0 : Math.max(0, steps.length - SHOWN)
  return (
    <Stack>
      {hidden > 0 && (
        <ButtonBase onClick={() => setAll(true)} sx={{ alignSelf: 'flex-start', fontSize: '0.6875rem', color: 'text.secondary', py: 0.25 }}>
          {hidden} earlier step{hidden === 1 ? '' : 's'}
        </ButtonBase>
      )}
      {steps.slice(hidden).map((s) => (
        <StepRow key={s.id} step={s} />
      ))}
    </Stack>
  )
}

/** "N steps" under a finished reply: what the agent did to get there. */
export function StepsToggle({ steps }: { steps: BotStep[] }) {
  const [open, setOpen] = useState(false)
  const failed = steps.filter((s) => s.state === 'error').length
  return (
    <Stack sx={{ mt: 0.5 }}>
      <ButtonBase
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        sx={{ alignSelf: 'flex-start', gap: 0.5, fontSize: '0.6875rem', color: 'text.secondary', borderRadius: 1, px: 0.5, ml: -0.5, '&:hover': { color: 'text.primary' } }}
      >
        <Box component="span" sx={{ display: 'inline-flex', transform: open ? 'rotate(90deg)' : 'none', transition: 'transform 120ms' }}>
          <ChevronRightLinear size={11} color="currentColor" />
        </Box>
        {steps.length} step{steps.length === 1 ? '' : 's'}
        {failed > 0 ? ` · ${failed} failed` : ''}
      </ButtonBase>
      {open && (
        <Box sx={{ pl: 1, ml: 0.5, borderLeft: 1, borderColor: 'divider' }}>
          <StepList steps={steps} />
        </Box>
      )}
    </Stack>
  )
}

/** An agent at work in this conversation: what it's doing, step by step. */
export function LiveRunCard({ agent, steps }: { agent: TeamAgent; steps: BotStep[] }) {
  const theme = useTheme()
  const color = colorOf(agent)
  const current = [...steps].reverse().find((s) => s.state === 'running')
  const face = faceState({ root: '', botId: agent.id, state: 'working', activity: null }, { channel: '', steps }, undefined, undefined)
  return (
    <Stack direction="row" sx={{ gap: 1.25, mt: 1.5, alignItems: 'flex-start' }}>
      <Box sx={{ width: 28, flexShrink: 0 }}>
        <Avatar name={agent.name} color={color} face={agent.face} size={28} state={face} />
      </Box>
      <Stack sx={{ minWidth: 0, flex: 1 }}>
        <Stack direction="row" sx={{ alignItems: 'baseline', gap: 0.75 }}>
          <Typography variant="caption" sx={{ fontWeight: 700, color }}>
            {agent.name}
          </Typography>
          <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem' }}>
            {current ? 'working' : steps.length ? 'thinking about what it found' : 'thinking'}
          </Typography>
        </Stack>
        <Box
          sx={{
            mt: 0.5,
            px: 1.25,
            py: 0.75,
            borderRadius: '10px',
            border: 1,
            borderColor: 'divider',
            bgcolor: alpha(theme.palette.text.primary, 0.025),
          }}
        >
          {steps.length === 0 ? (
            <Stack direction="row" sx={{ gap: 1, alignItems: 'center', py: 0.25 }}>
              <CircularProgress size={11} />
              <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.75rem' }}>
                Working out what to do…
              </Typography>
            </Stack>
          ) : (
            <StepList steps={steps} />
          )}
        </Box>
      </Stack>
    </Stack>
  )
}
