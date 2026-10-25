import { useCallback, useEffect, useState } from 'react'
import { INSTANT_ENABLED } from '../../instant/enabled'
import { Alert, Box, InputBase, Stack, Switch, Typography, alpha, useTheme } from '../../../ui'
import { call } from '../../../ipc'
import { ModelPicker } from '../../ai/components/ModelPicker'
import type { SubagentModelOverride } from '../lib/subagentProfiles'

type Tier = 'easy' | 'medium' | 'hard'
interface Routing {
  enabled: boolean
  quick: SubagentModelOverride | null
  /** Match the model to how hard the task reads. */
  auto: boolean
  tiers: Partial<Record<Tier, SubagentModelOverride | null>>
}
interface Verdict {
  level: Tier
  by: 'laya' | 'jev' | 'rules'
  laya: boolean
  jev: boolean
}
const TIERS: { id: Tier; label: string; hint: string }[] = [
  { id: 'easy', label: 'Easy', hint: 'a lookup, a rename, a summary' },
  { id: 'medium', label: 'Medium', hint: 'a feature or fix in a few files' },
  { id: 'hard', label: 'Hard', hint: 'design, a subtle bug, many dependent steps' },
]
const BY: Record<Verdict['by'], string> = { laya: 'Laya (on this machine)', jev: 'Jev', rules: 'built-in rules' }
interface Savings {
  configured: boolean
  enabled?: boolean
  quick?: { name: string }
  smart?: { name: string }
  savings?: { input: number; output: number; requests: number; actualUsd: number | null; wouldUsd: number | null; savedUsd: number | null }
}

