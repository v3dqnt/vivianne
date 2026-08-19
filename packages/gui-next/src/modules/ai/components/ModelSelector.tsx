import { useEffect, useMemo, useState } from 'react'
import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import Menu from '@mui/material/Menu'
import MenuItem from '@mui/material/MenuItem'
import ListSubheader from '@mui/material/ListSubheader'
import InputBase from '@mui/material/InputBase'
import Typography from '@mui/material/Typography'
import Divider from '@mui/material/Divider'
import { TickCircleLinear, SearchNormalLinear, CpuLinear, ArrowRightLinear } from '../../../icons'
import { useModelStore, type SelectableModel } from '../store/modelStore'
import { useLlamaStore } from '../../settings/store/llamaStore'
import { useCustomProviderStore } from '../../settings/store/customProviderStore'
import { useSettingsUi } from '../../settings/store/uiStore'
import { llamaModelId, isRunning } from '../../settings/lib/llama'

// Deterministic color per provider name — avoids fetching real brand logos
// over the network (this is an offline-capable desktop app), while still
// giving each provider a distinct, recognizable mark in the list.
function providerColor(provider: string): string {
  let hash = 0
  for (let i = 0; i < provider.length; i++) hash = (hash * 31 + provider.charCodeAt(i)) >>> 0
  return `hsl(${hash % 360}, 55%, 55%)`
}

function ProviderMark({ provider, size = 16 }: { provider: string; size?: number }) {
  return (
    <Box
      sx={{
        width: size,
        height: size,
        borderRadius: '30%',
        flexShrink: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        bgcolor: providerColor(provider),
        color: 'rgba(0,0,0,0.75)',
        fontSize: size * 0.55,
        fontWeight: 700,
        lineHeight: 1,
      }}
    >
      {provider.charAt(0).toUpperCase()}
    </Box>
  )
}

