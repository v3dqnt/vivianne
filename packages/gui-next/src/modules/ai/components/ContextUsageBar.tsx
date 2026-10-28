import { useMemo, useState } from 'react'
import { Box, Popover, Stack, Tooltip, Typography, ButtonBase, Snackbar, CircularProgress, alpha, useTheme } from '../../../ui'
import { ArchiveLinear } from '../../../icons'
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
    hint: 'JSON schema for every enabled tool, sent on every request. Disabling unused tools in Settings > Mods reclaims this.',
  },
  { key: 'toolResults' as const, label: 'Tool results', hint: 'Output from tool calls, usually the largest and most compactable part.' },
  { key: 'assistant' as const, label: 'Replies', hint: "The agent's own messages, including reasoning." },
  { key: 'user' as const, label: 'Your messages', hint: 'Your prompts and attachments.' },
]

/** One type family throughout; numbers line up by using tabular figures
 * rather than a second, monospaced face. */
const NUM = { fontVariantNumeric: 'tabular-nums' } as const

/** The context window as a ring: a faint track, with each category's share
 * of the window drawn as an arc in its own colour, in order from the top. */
function ContextRing({
  rows,
  colors,
  window,
  accent,
  size = 22,
}: {
  rows: { key: string; tokens: number }[]
  colors: Record<string, string>
  window: number
  accent: string
  size?: number
}) {
  const stroke = 3.5
  const r = (size - stroke) / 2
  const c = 2 * Math.PI * r
  let used = 0
  return (
    <Box component="span" sx={{ display: 'inline-flex', flexShrink: 0, width: size, height: size }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden>
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="currentColor" strokeOpacity={0.14} strokeWidth={stroke} />
        {rows.map((row) => {
          const len = Math.min(c - used, (row.tokens / window) * c)
          const offset = -used
          used += len
          if (len <= 0) return null
          return (
            <circle
              key={row.key}
              cx={size / 2}
              cy={size / 2}
              r={r}
              fill="none"
              stroke={colors[row.key] ?? accent}
              strokeWidth={stroke}
              strokeDasharray={`${len} ${c - len}`}
              strokeDashoffset={offset}
              transform={`rotate(-90 ${size / 2} ${size / 2})`}
              style={{ transition: 'stroke-dasharray 240ms ease, stroke-dashoffset 240ms ease' }}
            />
          )
        })}
      </svg>
    </Box>
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

  // One hue in steps rather than five unrelated colours: the categories
  // read as parts of one window, and only fullness gets a warning colour.
  const colors = useMemo(() => {
    const ink = theme.palette.text.primary
    return {
      systemPrompt: alpha(ink, 0.9),
      toolSchemas: alpha(ink, 0.65),
      toolResults: alpha(ink, 0.45),
      assistant: alpha(ink, 0.3),
      user: alpha(ink, 0.18),
    }
  }, [theme])

  if (!stats) return null

  // `contextPercent` comes through as NaN before a model is actually
  // selected (0 tokens / 0 window on the backend's default ModelRef) — guard
  // rather than render the literal string "NaN%".
  const pct = Number.isFinite(stats.contextPercent) ? Math.min(100, stats.contextPercent) : 0
  const accent = pct > 90 ? theme.palette.error.main : pct > 70 ? theme.palette.warning.main : theme.palette.text.primary

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
      <Box sx={{ display: 'flex', alignItems: 'center' }}>
        <Tooltip title={`Context ${Math.round(pct)}% full — click for the breakdown`}>
          <ButtonBase
            className="cq-context"
            aria-label={`Context ${Math.round(pct)}% full`}
            onClick={(e) => setAnchor(e.currentTarget)}
            sx={{
              display: 'flex',
              alignItems: 'center',
              gap: 0.75,
              borderRadius: '8px',
              px: 1,
              py: 0.375,
              '&:hover': { bgcolor: 'action.hover' },
            }}
          >
            {/* A ring: its arcs are the same segments as the breakdown, each in
                its own colour, so the shape shows which category is eating
                the window. The percentage sits beside it. */}
            <ContextRing rows={rows} colors={colors} window={window} accent={accent} />
            <Typography
              className="cq-wide"
              variant="caption"
              sx={{ ...NUM, fontSize: '12px', color: pct > 70 ? accent : 'text.secondary', minWidth: 28, textAlign: 'left' }}
            >
              {Math.round(pct)}%
            </Typography>
          </ButtonBase>
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
              width: 300,
              borderRadius: '12px',
              border: 1,
              borderColor: 'divider',
              bgcolor: 'background.paper',
              overflow: 'hidden',
              boxShadow: theme.elevation.lg,
            },
          },
        }}
      >
        <Stack sx={{ p: '16px', pb: '12px', gap: '4px' }}>
          <Stack direction="row" sx={{ alignItems: 'baseline', justifyContent: 'space-between' }}>
            <Typography sx={{ fontSize: '13px', fontWeight: 600 }}>Context</Typography>
            <Typography sx={{ ...NUM, fontSize: '13px', fontWeight: 600, color: pct > 70 ? accent : 'text.primary' }}>{Math.round(pct)}%</Typography>
          </Stack>
          <Typography noWrap sx={{ ...NUM, fontSize: '12px', color: 'text.secondary' }}>
            {formatTokens(stats.contextTokens)} of {formatTokens(stats.contextWindow)} on {stats.modelName || stats.modelId}
          </Typography>
        </Stack>

        {/* The whole window as one bar, a segment per category, headroom left empty. */}
        <Box sx={{ px: '16px' }}>
          <Box sx={{ display: 'flex', gap: '2px', height: 6, borderRadius: '3px', overflow: 'hidden', bgcolor: alpha(theme.palette.text.primary, 0.06) }}>
            {rows.map((r) => (
              <Box key={r.key} sx={{ width: `${Math.max(r.share, 0.6)}%`, bgcolor: pct > 70 ? accent : colors[r.key], opacity: pct > 70 ? 0.9 : 1, transition: 'width 240ms ease' }} />
            ))}
          </Box>
        </Box>

        <Stack sx={{ px: '8px', py: '8px' }}>
          {rows.map((r) => (
            <Tooltip key={r.key} title={r.hint} placement="left">
              <Stack direction="row" sx={{ alignItems: 'center', gap: '8px', height: 28, px: '8px', borderRadius: '6px', '&:hover': { bgcolor: 'action.hover' } }}>
                <Box sx={{ width: 8, height: 8, borderRadius: '2px', bgcolor: colors[r.key], flexShrink: 0 }} />
                <Typography noWrap sx={{ fontSize: '13px', flex: 1, minWidth: 0 }}>
                  {r.label}
                </Typography>
                <Typography sx={{ ...NUM, fontSize: '12px', color: 'text.secondary' }}>{formatTokens(r.tokens)}</Typography>
              </Stack>
            </Tooltip>
          ))}
          {/* `contextTokens` counts the conversation; the rows also count the
              standing prompt and tool schemas. Said, rather than hidden. */}
          {accounted > stats.contextTokens && (
            <Typography sx={{ px: '8px', pt: '4px', fontSize: '12px', lineHeight: 1.5, color: 'text.disabled' }}>
              Prompt and tool schemas ({formatTokens(accounted - stats.contextTokens)}) are sent with every request and not counted in the percentage.
            </Typography>
          )}
        </Stack>

        {/* Session totals and the one action this view offers. */}
        <Stack direction="row" sx={{ alignItems: 'center', gap: '12px', px: '16px', py: '10px', borderTop: 1, borderColor: 'divider' }}>
          <Typography noWrap sx={{ ...NUM, flex: 1, minWidth: 0, fontSize: '12px', color: 'text.secondary' }}>
            {formatTokens(stats.inputTokens)} in, {formatTokens(stats.outputTokens)} out
            {stats.cacheRead > 0 && `, ${formatTokens(stats.cacheRead)} cached`}
            {sessionCost && `, ${formatCost(sessionCost.cost)}`}
          </Typography>
          <Tooltip title={isBusy ? 'Wait for the current turn to finish' : 'Fold older history into a summary to free room'}>
            <Box component="span" sx={{ display: 'flex' }}>
              <ButtonBase
                disabled={compacting || isBusy || messageCount === 0}
                onClick={() => void runCompact()}
                sx={{
                  gap: '6px',
                  px: '10px',
                  height: 28,
                  borderRadius: '6px',
                  fontSize: '12px',
                  fontWeight: 500,
                  border: 1,
                  borderColor: 'divider',
                  '&:hover': { bgcolor: 'action.hover' },
                  '&:disabled': { opacity: 0.5 },
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
