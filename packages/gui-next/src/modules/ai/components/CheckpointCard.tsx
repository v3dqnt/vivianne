import { useState } from 'react'
import { Box, ButtonBase, Collapse, Stack, Typography } from '../../../ui'
import { TickCircleLinear } from '../../../icons'
import type { Checkpoint } from '../lib/checkpoints'
import { TOOL_BLOCK_SX } from './ToolCallCard'

/** "40 s", "15 min", "2 h 5 min" — how long a piece of work took, without
 * a countdown's precision. */
function took(ms: number): string {
  const min = Math.round(ms / 60_000)
  if (min < 1) return `${Math.max(1, Math.round(ms / 1000))} s`
  if (min < 60) return `${min} min`
  const h = Math.floor(min / 60)
  return min % 60 ? `${h} h ${min % 60} min` : `${h} h`
}

/** "Task 2 of 5", "Tasks 2–3 of 5". */
function positionLabel(c: Checkpoint): string {
  const [first, last] = [c.positions[0], c.positions[c.positions.length - 1]]
  return c.positions.length === 1 ? `Task ${first} of ${c.total}` : `Tasks ${first}–${last} of ${c.total}`
}

/**
 * A finished part of the plan, as a marker in the conversation — so a task
 * that ran for hours reads as the progress it made, not as a wall of tool
 * calls. Shows what was done, how long it took, and what changed; the file
 * list is a click away.
 */
export function CheckpointCard({ checkpoint: c }: { checkpoint: Checkpoint }) {
  const [open, setOpen] = useState(false)
  const meta: string[] = []
  if (c.files.length) meta.push(`${c.files.length} file${c.files.length === 1 ? '' : 's'}`)
  if (c.elapsedMs >= 1000) meta.push(took(c.elapsedMs))
  if (c.costUsd !== null && c.costUsd >= 0.005) meta.push(`$${c.costUsd.toFixed(2)}`)
  const allDone = c.done === c.total

  return (
    <Stack sx={TOOL_BLOCK_SX}>
      <ButtonBase
        onClick={() => c.files.length && setOpen((v) => !v)}
        aria-expanded={c.files.length ? open : undefined}
        sx={{
          display: 'flex',
          alignItems: 'flex-start',
          justifyContent: 'flex-start',
          gap: 1,
          px: 1.5,
          py: 1,
          borderRadius: '12px',
          border: 1,
          borderColor: 'divider',
          bgcolor: 'background.paper',
          textAlign: 'left',
          cursor: c.files.length ? 'pointer' : 'default',
          '&:hover': c.files.length ? { bgcolor: 'action.hover' } : undefined,
        }}
      >
        <Box sx={{ display: 'flex', color: 'success.main', pt: 0.125, flexShrink: 0 }}>
          <TickCircleLinear size={16} color="currentColor" />
        </Box>
        <Box sx={{ minWidth: 0, flex: 1 }}>
          <Stack direction="row" sx={{ alignItems: 'baseline', gap: 1, flexWrap: 'wrap' }}>
            <Typography variant="caption" sx={{ fontWeight: 600 }}>
              {positionLabel(c)} done{allDone ? ' — plan complete' : ''}
            </Typography>
            {meta.length > 0 && (
              <Typography variant="caption" color="text.disabled">
                {meta.join(' · ')}
              </Typography>
            )}
          </Stack>
          {c.items.map((t, i) => (
            <Typography key={i} variant="caption" color="text.secondary" sx={{ display: 'block', lineHeight: 1.5 }}>
              {t.content}
              {t.summary ? <Box component="span" sx={{ color: 'text.disabled' }}> — {t.summary}</Box> : null}
            </Typography>
          ))}
        </Box>
      </ButtonBase>
      <Collapse in={open} unmountOnExit>
        <Stack sx={{ px: 1.5, pt: 0.75, gap: 0.25 }}>
          {c.files.map((f) => (
            <Typography key={f} variant="caption" color="text.secondary" noWrap sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace' }}>
              {f}
            </Typography>
          ))}
        </Stack>
      </Collapse>
    </Stack>
  )
}
