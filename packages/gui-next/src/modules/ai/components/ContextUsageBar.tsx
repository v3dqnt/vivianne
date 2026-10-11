import { useMemo, useState } from 'react'
import { Box, Popover, Stack, Tooltip, Typography, ButtonBase, IconButton, Snackbar, CircularProgress, alpha, useTheme } from '../../../ui'
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

const MONO = '"Geist Mono", ui-monospace, monospace'

/** How full the window is, as a ring with the percentage inside. */
function Ring({ pct, color, track }: { pct: number; color: string; track: string }) {
  const r = 20
  const c = 2 * Math.PI * r
  return (
    <Box sx={{ position: 'relative', width: 52, height: 52, flexShrink: 0 }}>
      <svg width="52" height="52" viewBox="0 0 52 52" aria-hidden>
        <circle cx="26" cy="26" r={r} fill="none" stroke={track} strokeWidth="5" />
        <circle
          cx="26"
          cy="26"
          r={r}
          fill="none"
          stroke={color}
          strokeWidth="5"
          strokeLinecap="round"
          strokeDasharray={`${(Math.max(pct, 1) / 100) * c} ${c}`}
          transform="rotate(-90 26 26)"
          style={{ transition: 'stroke-dasharray 300ms ease' }}
        />
      </svg>
      <Typography sx={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: '0.75rem', fontWeight: 600, fontFamily: MONO }}>
        {Math.round(pct)}%
      </Typography>
    </Box>
  )
}

/** A small labelled number in the popover's footer. */
function Stat({ label, value }: { label: string; value: string }) {
  return (
    <Stack sx={{ gap: 0.125 }}>
      <Typography sx={{ fontSize: '0.625rem', textTransform: 'uppercase', letterSpacing: 0.5, color: 'text.disabled' }}>{label}</Typography>
      <Typography sx={{ fontSize: '0.75rem', fontFamily: MONO }}>{value}</Typography>
    </Stack>
  )
}

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
        slotProps={{
          paper: {
            sx: {
              mt: -1,
              width: 340,
              borderRadius: '14px',
              border: 1,
              borderColor: 'divider',
              bgcolor: 'background.paper',
              overflow: 'hidden',
              boxShadow: theme.palette.mode === 'dark' ? '0 16px 40px rgba(0,0,0,0.5), 0 2px 6px rgba(0,0,0,0.3)' : '0 16px 40px rgba(0,0,0,0.12), 0 2px 6px rgba(0,0,0,0.06)',
            },
          },
        }}
      >
        {/* Header: a ring for how full the window is, and the numbers. */}
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1.75, p: 2, pb: 1.5 }}>
          <Ring pct={pct} color={accent} track={alpha(theme.palette.text.primary, 0.08)} />
          <Stack sx={{ minWidth: 0, flex: 1, gap: 0.25 }}>
            <Typography sx={{ fontSize: '0.875rem', fontWeight: 600 }}>Context window</Typography>
            <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>
              <Box component="span" sx={{ color: 'text.primary', fontFamily: MONO }}>{formatTokens(stats.contextTokens)}</Box> of{' '}
              <Box component="span" sx={{ fontFamily: MONO }}>{formatTokens(stats.contextWindow)}</Box> used
            </Typography>
            <Typography noWrap sx={{ fontSize: '0.6875rem', color: 'text.disabled' }}>
              {stats.modelName || stats.modelId} · {formatTokens(Math.max(0, stats.contextWindow - stats.contextTokens))} left
            </Typography>
          </Stack>
        </Stack>

        {/* The whole window as one bar, a segment per category, headroom left empty. */}
        <Box sx={{ px: 2 }}>
          <Box sx={{ display: 'flex', gap: '2px', height: 8, borderRadius: 4, overflow: 'hidden', bgcolor: alpha(theme.palette.text.primary, 0.06) }}>
            {rows.map((r) => (
              <Box key={r.key} sx={{ width: `${Math.max(r.share, 0.6)}%`, bgcolor: colors[r.key], transition: 'width 240ms ease' }} />
            ))}
          </Box>
        </Box>

        <Stack sx={{ px: 1, py: 1.25, gap: 0.25 }}>
          {rows.map((r) => (
            <Tooltip key={r.key} title={r.hint} placement="left">
              <Stack direction="row" sx={{ alignItems: 'center', gap: 1.25, px: 1, py: 0.625, borderRadius: '9px', '&:hover': { bgcolor: 'action.hover' } }}>
                <Box sx={{ width: 10, height: 10, borderRadius: '3px', bgcolor: colors[r.key], flexShrink: 0 }} />
                <Typography noWrap sx={{ fontSize: '0.8125rem', flex: 1, minWidth: 0 }}>
                  {r.label}
                </Typography>
                <Box sx={{ width: 64, height: 4, borderRadius: 2, bgcolor: alpha(theme.palette.text.primary, 0.06), overflow: 'hidden', flexShrink: 0 }}>
                  <Box sx={{ width: `${accounted ? Math.max(4, (r.tokens / accounted) * 100) : 0}%`, height: '100%', bgcolor: colors[r.key] }} />
                </Box>
                <Typography sx={{ width: 44, textAlign: 'right', fontSize: '0.75rem', color: 'text.secondary', fontFamily: MONO }}>
                  {formatTokens(r.tokens)}
                </Typography>
              </Stack>
            </Tooltip>
          ))}
          {/* `contextTokens` counts the conversation; the rows also count the
              standing prompt and tool schemas. Said, rather than hidden. */}
          {accounted > stats.contextTokens && (
            <Typography sx={{ px: 1, pt: 0.75, fontSize: '0.6875rem', lineHeight: 1.5, color: 'text.disabled' }}>
              {formatTokens(accounted - stats.contextTokens)} of this is the standing prompt and tool schemas, sent with every request
              and outside the percentage.
            </Typography>
          )}
        </Stack>

        {/* Session totals and the one action this view offers. */}
        <Stack direction="row" sx={{ gap: 1, px: 2, py: 1.25, borderTop: 1, borderColor: 'divider', bgcolor: alpha(theme.palette.text.primary, 0.02) }}>
          <Stat label="In" value={formatTokens(stats.inputTokens)} />
          <Stat label="Out" value={formatTokens(stats.outputTokens)} />
          {stats.cacheRead > 0 && <Stat label="Cached" value={formatTokens(stats.cacheRead)} />}
          {sessionCost && <Stat label="Cost" value={formatCost(sessionCost.cost)} />}
          <Box sx={{ flex: 1 }} />
          <Tooltip title={isBusy ? 'Wait for the current turn to finish' : 'Fold older history into a summary to free room'}>
            <Box component="span" sx={{ display: 'flex', alignItems: 'center' }}>
              <ButtonBase
                disabled={compacting || isBusy || messageCount === 0}
                onClick={() => void runCompact()}
                sx={{
                  gap: 0.75,
                  px: 1.25,
                  height: 28,
                  borderRadius: '999px',
                  fontSize: '0.75rem',
                  fontWeight: 600,
                  border: 1,
                  borderColor: 'divider',
                  bgcolor: 'background.card',
                  '&:hover': { bgcolor: 'action.hover' },
                  '&.Mui-disabled, &:disabled': { opacity: 0.5 },
                }}
              >
                {compacting ? <CircularProgress size={12} /> : <ArchiveLinear size={13} color="currentColor" />}
                Compact
              </ButtonBase>
            </Box>
          </Tooltip>
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
