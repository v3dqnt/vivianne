import { useMemo, useState } from 'react'
import { Box, Stack, Tooltip, Typography, ButtonBase, Snackbar, CircularProgress, alpha, useTheme } from '../../../ui'
import { ArchiveLinear } from '../../../icons'
import { useModelStore } from '../store/modelStore'
import { useAgentStore } from '../store/agentStore'
import { TOOLBAR_PILL } from '../lib/toolbarPill'

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
 * of the window drawn as an arc, in order from the top. */
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
  const stroke = size <= 16 ? 2 : 3.5
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

function useContextRows() {
  const theme = useTheme()
  const stats = useModelStore((s) => s.stats)
  // One hue in steps rather than five unrelated colours: the categories
  // read as parts of one window, and only fullness gets a warning colour.
  const colors = useMemo(() => {
    const ink = theme.palette.text.primary
    return { systemPrompt: alpha(ink, 0.9), toolSchemas: alpha(ink, 0.65), toolResults: alpha(ink, 0.45), assistant: alpha(ink, 0.3), user: alpha(ink, 0.18) }
  }, [theme])
  if (!stats) return null
  // `contextPercent` is NaN before a model is selected: show 0, not "NaN%".
  const pct = Number.isFinite(stats.contextPercent) ? Math.min(100, stats.contextPercent) : 0
  const accent = pct > 90 ? theme.palette.error.main : pct > 70 ? theme.palette.warning.main : theme.palette.text.primary
  const window = Math.max(1, stats.contextWindow)
  // Scaled against the whole window so the empty remainder stays visible as headroom.
  const rows = SEGMENTS.map((seg) => {
    const tokens = stats.breakdown?.[seg.key] ?? 0
    return { ...seg, tokens, share: (tokens / window) * 100 }
  }).filter((r) => r.tokens > 0)
  return { stats, colors, pct, accent, window, rows, accounted: rows.reduce((sum, r) => sum + r.tokens, 0) }
}

/** The composer's context indicator: the ring alone. The breakdown lives in the card beside the chat. */
export function ContextUsageBar() {
  const c = useContextRows()
  if (!c) return null
  const label = `Context ${Math.round(c.pct)}% full`
  return (
    <Tooltip title={label}>
      <Box className="cq-context" aria-label={label} sx={{ ...TOOLBAR_PILL, cursor: 'default', '&:hover': { bgcolor: 'transparent' } }}>
        <ContextRing rows={c.rows} colors={c.colors} window={c.window} accent={c.accent} size={14} />
      </Box>
    </Tooltip>
  )
}

/** The window's fullness as a percentage, for a section header. */
export function ContextPercent() {
  const c = useContextRows()
  if (!c) return null
  return <Typography sx={{ ...NUM, fontSize: '0.75rem', fontWeight: 500, color: c.pct > 70 ? c.accent : 'text.secondary' }}>{Math.round(c.pct)}%</Typography>
}

/** What fills the context window, with the one action it offers (compact). */
export function ContextBreakdown() {
  const theme = useTheme()
  const c = useContextRows()
  const sessionCost = useModelStore((s) => s.sessionCost)
  const compacting = useAgentStore((s) => s.compacting)
  const isBusy = useAgentStore((s) => s.isBusy)
  const messageCount = useAgentStore((s) => s.messages.length)
  const compactNow = useAgentStore((s) => s.compactNow)
  const [toast, setToast] = useState<string | null>(null)
  if (!c) return null
  const { stats, colors, pct, accent, rows, accounted } = c
  void accent
  const runCompact = async () => {
    const folded = await compactNow()
    setToast(folded > 0 ? `Compacted ${folded} messages into a summary.` : 'Nothing to compact yet.')
  }
  return (
    <>
      <Typography noWrap sx={{ ...NUM, fontSize: '0.75rem', color: 'text.secondary', px: '8px', pb: '8px' }}>
        {formatTokens(stats.contextTokens)} of {formatTokens(stats.contextWindow)}
      </Typography>
      <Box sx={{ px: '8px' }}>
        <Box sx={{ display: 'flex', gap: '2px', height: 6, borderRadius: '3px', overflow: 'hidden', bgcolor: alpha(theme.palette.text.primary, 0.06) }}>
          {rows.map((r) => (
            <Box key={r.key} sx={{ width: `${Math.max(r.share, 0.6)}%`, bgcolor: pct > 70 ? accent : colors[r.key], transition: 'width 240ms ease' }} />
          ))}
        </Box>
      </Box>
      <Stack sx={{ pt: '6px' }}>
        {rows.map((r) => (
          <Tooltip key={r.key} title={r.hint} placement="left">
            <Stack direction="row" sx={{ alignItems: 'center', gap: '8px', height: 28, px: '8px', borderRadius: '6px', '&:hover': { bgcolor: 'action.hover' } }}>
              <Box sx={{ width: 8, height: 8, borderRadius: '2px', bgcolor: colors[r.key], flexShrink: 0 }} />
              <Typography noWrap sx={{ fontSize: '0.8125rem', flex: 1, minWidth: 0, color: 'text.secondary' }}>
                {r.label}
              </Typography>
              <Typography sx={{ ...NUM, fontSize: '0.75rem', color: 'text.secondary' }}>{formatTokens(r.tokens)}</Typography>
            </Stack>
          </Tooltip>
        ))}
        {accounted > stats.contextTokens && (
          <Typography sx={{ px: '8px', pt: '4px', fontSize: '0.75rem', lineHeight: 1.5, color: 'text.disabled' }}>
            Prompt and tool schemas ({formatTokens(accounted - stats.contextTokens)}) are sent with every request and not counted in the percentage.
          </Typography>
        )}
        <Typography noWrap sx={{ ...NUM, fontSize: '0.75rem', color: 'text.disabled', px: '8px', pt: '6px' }}>
          {formatTokens(stats.inputTokens)} in, {formatTokens(stats.outputTokens)} out
          {stats.cacheRead > 0 && `, ${formatTokens(stats.cacheRead)} cached`}
          {sessionCost && `, ${formatCost(sessionCost.cost)}`}
        </Typography>
        <Tooltip title={isBusy ? 'Wait for the current turn to finish' : 'Fold older history into a summary to free room'}>
          <Box component="span" sx={{ display: 'flex' }}>
            <ButtonBase
              disabled={compacting || isBusy || messageCount === 0}
              onClick={() => void runCompact()}
              sx={{ display: 'flex', width: '100%', justifyContent: 'flex-start', alignItems: 'center', gap: '8px', height: 28, px: '8px', borderRadius: '6px', fontSize: '0.8125rem', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' }, '&:disabled': { opacity: 0.5 } }}
            >
              {compacting ? <CircularProgress size={12} /> : <ArchiveLinear size={13} color="currentColor" />}
              Compact history
            </ButtonBase>
          </Box>
        </Tooltip>
      </Stack>
      <Snackbar open={!!toast} autoHideDuration={3000} onClose={() => setToast(null)} message={toast} anchorOrigin={{ vertical: 'bottom', horizontal: 'center' }} />
    </>
  )
}
