import { useState } from 'react'
import { Box, Stack, Typography, ButtonBase, Collapse, alpha, useTheme } from '../../../ui'
import { Markdown } from './Markdown'

/** Matches `agent::days::HANDOFF_TAG`. */
export const HANDOFF_TAG = '<previous_day'

export function isHandoffText(text: string): boolean {
  return text.trimStart().startsWith(HANDOFF_TAG)
}

/** Pull the date, kind and body out of a `<previous_day …>…</previous_day>` note. */
function parse(text: string): { date: string | null; kind: string | null; body: string } {
  const open = text.match(/<previous_day([^>]*)>/)
  const attrs = open?.[1] ?? ''
  const date = attrs.match(/date="([^"]+)"/)?.[1] ?? null
  const kind = attrs.match(/kind="([^"]+)"/)?.[1] ?? null
  const body = text
    .replace(/<previous_day[^>]*>/, '')
    .replace(/<\/previous_day>\s*$/, '')
    // The first paragraph is an instruction to the model, not something the
    // reader needs to see.
    .replace(/^\s*This is a handoff note[^\n]*\n(?:[^\n]*\n)?/, '')
    .trim()
  return { date, kind, body }
}

function formatDay(day: string): string {
  const [y, m, d] = day.split('-').map(Number)
  const date = new Date(y, (m ?? 1) - 1, d ?? 1)
  return date.toLocaleDateString(undefined, { weekday: 'long', month: 'short', day: 'numeric' })
}

/**
 * The note a new day opens with, carrying yesterday forward.
 *
 * It travels to the model as a `User` message — the only message kind that
 * survives into the provider request — but it is not something the user said,
 * so it must not look like it. It renders as a divider between days instead,
 * collapsed by default: the reader already knows what they did yesterday, and
 * the note is there for the model.
 */
export function HandoffDivider({ text }: { text: string }) {
  const theme = useTheme()
  const [open, setOpen] = useState(false)
  const { date, kind, body } = parse(text)
  const label = date ? `Continued from ${formatDay(date)}` : 'Continued from the previous day'

  return (
    <Stack sx={{ gap: 0.75, py: 0.5 }}>
      <ButtonBase
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        sx={{
          display: 'flex',
          alignItems: 'center',
          gap: 1.25,
          width: '100%',
          borderRadius: '8px',
          py: 0.5,
          '&:focus-visible': { outline: `2px solid ${theme.palette.primary.main}`, outlineOffset: 2 },
        }}
      >
        <Box sx={{ flex: 1, height: '1px', bgcolor: 'divider' }} />
        <Typography variant="caption" sx={{ color: 'text.secondary', fontWeight: 600, whiteSpace: 'nowrap' }}>
          {label}
        </Typography>
        <Typography variant="caption" sx={{ color: 'text.disabled', whiteSpace: 'nowrap' }}>
          {open ? 'Hide' : 'Show'} handoff
        </Typography>
        <Box sx={{ flex: 1, height: '1px', bgcolor: 'divider' }} />
      </ButtonBase>
      <Collapse in={open} unmountOnExit>
        <Box
          sx={{
            px: 2,
            py: 1.5,
            borderRadius: '12px',
            bgcolor: alpha(theme.palette.text.primary, 0.035),
            border: 1,
            borderColor: 'divider',
          }}
        >
          <Box sx={{ fontSize: '0.8125rem', color: 'text.secondary' }}>
            <Markdown text={body} />
          </Box>
          {/* Says which kind of note this is, because they differ in what they
              can be trusted for: a digest is literally what was asked, a
              summary is a model's reading of it. */}
          <Typography variant="caption" sx={{ display: 'block', mt: 1, color: 'text.disabled' }}>
            {kind === 'summary'
              ? 'Written by the model from that day’s conversation.'
              : 'Built from that day’s requests — a fuller summary replaces it if one can be written before you start.'}
          </Typography>
        </Box>
      </Collapse>
    </Stack>
  )
}

/** Matches `agent::conversation::SUMMARY_TAG` — the note compaction puts at
 * the front of the model's context in place of the messages it removed. */
const SUMMARY_TAG = '<compacted_history>'

export function isSummaryText(text: string): boolean {
  return text.trimStart().startsWith(SUMMARY_TAG)
}

/**
 * Where the model's memory of the conversation switches from word-for-word to
 * a summary. Everything above it is still shown to the user in full; this is
 * what the agent carries of it, available on demand.
 */