const compact = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${(n / 1e3).toFixed(1)}K` : String(n))
const money = (c: number) => `$${c < 0.1 ? c.toFixed(3) : c.toFixed(2)}`

/**
 * Cost-aware routing (beta): the work you never read (summaries, memory
 * upkeep, teammates' routines, read-only exploring) goes to a cheap model.
 * Your chat replies and planning stay on the model you picked.
 */
export function RoutingPanel() {
  const theme = useTheme()
  const [cfg, setCfg] = useState<Routing>({ enabled: false, quick: null, auto: false, tiers: {} })
  const [probe, setProbe] = useState('')
  const [verdict, setVerdict] = useState<Verdict | null>(null)
  const [savings, setSavings] = useState<Savings | null>(null)
  const [loaded, setLoaded] = useState(false)

  const loadSavings = useCallback(async () => setSavings(await call<Savings>('cmd_routing_savings', { days: 30 })), [])

  useEffect(() => {
    void call<Record<string, unknown>>('cmd_get_settings').then((s) => {
      const r = s?.routing as Partial<Routing> | undefined
      setCfg({ enabled: !!r?.enabled, quick: r?.quick ?? null, auto: !!r?.auto, tiers: r?.tiers ?? {} })
      setLoaded(true)
    })
    void loadSavings()
  }, [loadSavings])

  const save = async (next: Routing) => {
    setCfg(next)
    const st = (await call<Record<string, unknown>>('cmd_get_settings')) ?? {}
    await call('cmd_save_settings', { settings: { ...st, routing: next } })
    void loadSavings()
  }

  // Try the classifier on a sentence, after a short pause in typing.
  useEffect(() => {
    if (!probe.trim()) {
      setVerdict(null)
      return
    }
    const t = window.setTimeout(() => void call<Verdict>('cmd_routing_classify', { text: probe }).then(setVerdict), 350)
    return () => window.clearTimeout(t)
  }, [probe])

  const s = savings?.savings
  return (
    <Stack sx={{ gap: 2, maxWidth: 520 }}>
      <Stack sx={{ gap: 0.5 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
          <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
            Cost-aware routing
          </Typography>
          <Box component="span" sx={{ px: 0.75, height: 18, lineHeight: '18px', borderRadius: '5px', fontSize: '0.625rem', fontWeight: 700, letterSpacing: 0.4, bgcolor: alpha(theme.palette.warning.main, 0.16), color: 'warning.main' }}>
            BETA
          </Box>
        </Stack>
        <Typography variant="body2" color="text.secondary">
          Small models are good enough for most of an agent's background work. With this on, that work goes to a quick model you choose, and the
          model you picked for chat does the rest.
        </Typography>
      </Stack>

      <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
        <Stack sx={{ flex: 1 }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            Send background work to the quick model
          </Typography>
          <Typography variant="caption" color="text.secondary">
            Off by default. Anything it can't use falls back to your chat model.
          </Typography>
        </Stack>
        <Switch checked={cfg.enabled} disabled={!loaded} onChange={(e) => void save({ ...cfg, enabled: e.target.checked })} />
      </Stack>

      <Stack sx={{ gap: 0.75 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Quick model
        </Typography>
        <ModelPicker mode="agent" variant="field" value={cfg.quick} onChange={(m) => void save({ ...cfg, quick: m })} />
        <Typography variant="caption" color="text.secondary">
          Pick a small, cheap one, for example a Nemotron Nano. “Same as the chat” means nothing is routed.
        </Typography>
      </Stack>

      <Stack sx={{ gap: 1.25, p: 1.5, borderRadius: '10px', border: 1, borderColor: 'divider' }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
          <Stack sx={{ flex: 1 }}>
            <Typography variant="body2" sx={{ fontWeight: 600 }}>
              Match the model to the task
            </Typography>
            <Typography variant="caption" color="text.secondary">
              For subagent briefs and teammates' routines: read the task, call it easy, medium or hard, and use the model you set for that level.
            </Typography>
          </Stack>
          <Switch checked={cfg.auto} disabled={!loaded || !cfg.enabled} onChange={(e) => void save({ ...cfg, auto: e.target.checked })} />
        </Stack>
        {!cfg.enabled && (
          <Typography variant="caption" color="text.secondary">
            Turn routing on above to use this.
          </Typography>
        )}
        {TIERS.map((t) => (
          <Stack key={t.id} direction="row" sx={{ alignItems: 'center', gap: 1.5, opacity: cfg.auto ? 1 : 0.55 }}>
            <Stack sx={{ width: 120, flexShrink: 0 }}>
              <Typography variant="body2" sx={{ fontWeight: 600 }}>
                {t.label}
              </Typography>
              <Typography variant="caption" color="text.secondary">
                {t.hint}
              </Typography>
            </Stack>
            <Box sx={{ flex: 1, minWidth: 0 }}>
              <ModelPicker mode="agent" variant="field" value={cfg.tiers[t.id] ?? null} onChange={(m) => void save({ ...cfg, tiers: { ...cfg.tiers, [t.id]: m } })} />
            </Box>
          </Stack>
        ))}
        <Typography variant="caption" color="text.secondary">
          A level with no model uses the next one up, never a smaller one. Easy work with none set uses the quick model.
        </Typography>

        <Stack sx={{ gap: 0.5, pt: 0.5 }}>
          <Typography variant="caption" sx={{ fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.4, color: 'text.secondary' }}>
            Try it
          </Typography>
          <InputBase
            fullWidth
            placeholder="Type a task to see how it would be read"
            value={probe}
            onChange={(e) => setProbe(e.target.value)}
            sx={{ fontSize: '0.8125rem', px: 1.25, height: 34, border: 1, borderColor: 'divider', borderRadius: '9px' }}
          />
          {verdict && (
            <Typography variant="body2">
              Reads as <b>{verdict.level}</b>, decided by {BY[verdict.by]}.
              {verdict.by === 'rules' && !verdict.laya && !verdict.jev ? (INSTANT_ENABLED ? ' Laya (Settings, Viv Instant) or a Jev key makes this sharper.' : ' A Jev key makes this sharper.') : ''}
            </Typography>
          )}
        </Stack>
      </Stack>

      <Stack sx={{ gap: 0.5, p: 1.5, borderRadius: '10px', bgcolor: alpha(theme.palette.text.primary, 0.04) }}>
        <Typography variant="caption" sx={{ fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.4, color: 'text.secondary' }}>
          What it routes
        </Typography>
        <Typography variant="body2">Summaries of old conversation, hand-off notes, memory upkeep and re-ranking, who should reply in a team chat, teammates' routines and check-ins, and read-only exploring subagents.</Typography>
        <Typography variant="body2" color="text.secondary">
          Not routed: your replies, plan mode, and anything that writes files or runs commands.
        </Typography>
      </Stack>

      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Last 30 days
        </Typography>
        {!savings?.configured ? (
          <Typography variant="caption" color="text.secondary">
            Choose a quick model to start counting.
          </Typography>
        ) : !s || s.requests === 0 ? (
          <Typography variant="caption" color="text.secondary">
            Nothing has been routed to {savings.quick?.name || 'the quick model'} yet.
          </Typography>
        ) : (
          <Alert severity="success">
            Routed {s.requests} call{s.requests === 1 ? '' : 's'} ({compact(s.input)} in, {compact(s.output)} out) to {savings.quick?.name}.{' '}
            {s.savedUsd !== null && s.actualUsd !== null && s.wouldUsd !== null
              ? `That cost ${money(s.actualUsd)}; on ${savings.smart?.name} it would have been about ${money(s.wouldUsd)}, so roughly ${money(s.savedUsd)} saved.`
              : 'One of the two models has no known price, so there is no dollar figure to show.'}
          </Alert>
        )}
      </Stack>
    </Stack>
  )
}
