import { useEffect, useState } from 'react'
import { Box, Stack, Typography, InputBase, IconButton, CircularProgress, Popover, Tooltip, Select, MenuItem, Divider, Button } from '../../../ui'
import { TickCircleLinear, TrashLinear, KeyLinear, LayerLinear, AddLinear, RefreshLinear } from '../../../icons'
import type { ProviderMeta } from '../lib/providers'
import { useAuthStore, type RotationStrategy, type PoolEntryInfo } from '../store/authStore'

const STRATEGY_LABELS: Record<RotationStrategy, string> = {
  fill_first: 'Fill first (use one key until it fails)',
  round_robin: 'Round robin (cycle every request)',
  least_used: 'Least used (prefer the idlest key)',
  random: 'Random',
}

export function ProviderRow({ provider }: { provider: ProviderMeta }) {
  const connected = useAuthStore((s) => s.status[provider.id] ?? false)
  const saveKey = useAuthStore((s) => s.saveKey)
  const deleteKey = useAuthStore((s) => s.deleteKey)
  const [draft, setDraft] = useState('')
  const [busy, setBusy] = useState(false)
  const [poolAnchor, setPoolAnchor] = useState<HTMLElement | null>(null)

  const save = async () => {
    if (!draft.trim()) return
    setBusy(true)
    try {
      await saveKey(provider.id, draft.trim())
      setDraft('')
    } finally {
      setBusy(false)
    }
  }

  const remove = async () => {
    setBusy(true)
    try {
      await deleteKey(provider.id)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Stack
      direction="row"
      sx={{ alignItems: 'center', gap: 1.5, py: 1.25, px: 1.5, borderRadius: 2, '&:hover': { bgcolor: 'action.hover' } }}
    >
      <Box sx={{ width: 34, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'text.secondary' }}>
        <KeyLinear size={16} color="currentColor" />
      </Box>

      <Box sx={{ width: 160, flexShrink: 0 }}>
        <Typography variant="body2" sx={{ fontWeight: 500 }}>
          {provider.label}
        </Typography>
        <Typography variant="caption" color={connected ? 'success.main' : 'text.secondary'}>
          {connected ? 'Connected' : 'Not connected'}
        </Typography>
      </Box>

      {connected ? (
        <>
          <Box sx={{ flex: 1 }} />
          <Tooltip title="Manage key pool (rotate between multiple keys)">
            <IconButton size="small" onClick={(e) => setPoolAnchor(e.currentTarget)}>
              <LayerLinear size={15} color="currentColor" />
            </IconButton>
          </Tooltip>
          <IconButton size="small" onClick={() => void remove()} disabled={busy}>
            {busy ? <CircularProgress size={14} /> : <TrashLinear size={15} color="currentColor" />}
          </IconButton>
          <Popover
            open={!!poolAnchor}
            anchorEl={poolAnchor}
            onClose={() => setPoolAnchor(null)}
            anchorOrigin={{ vertical: 'bottom', horizontal: 'right' }}
            transformOrigin={{ vertical: 'top', horizontal: 'right' }}
            slotProps={{ paper: { sx: { width: 360, p: 1.5 } } }}
          >
            <KeyPoolPanel providerId={provider.id} providerLabel={provider.label} />
          </Popover>
        </>
      ) : (
        <>
          <InputBase
            fullWidth
            type="password"
            placeholder="Paste API key…"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && void save()}
            sx={{
              fontSize: '0.8125rem',
              fontFamily: '"Geist Mono", ui-monospace, monospace',
              bgcolor: 'rgba(255,255,255,0.05)',
              borderRadius: 999,
              px: 1.5,
              py: 0.5,
            }}
          />
          <IconButton size="small" onClick={() => void save()} disabled={busy || !draft.trim()}>
            {busy ? <CircularProgress size={14} /> : <TickCircleLinear size={16} color="currentColor" />}
          </IconButton>
        </>
      )}
    </Stack>
  )
}

function statusColor(status: PoolEntryInfo['status'], inCooldown: boolean): string {
  if (!inCooldown) return 'text.secondary'
  if (status === 'quota_or_billing') return 'error.main'
  return 'warning.main'
}

function statusLabel(entry: PoolEntryInfo): string {
  if (!entry.inCooldown) return entry.requestCount > 0 ? `${entry.requestCount} requests` : 'Ready'
  switch (entry.status) {
    case 'quota_or_billing':
      return 'Quota/billing — cooling down'
    case 'retryable':
      return 'Rate limited — cooling down'
    default:
      return 'Failed — cooling down'
  }
}

/** Multi-key rotation for one provider — ported from Hermes Agent's
 * credential-pool feature. The primary key (managed above, in `ProviderRow`
 * itself) always appears as the first entry here too, so this panel shows
 * the complete picture rather than a separate, easily-forgotten list. */