export function SummaryDivider({ text }: { text: string }) {
  const theme = useTheme()
  const [open, setOpen] = useState(false)
  const body = text.replace(/<\/?compacted_history>/g, '').trim()
  return (
    <Stack sx={{ gap: 0.75, py: 0.5 }}>
      <ButtonBase
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        sx={{
          display: 'flex',
          alignItems: 'center',
          gap: 1.25,
          width: '100%',
          borderRadius: '8px',
          py: 0.5,
          '&:focus-visible': { outline: `2px solid ${theme.palette.primary.main}`, outlineOffset: 2 },
        }}
      >
        <Box sx={{ flex: 1, height: '1px', bgcolor: 'divider' }} />
        <Typography variant="caption" sx={{ color: 'text.secondary', fontWeight: 600, whiteSpace: 'nowrap' }}>
          Above this, the agent works from a summary
        </Typography>
        <Typography variant="caption" sx={{ color: 'text.disabled', whiteSpace: 'nowrap' }}>
          {open ? 'Hide' : 'Show'} it
        </Typography>
        <Box sx={{ flex: 1, height: '1px', bgcolor: 'divider' }} />
      </ButtonBase>
      <Collapse in={open} unmountOnExit>
        <Box sx={{ px: 2, py: 1.5, borderRadius: '12px', bgcolor: alpha(theme.palette.text.primary, 0.035), border: 1, borderColor: 'divider' }}>
          <Box sx={{ fontSize: '0.8125rem', color: 'text.secondary' }}>
            <Markdown text={body} />
          </Box>
          <Typography variant="caption" sx={{ display: 'block', mt: 1, color: 'text.disabled' }}>
            Long conversations are condensed so the model has room to work. Nothing is deleted — every message stays above.
          </Typography>
        </Box>
      </Collapse>
    </Stack>
  )
}

/** Matches `multiplayer::collab::PAIR_ACTIVITY_TAG`: what the partner's
 * agent did, added to the conversation right before a run so this agent
 * knows. Shown as a divider — it isn't something the user said. */
export function isPairActivityText(text: string): boolean {
  return text.trimStart().startsWith('<pair_activity')
}

export function PairActivityDivider({ text }: { text: string }) {
  const theme = useTheme()
  const [open, setOpen] = useState(false)
  const partner = text.match(/partner="([^"]*)"/)?.[1] ?? 'Your partner'
  const lines = text
    .replace(/<pair_activity[^>]*>/, '')
    .replace(/<\/pair_activity>\s*$/, '')
    .split('\n')
    .filter((l) => l.startsWith('- ') || l.startsWith('⚠') || l.startsWith('('))
  const updates = lines.filter((l) => l.startsWith('- ')).length
  return (
    <Stack sx={{ gap: 0.75, py: 0.5 }}>
      <ButtonBase
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        sx={{ display: 'flex', alignItems: 'center', gap: 1.25, width: '100%', borderRadius: '8px', py: 0.5 }}
      >
        <Box sx={{ flex: 1, height: '1px', bgcolor: 'divider' }} />
        <Typography variant="caption" sx={{ color: 'success.main', fontWeight: 600, whiteSpace: 'nowrap' }}>
          {partner}: {updates || 'an'} update{updates === 1 ? '' : 's'} shared with your agent
        </Typography>
        <Typography variant="caption" sx={{ color: 'text.disabled', whiteSpace: 'nowrap' }}>
          {open ? 'Hide' : 'Show'}
        </Typography>
        <Box sx={{ flex: 1, height: '1px', bgcolor: 'divider' }} />
      </ButtonBase>
      <Collapse in={open} unmountOnExit>
        <Box sx={{ px: 2, py: 1.25, borderRadius: '12px', bgcolor: alpha(theme.palette.success.main, 0.05), border: 1, borderColor: 'divider' }}>
          {lines.map((l, i) => (
            <Typography key={i} variant="caption" color={l.startsWith('⚠') ? 'warning.main' : 'text.secondary'} sx={{ display: 'block', lineHeight: 1.5, py: 0.25 }}>
              {l}
            </Typography>
          ))}
        </Box>
      </Collapse>
    </Stack>
  )
}

/** Matches `run_ledger::cmd_long_task_resume_prompt`: the message that picks
 * a long task back up. It is the app speaking, not the user. */
export function isResumeText(text: string): boolean {
  return text.trimStart().startsWith('<long_task_resumed>')
}

export function ResumeDivider({ text }: { text: string }) {
  const theme = useTheme()
  const [open, setOpen] = useState(false)
  const body = (text.match(/<long_task>([\s\S]*?)<\/long_task>/)?.[1] ?? '').trim()
  return (
    <Stack sx={{ gap: 0.75, py: 0.5 }}>
      <ButtonBase
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        sx={{ display: 'flex', alignItems: 'center', gap: 1.25, width: '100%', borderRadius: '8px', py: 0.5 }}
      >
        <Box sx={{ flex: 1, height: '1px', bgcolor: 'divider' }} />
        <Typography variant="caption" sx={{ color: 'text.secondary', fontWeight: 600, whiteSpace: 'nowrap' }}>
          Picked up where it left off
        </Typography>
        <Typography variant="caption" sx={{ color: 'text.disabled', whiteSpace: 'nowrap' }}>
          {open ? 'Hide' : 'Show'} where it stood
        </Typography>
        <Box sx={{ flex: 1, height: '1px', bgcolor: 'divider' }} />
      </ButtonBase>
      <Collapse in={open} unmountOnExit>
        <Box
          sx={{
            px: 2,
            py: 1.25,
            borderRadius: '12px',
            bgcolor: alpha(theme.palette.text.primary, 0.035),
            border: 1,
            borderColor: 'divider',
            whiteSpace: 'pre-wrap',
          }}
        >
          <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.6 }}>
            {body}
          </Typography>
        </Box>
      </Collapse>
    </Stack>
  )
}
