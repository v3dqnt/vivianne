import { useMemo, useState } from 'react'
import { Box, Popover, Stack, Tooltip, Typography, ButtonBase, IconButton, Snackbar, CircularProgress, useTheme } from '../../../ui'
import { ArchiveLinear, ContextLinear } from '../../../icons'
import { useModelStore } from '../store/modelStore'
import { useAgentStore } from '../store/agentStore'

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1000) return `${(n / 1000).toFixed(1)}K`
  return String(n)
}

function formatCost(n: number): string {
  return n < 0.01 && n > 0 ? '<$0.01' : `$${n.toFixed(2)}`
}

/** Ordered widest-lever-first: the two rows a user can act on immediately
 * (compact history, disable tools) sit above the ones they can't. */
const SEGMENTS = [
  {
    key: 'systemPrompt' as const,
    label: 'System prompt',
    hint: 'Persona, directive, and the generated capability map. Fixed cost on every request.',
  },
  {
    key: 'toolSchemas' as const,
    label: 'Tool schemas',
    hint: 'JSON schema for every enabled tool, sent on every request. Disabling unused tools in Settings → Mods reclaims this.',
  },
  { key: 'toolResults' as const, label: 'Tool results', hint: 'Output from tool calls — usually the largest and most compactable part.' },
  { key: 'assistant' as const, label: 'Replies', hint: "The agent's own messages, including reasoning." },
  { key: 'user' as const, label: 'Your messages', hint: 'Your prompts and attachments.' },
]

