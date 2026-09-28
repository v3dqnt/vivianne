import { useEffect, useMemo, useState } from 'react'
import { Box, Stack, Typography, ButtonBase, alpha, useTheme } from '../../../ui'
import { call, on } from '../../../ipc'

/** Mirrors `multiplayer::collab` types (`rename_all = "camelCase"`). */
interface FileTouch {
  path: string
  op: 'created' | 'edited' | 'deleted'
}
interface CommandRun {
  command: string
  ok: boolean
}
export interface ActivityEntry {
  id: string
  at: number
  author: string
  request: string
  outcome: string
  files: FileTouch[]
  commands: CommandRun[]
  other: string[]
  commit: string | null
}
interface DayUsage {
  input: number
  output: number
  cacheRead: number
  cacheWrite: number
  cost: number
  requests: number
  hasUnknownCost: boolean
}
interface SyncNote {
  at: number
  text: string
}
interface CollabData {
  mine: ActivityEntry[]
  partner: ActivityEntry[]
  myUsage: Record<string, DayUsage>
  partnerUsage: Record<string, DayUsage>
  notes: SyncNote[]
  lastSync: string | null
  pendingMerge: boolean
}

function today(): string {
  const d = new Date()
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

interface Totals {
  tokens: number
  cost: number
  requests: number
  unknown: boolean
}

function sum(days: Record<string, DayUsage>, only?: string): Totals {
  const t: Totals = { tokens: 0, cost: 0, requests: 0, unknown: false }
  for (const [day, u] of Object.entries(days)) {
    if (only && day !== only) continue
    t.tokens += u.input + u.output
    t.cost += u.cost
    t.requests += u.requests
    t.unknown ||= u.hasUnknownCost && u.requests > 0
  }
  return t
}

function tokens(n: number): string {
  return n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)}M` : n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n)
}
function money(t: Totals): string {
  return `$${t.cost.toFixed(t.cost < 1 ? 3 : 2)}${t.unknown ? '+' : ''}`
}
function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
}

const SECTION = { fontWeight: 600, letterSpacing: 0.4, textTransform: 'uppercase', fontSize: '0.625rem', color: 'text.secondary' } as const

/** Usage, activity and sync for one multiplayer project — `multiplayer::collab`. */
export function CollabView({ projectId, partnerName }: { projectId: string; partnerName: string }) {
  const [data, setData] = useState<CollabData | null>(null)

  useEffect(() => {
    let alive = true
    const load = () =>
      void call<CollabData>('cmd_multiplayer_collab', { projectId }).then((d) => {
        if (alive && d) setData(d)
      })
    load()
    let off: (() => void) | undefined
    void on<string>('multiplayer_collab', (id) => {
      if (id === projectId) load()
    }).then((u) => {
      off = u ?? undefined
    })
    return () => {
      alive = false
      off?.()
    }
  }, [projectId])

  if (!data) return null
  return (
    <Stack sx={{ gap: 2 }}>
      <UsageBlock data={data} partnerName={partnerName} />
      <SyncBlock data={data} projectId={projectId} onChange={() => void call<CollabData>('cmd_multiplayer_collab', { projectId }).then((d) => d && setData(d))} />
      <ActivityBlock data={data} partnerName={partnerName} />
    </Stack>
  )
}

function UsageBlock({ data, partnerName }: { data: CollabData; partnerName: string }) {
  const theme = useTheme()
  const [range, setRange] = useState<'today' | 'all'>('today')
  const only = range === 'today' ? today() : undefined
  const me = sum(data.myUsage, only)
  const them = sum(data.partnerUsage, only)
  const total: Totals = { tokens: me.tokens + them.tokens, cost: me.cost + them.cost, requests: me.requests + them.requests, unknown: me.unknown || them.unknown }
  const share = total.tokens ? me.tokens / total.tokens : 0.5
  const rows: Array<[string, Totals, string]> = [
    ['You', me, theme.palette.primary.main],
    [partnerName, them, theme.palette.success.main],
  ]
  return (
    <Stack sx={{ gap: 1 }}>
      <Stack direction="row" sx={{ alignItems: 'center' }}>
        <Typography sx={{ ...SECTION, flex: 1 }}>Shared usage</Typography>
        {(['today', 'all'] as const).map((r) => (
          <ButtonBase
            key={r}
            onClick={() => setRange(r)}
            sx={{ fontSize: '0.6875rem', px: 0.75, py: 0.125, borderRadius: '6px', color: range === r ? 'text.primary' : 'text.secondary', bgcolor: range === r ? 'action.selected' : 'transparent' }}
          >
            {r === 'today' ? 'Today' : 'All time'}
          </ButtonBase>
        ))}
      </Stack>
      <Box sx={{ display: 'flex', height: 6, borderRadius: 3, overflow: 'hidden', bgcolor: 'action.hover' }}>
        {total.tokens > 0 && (
          <>
            <Box sx={{ width: `${share * 100}%`, bgcolor: 'primary.main' }} />
            <Box sx={{ width: 2, bgcolor: 'background.paper' }} />
            <Box sx={{ flex: 1, bgcolor: 'success.main' }} />
          </>
        )}
      </Box>
      <Box component="table" sx={{ width: '100%', borderCollapse: 'collapse', fontSize: '0.75rem', fontVariantNumeric: 'tabular-nums', '& td': { py: 0.375 } }}>
        <tbody>
          {rows.map(([name, t, color]) => (
            <tr key={name}>
              <td>
                <Box component="span" sx={{ display: 'inline-block', width: 8, height: 8, borderRadius: '50%', bgcolor: color, mr: 0.75 }} />
                {name}
              </td>
              <Box component="td" sx={{ textAlign: 'right', color: 'text.secondary' }}>
                {tokens(t.tokens)} tokens
              </Box>
              <Box component="td" sx={{ textAlign: 'right', color: 'text.secondary' }}>
                {t.requests} req
              </Box>
              <Box component="td" sx={{ textAlign: 'right' }}>
                {money(t)}
              </Box>
            </tr>
          ))}
          <Box component="tr" sx={{ borderTop: 1, borderColor: 'divider', fontWeight: 600 }}>
            <td>Together</td>
            <Box component="td" sx={{ textAlign: 'right' }}>
              {tokens(total.tokens)} tokens
            </Box>
            <Box component="td" sx={{ textAlign: 'right' }}>
              {total.requests} req
            </Box>
            <Box component="td" sx={{ textAlign: 'right' }}>
              {money(total)}
            </Box>
          </Box>
        </tbody>
      </Box>
      {total.unknown && (
        <Typography variant="caption" color="text.disabled" sx={{ fontSize: '0.6875rem' }}>
          + some requests used a model with no known price.
        </Typography>
      )}
    </Stack>
  )
}

function SyncBlock({ data, projectId, onChange }: { data: CollabData; projectId: string; onChange: () => void }) {
  const theme = useTheme()
  return (
    <Stack sx={{ gap: 0.75 }}>
      <Typography sx={SECTION}>Code sync</Typography>
      <Typography variant="caption" color="text.secondary">
        {data.pendingMerge
          ? 'Your partner’s latest commits are here and will be merged before your agent’s next run.'
          : (data.lastSync ?? 'Each run is committed and shared automatically.')}
      </Typography>
      {data.notes.map((n) => (
        <Box key={n.at} sx={{ px: 1, py: 0.75, borderRadius: '8px', bgcolor: alpha(theme.palette.warning.main, 0.1), color: 'warning.main' }}>
          <Typography variant="caption" sx={{ display: 'block', lineHeight: 1.45 }}>
            {n.text}
          </Typography>
        </Box>
      ))}
      {data.notes.length > 0 && (
        <ButtonBase
          onClick={() => void call('cmd_multiplayer_dismiss_notes', { projectId }).then(onChange)}
          sx={{ alignSelf: 'flex-start', fontSize: '0.6875rem', color: 'text.secondary', px: 0.5, borderRadius: '6px' }}
        >
          Dismiss
        </ButtonBase>
      )}
    </Stack>
  )
}

function ActivityBlock({ data, partnerName }: { data: CollabData; partnerName: string }) {
  const merged = useMemo(
    () =>
      [...data.partner.map((e) => ({ e, mine: false })), ...data.mine.map((e) => ({ e, mine: true }))]
        .sort((a, b) => b.e.at - a.e.at)
        .slice(0, 40),
    [data],
  )
  return (
    <Stack sx={{ gap: 0.75 }}>
      <Typography sx={SECTION}>Activity</Typography>
      {merged.length === 0 ? (
        <Typography variant="caption" color="text.secondary">
          Nothing yet. Each run either agent finishes shows up here, and the other agent is told about it before its
          next run.
        </Typography>
      ) : (
        <Stack sx={{ gap: 0.75, maxHeight: 320, overflowY: 'auto' }}>
          {merged.map(({ e, mine }) => (
            <ActivityRow key={e.id} entry={e} who={mine ? 'You' : partnerName} mine={mine} />
          ))}
        </Stack>
      )}
    </Stack>
  )
}

function ActivityRow({ entry, who, mine }: { entry: ActivityEntry; who: string; mine: boolean }) {
  const theme = useTheme()
  const color = mine ? theme.palette.primary.main : theme.palette.success.main
  return (
    <Stack sx={{ gap: 0.25, pl: 1.25, borderLeft: `2px solid ${alpha(color, 0.6)}` }}>
      <Stack direction="row" sx={{ gap: 0.75, alignItems: 'baseline' }}>
        <Typography variant="caption" sx={{ fontWeight: 600, color }}>
          {who}
        </Typography>
        <Typography variant="caption" color="text.disabled" sx={{ fontSize: '0.6875rem' }}>
          {clock(entry.at)}
          {entry.commit ? ` · ${entry.commit}` : ''}
        </Typography>
      </Stack>
      <Typography variant="caption" sx={{ lineHeight: 1.4 }}>
        {entry.request}
      </Typography>
      {entry.files.length > 0 && (
        <Typography variant="caption" color="text.secondary" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.6875rem', wordBreak: 'break-all' }}>
          {entry.files
            .slice(0, 5)
            .map((f) => `${f.op === 'created' ? '+' : f.op === 'deleted' ? '−' : '~'} ${f.path}`)
            .join('  ')}
          {entry.files.length > 5 ? `  +${entry.files.length - 5} more` : ''}
        </Typography>
      )}
      {entry.commands.length > 0 && (
        <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem' }}>
          {entry.commands.map((c) => `${c.ok ? '✓' : '✗'} ${c.command}`).join('   ')}
        </Typography>
      )}
      {entry.outcome && (
        <Typography variant="caption" color="text.secondary" sx={{ fontStyle: 'italic' }}>
          {entry.outcome}
        </Typography>
      )}
    </Stack>
  )
}