export function ModelSelector() {
  const cloudModels = useModelStore((s) => s.models)
  const stats = useModelStore((s) => s.stats)
  const selectModel = useModelStore((s) => s.selectModel)
  const init = useModelStore((s) => s.init)
  const llamaEndpoints = useLlamaStore((s) => s.endpoints)
  const refreshLlama = useLlamaStore((s) => s.refresh)
  const customProviders = useCustomProviderStore((s) => s.providers)
  const refreshCustomProviders = useCustomProviderStore((s) => s.refresh)
  const openSettings = useSettingsUi((s) => s.openSettings)
  const [anchor, setAnchor] = useState<HTMLElement | null>(null)
  const [query, setQuery] = useState('')

  useEffect(() => {
    void init()
    void refreshLlama()
    void refreshCustomProviders()
  }, [init, refreshLlama, refreshCustomProviders])

  const isLocalSelected = stats?.provider === 'local'

  // Running llama.cpp endpoints, shaped like any other discovered model —
  // only running ones are offered here; a stopped one is a Settings concern
  // (start it there), not something worth listing as pickable-but-broken.
  const localModels = useMemo<SelectableModel[]>(
    () =>
      llamaEndpoints
        .filter((e) => isRunning(e.status))
        .map((e) => ({
          id: llamaModelId(e.id),
          name: e.name,
          provider: 'local',
          api: 'openai-chat',
          contextWindow: e.ctxSize,
          maxOutputTokens: null,
          inputCostPerMillion: 0,
          outputCostPerMillion: 0,
          isImageModel: false,
          isAudioModel: false,
          supportsVision: false,
        })),
    [llamaEndpoints],
  )

  useEffect(() => {
    if (!anchor) setQuery('')
  }, [anchor])

  const matchesQuery = (m: SelectableModel) => {
    if (!query.trim()) return true
    const q = query.trim().toLowerCase()
    return m.name.toLowerCase().includes(q) || m.provider.toLowerCase().includes(q)
  }
  const filteredCloud = useMemo(() => cloudModels.filter(matchesQuery), [cloudModels, query])
  const filteredLocal = useMemo(() => localModels.filter(matchesQuery), [localModels, query])

  const grouped = useMemo(() => {
    const map = new Map<string, SelectableModel[]>()
    for (const m of filteredCloud) map.set(m.provider, [...(map.get(m.provider) ?? []), m])
    return map
  }, [filteredCloud])

  // Manually entered, not live-discovered (see `customProviders.ts`) — each
  // provider's own model list is already grouped, so no re-keying needed.
  const customEntries = useMemo(
    () =>
      customProviders.map((cp) => ({
        label: cp.name,
        models: cp.models
          .map(
            (m): SelectableModel => ({
              id: m.id,
              name: m.id,
              provider: cp.id,
              api: cp.apiFormat,
              baseUrl: cp.baseUrl,
              contextWindow: m.contextWindow,
              maxOutputTokens: m.maxOutputTokens,
              inputCostPerMillion: null,
              outputCostPerMillion: null,
              isImageModel: false,
              isAudioModel: false,
              supportsVision: false,
            }),
          )
          .filter(matchesQuery),
      })),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [customProviders, query],
  )

  const goToLocalSettings = () => {
    setAnchor(null)
    openSettings('local')
  }

  const modelRow = (m: SelectableModel, icon: React.ReactNode) => {
    const selected = stats?.modelId === m.id && stats?.provider === m.provider
    return (
      <MenuItem
        key={`${m.provider}-${m.id}`}
        selected={selected}
        onClick={() => {
          void selectModel(m)
          setAnchor(null)
        }}
        sx={{ gap: 1, fontSize: '0.8125rem' }}
      >
        {icon}
        <Box sx={{ flex: 1, minWidth: 0 }}>
          <Typography variant="body2" noWrap>
            {m.name}
          </Typography>
        </Box>
        {m.contextWindow && (
          <Typography variant="caption" color="text.secondary" sx={{ fontFamily: 'monospace' }}>
            {Math.round(m.contextWindow / 1000)}K
          </Typography>
        )}
        {selected && <TickCircleLinear size={14} color="currentColor" />}
      </MenuItem>
    )
  }

  const label = stats?.modelName || 'Select model'
  const totalCustomModels = customEntries.reduce((n, e) => n + e.models.length, 0)
  const totalCustomModelsUnfiltered = customProviders.reduce((n, p) => n + p.models.length, 0)
  const noResults =
    filteredCloud.length === 0 &&
    filteredLocal.length === 0 &&
    totalCustomModels === 0 &&
    (cloudModels.length > 0 || localModels.length > 0 || totalCustomModelsUnfiltered > 0)

  return (
    <>
      <Button
        size="small"
        onClick={(e) => setAnchor(e.currentTarget)}
        startIcon={
          isLocalSelected ? (
            <CpuLinear size={13} color="currentColor" />
          ) : (
            stats?.provider && <ProviderMark provider={stats.provider} size={13} />
          )
        }
        sx={{
          textTransform: 'none',
          fontSize: '0.8125rem',
          color: 'text.primary',
          maxWidth: 200,
          minWidth: 0,
          borderRadius: 999,
          px: 1,
          py: 0.5,
          '&:hover': { bgcolor: 'action.hover' },
        }}
      >
        <Typography variant="body2" noWrap sx={{ fontWeight: 600 }}>
          {label}
        </Typography>
      </Button>
      <Menu
        anchorEl={anchor}
        open={!!anchor}
        onClose={() => setAnchor(null)}
        anchorOrigin={{ vertical: 'top', horizontal: 'left' }}
        transformOrigin={{ vertical: 'bottom', horizontal: 'left' }}
        slotProps={{ paper: { sx: { maxHeight: 420, width: 320 } } }}
      >
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, px: 1.5, py: 1, position: 'sticky', top: 0, bgcolor: 'background.paper', zIndex: 1 }}>
          <SearchNormalLinear size={14} color="currentColor" />
          <InputBase
            autoFocus
            fullWidth
            placeholder="Search models…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.stopPropagation()}
            sx={{ fontSize: '0.8125rem' }}
          />
        </Box>

        {cloudModels.length === 0 && localModels.length === 0 && totalCustomModelsUnfiltered === 0 && (
          <MenuItem disabled>
            <Typography variant="caption">No authenticated models — add a provider in Settings.</Typography>
          </MenuItem>
        )}
        {noResults && (
          <MenuItem disabled>
            <Typography variant="caption">No matches.</Typography>
          </MenuItem>
        )}

        {Array.from(grouped.entries()).map(([provider, providerModels]) => [
          <ListSubheader key={`h-${provider}`} sx={{ fontSize: '0.6875rem', textTransform: 'uppercase', letterSpacing: 0.4, lineHeight: '28px' }}>
            {provider}
          </ListSubheader>,
          ...providerModels.map((m) => modelRow(m, <ProviderMark provider={m.provider} />)),
        ])}

        {customEntries
          .filter((e) => e.models.length > 0)
          .map((e) => [
            <ListSubheader key={`h-${e.label}`} sx={{ fontSize: '0.6875rem', textTransform: 'uppercase', letterSpacing: 0.4, lineHeight: '28px' }}>
              {e.label}
            </ListSubheader>,
            ...e.models.map((m) => modelRow(m, <ProviderMark provider={e.label} />)),
          ])}

        {/* Always its own section (icon, not a provider-name header) — these
            run on this machine, not through a cloud API key, and should
            read as a different kind of thing from the groups above rather
            than blending in as just another provider. */}
        {(filteredLocal.length > 0 || !query.trim()) && (
          <>
            <Divider sx={{ my: 0.5 }} />
            <ListSubheader
              sx={{ display: 'flex', alignItems: 'center', gap: 0.75, fontSize: '0.6875rem', textTransform: 'uppercase', letterSpacing: 0.4, lineHeight: '28px' }}
            >
              <CpuLinear size={12} color="currentColor" />
              On this device
            </ListSubheader>
            {filteredLocal.length > 0 ? (
              filteredLocal.map((m) => modelRow(m, <CpuLinear size={16} color="currentColor" />))
            ) : (
              <MenuItem onClick={goToLocalSettings} sx={{ gap: 1, fontSize: '0.8125rem', color: 'text.secondary' }}>
                <CpuLinear size={16} color="currentColor" />
                <Typography variant="body2" sx={{ flex: 1 }}>
                  Add or start a local model…
                </Typography>
                <ArrowRightLinear size={14} color="currentColor" />
              </MenuItem>
            )}
          </>
        )}
      </Menu>
    </>
  )
}