export function ContextUsageBar() {
  const theme = useTheme()
  const stats = useModelStore((s) => s.stats)
  const sessionCost = useModelStore((s) => s.sessionCost)
  const compacting = useAgentStore((s) => s.compacting)
  const isBusy = useAgentStore((s) => s.isBusy)
  const messageCount = useAgentStore((s) => s.messages.length)
  const compactNow = useAgentStore((s) => s.compactNow)
  const [toast, setToast] = useState<string | null>(null)
  const [anchor, setAnchor] = useState<HTMLElement | null>(null)

  // Distinct hues per segment so the stacked bar and the legend below it are
  // read as one object rather than two separate readouts.
  const colors = useMemo(
    () => ({
      systemPrompt: theme.palette.text.disabled,
      toolSchemas: theme.palette.info.main,
      toolResults: theme.palette.warning.main,
      assistant: theme.palette.primary.main,
      user: theme.palette.success.main,
    }),
    [theme],
  )

  if (!stats) return null

  // `contextPercent` comes through as NaN before a model is actually
  // selected (0 tokens / 0 window on the backend's default ModelRef) — guard
  // rather than render the literal string "NaN%".
  const pct = Number.isFinite(stats.contextPercent) ? Math.min(100, stats.contextPercent) : 0
  const accent = pct > 90 ? theme.palette.error.main : pct > 70 ? theme.palette.warning.main : theme.palette.primary.main

  const breakdown = stats.breakdown
  const window = Math.max(1, stats.contextWindow)
  // The bar is scaled against the whole window, not against the used portion,
  // so the empty remainder stays visible as headroom.
  const rows = SEGMENTS.map((seg) => {
    const tokens = breakdown?.[seg.key] ?? 0
    return { ...seg, tokens, share: (tokens / window) * 100 }
  }).filter((r) => r.tokens > 0)

  const accounted = rows.reduce((sum, r) => sum + r.tokens, 0)

  const runCompact = async () => {
    const folded = await compactNow()
    setToast(folded > 0 ? `Compacted ${folded} messages into a summary.` : 'Nothing to compact yet.')
  }

  return (
    <>
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5, px: 0.5 }}>
        <Tooltip title={`Context ${Math.round(pct)}% full — click for the breakdown`}>
          <ButtonBase
            className="cq-context"
            aria-label={`Context ${Math.round(pct)}% full`}
            onClick={(e) => setAnchor(e.currentTarget)}
            sx={{
              display: 'flex',
              alignItems: 'center',
              gap: 1,
              minWidth: 90,
              borderRadius: 1,
              px: 0.5,
              py: 0.25,
              '&:hover': { bgcolor: 'action.hover' },
            }}
          >
            {/* Stacked segments instead of one opaque fill: the shape itself
                shows which category is eating the window. */}
            {/* Compact: the context icon, tinted by how full the window is, and
                the percentage — for a composer too narrow for the bar. Only
                one form is shown, by the composer's container query. */}
            <Box component="span" className="cq-compact" sx={{ alignItems: 'center', gap: 0.5, color: accent }}>
              <ContextLinear size={13} color="currentColor" />
              <Typography component="span" variant="caption" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.6875rem' }}>
                {Math.round(pct)}%
              </Typography>
            </Box>
            <Box
              className="cq-wide"
              sx={{
                flex: 1,
                display: 'flex',
                height: 4,
                borderRadius: 2,
                overflow: 'hidden',
                bgcolor: 'action.hover',
              }}
            >
              {rows.map((r) => (
                <Box key={r.key} sx={{ width: `${r.share}%`, bgcolor: colors[r.key], transition: 'width 240ms ease' }} />
              ))}
            </Box>
            <Typography className="cq-wide" variant="caption" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.6875rem', color: accent }}>
              {Math.round(pct)}%
            </Typography>
          </ButtonBase>
        </Tooltip>

        <Tooltip title={isBusy ? 'Wait for the current turn to finish' : 'Compact context — fold older history into a summary'}>
          {/* Span wrapper: a disabled button fires no events, so Tooltip
              needs a non-disabled element to attach its listeners to. */}
          <Box component="span" sx={{ display: 'flex' }}>
            <IconButton
              size="small"
              onClick={() => void runCompact()}
              disabled={compacting || isBusy || messageCount === 0}
              sx={{ color: 'text.secondary', '&:hover': { color: 'text.primary' } }}
            >
              {compacting ? <CircularProgress size={13} /> : <ArchiveLinear size={14} color="currentColor" />}
            </IconButton>
          </Box>
        </Tooltip>
      </Box>

      <Popover
        open={!!anchor}
        anchorEl={anchor}
        onClose={() => setAnchor(null)}
        anchorOrigin={{ vertical: 'top', horizontal: 'left' }}
        transformOrigin={{ vertical: 'bottom', horizontal: 'left' }}
        slotProps={{ paper: { sx: { p: 2, width: 320, borderRadius: 3 } } }}
      >
        <Stack sx={{ gap: 0.25, mb: 1.5 }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            Context
          </Typography>
          <Typography variant="caption" color="text.secondary">
            {formatTokens(stats.contextTokens)} of {formatTokens(stats.contextWindow)} used · {formatTokens(Math.max(0, stats.contextWindow - stats.contextTokens))} left
          </Typography>
        </Stack>

        <Stack sx={{ gap: 0.75 }}>
          {rows.map((r) => (
            <Tooltip key={r.key} title={r.hint} placement="left">
              <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
                <Box sx={{ width: 8, height: 8, borderRadius: '50%', bgcolor: colors[r.key], flexShrink: 0 }} />
                <Typography variant="caption" sx={{ flex: 1, minWidth: 0 }} noWrap>
                  {r.label}
                </Typography>
                <Typography variant="caption" color="text.secondary" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace' }}>
                  {formatTokens(r.tokens)}
                </Typography>
                <Typography variant="caption" color="text.secondary" sx={{ width: 34, textAlign: 'right', fontFamily: '"Geist Mono", ui-monospace, monospace' }}>
                  {r.share < 1 ? '<1' : Math.round(r.share)}%
                </Typography>
              </Stack>
            </Tooltip>
          ))}
        </Stack>

        {/* `contextTokens` is computed over the message list only, while the
            rows above also count the always-present system prompt and tool
            schemas. Showing the gap is more honest than quietly reconciling
            two numbers that genuinely measure different things. */}
        {accounted > stats.contextTokens && (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mt: 1.5, lineHeight: 1.5 }}>
            {formatTokens(accounted - stats.contextTokens)} of this is the standing prompt and tool schemas, which sit outside the
            conversation history the percentage counts.
          </Typography>
        )}

        <Stack direction="row" sx={{ justifyContent: 'space-between', mt: 1.5, pt: 1.5, borderTop: 1, borderColor: 'divider' }}>
          <Typography variant="caption" color="text.secondary">
            {stats.inputTokens} in · {stats.outputTokens} out this session
          </Typography>
          {sessionCost && (
            <Typography variant="caption" color="text.secondary" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace' }}>
              {formatCost(sessionCost.cost)}
            </Typography>
          )}
        </Stack>
      </Popover>

      <Snackbar
        open={!!toast}
        autoHideDuration={3000}
        onClose={() => setToast(null)}
        message={toast}
        anchorOrigin={{ vertical: 'bottom', horizontal: 'center' }}
      />
    </>
  )
}