function KeyPoolPanel({ providerId, providerLabel }: { providerId: string; providerLabel: string }) {
  const entries = useAuthStore((s) => s.entries[providerId] ?? [])
  const strategy = useAuthStore((s) => s.strategies[providerId] ?? 'fill_first')
  const loading = useAuthStore((s) => s.poolLoading)
  const refresh = useAuthStore((s) => s.refreshPool)
  const addKey = useAuthStore((s) => s.addPoolKey)
  const removeKey = useAuthStore((s) => s.removePoolKey)
  const setStrategy = useAuthStore((s) => s.setPoolStrategy)
  const resetCooldowns = useAuthStore((s) => s.resetPoolCooldowns)

  const [label, setLabel] = useState('')
  const [key, setKey] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    void refresh(providerId)
  }, [providerId, refresh])

  const extraKeys = entries.filter((e) => e.source === 'pool')

  const add = async () => {
    if (!key.trim()) return
    setBusy(true)
    try {
      await addKey(providerId, label.trim(), key.trim())
      setLabel('')
      setKey('')
    } finally {
      setBusy(false)
    }
  }

  return (
    <Stack sx={{ gap: 1 }}>
      <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 600, display: 'block' }}>
        {providerLabel} key pool
      </Typography>
      <Typography variant="caption" color="text.secondary">
        Register more than one key and Vivianne rotates to the next when one hits a rate limit, quota, or auth error —
        tried before ever switching to a different provider.
      </Typography>

      <Stack sx={{ gap: 0.25, mt: 0.5 }}>
        {entries.map((entry) => (
          <Stack key={entry.id} direction="row" sx={{ alignItems: 'center', gap: 1, py: 0.5 }}>
            <Typography variant="caption" sx={{ flex: 1, minWidth: 0, fontFamily: '"Geist Mono", ui-monospace, monospace' }} noWrap>
              {entry.label} {entry.source === 'primary' && <Box component="span" sx={{ color: 'text.secondary' }}>(primary)</Box>}
            </Typography>
            <Typography variant="caption" sx={{ color: statusColor(entry.status, entry.inCooldown), flexShrink: 0 }}>
              {statusLabel(entry)}
            </Typography>
            {entry.source === 'pool' && (
              <IconButton size="small" onClick={() => void removeKey(providerId, entry.id)} sx={{ flexShrink: 0 }}>
                <TrashLinear size={12} color="currentColor" />
              </IconButton>
            )}
          </Stack>
        ))}
      </Stack>

      <Divider sx={{ my: 0.5 }} />

      <Stack sx={{ gap: 0.75 }}>
        <InputBase
          fullWidth
          placeholder="Label (e.g. backup key)"
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          sx={{ fontSize: '0.8125rem', bgcolor: 'action.hover', borderRadius: 1.5, px: 1.25, py: 0.5 }}
        />
        <Stack direction="row" sx={{ gap: 0.75 }}>
          <InputBase
            fullWidth
            type="password"
            placeholder="Paste another API key…"
            value={key}
            onChange={(e) => setKey(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && void add()}
            sx={{ fontSize: '0.8125rem', fontFamily: '"Geist Mono", ui-monospace, monospace', bgcolor: 'action.hover', borderRadius: 1.5, px: 1.25, py: 0.5 }}
          />
          <IconButton size="small" onClick={() => void add()} disabled={busy || !key.trim()}>
            {busy ? <CircularProgress size={14} /> : <AddLinear size={16} color="currentColor" />}
          </IconButton>
        </Stack>
      </Stack>

      <Divider sx={{ my: 0.5 }} />

      <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
        <Typography variant="caption" color="text.secondary" sx={{ flexShrink: 0 }}>
          Rotation
        </Typography>
        <Select
          size="small"
          value={strategy}
          onChange={(e) => void setStrategy(providerId, e.target.value as RotationStrategy)}
          disabled={extraKeys.length === 0}
          sx={{ fontSize: '0.75rem', flex: 1, '& .ui-Select-select': { py: 0.5 } }}
        >
          {(Object.keys(STRATEGY_LABELS) as RotationStrategy[]).map((s) => (
            <MenuItem key={s} value={s} sx={{ fontSize: '0.75rem' }}>
              {STRATEGY_LABELS[s]}
            </MenuItem>
          ))}
        </Select>
      </Stack>

      {entries.some((e) => e.inCooldown) && (
        <Button
          size="small"
          startIcon={<RefreshLinear size={13} color="currentColor" />}
          onClick={() => void resetCooldowns(providerId)}
          disabled={loading}
          sx={{ textTransform: 'none', alignSelf: 'flex-start', fontSize: '0.75rem' }}
        >
          Clear cooldowns
        </Button>
      )}
    </Stack>
  )
}
