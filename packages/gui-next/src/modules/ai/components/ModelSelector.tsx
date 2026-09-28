import { useEffect, useMemo, useState } from 'react'
import { Box, Button, Menu, MenuItem, ListSubheader, InputBase, Typography, Divider, Stack, Tooltip, alpha, useTheme } from '../../../ui'
import { TickCircleLinear, SearchNormalLinear, CpuLinear, ArrowRightLinear, CloseCircleLinear } from '../../../icons'
import { useModelStore, type SelectableModel } from '../store/modelStore'
import { useLlamaStore } from '../../settings/store/llamaStore'
import { useCustomProviderStore } from '../../settings/store/customProviderStore'
import { useSettingsUi } from '../../settings/store/uiStore'
import { llamaModelId, isRunning } from '../../settings/lib/llama'

/** Height of the pinned search + filter block, so provider subheaders can
 * stick directly beneath it instead of sliding underneath it. */
const STICKY_HEADER_H = 84

/** Hues chosen to stay distinguishable from one another and legible against
 * both the light (#fff) and dark (#141414) menu surfaces. Indexed by a hash
 * of the name rather than assigned in list order, so a provider keeps the
 * same colour no matter which other providers happen to be authenticated. */
const PROVIDER_HUES = [210, 25, 152, 280, 340, 45, 190, 100, 320, 0]

function providerHue(name: string): number {
  let hash = 0
  for (let i = 0; i < name.length; i++) hash = (hash * 31 + name.charCodeAt(i)) >>> 0
  return PROVIDER_HUES[hash % PROVIDER_HUES.length]
}

/** The square initial mark shown against a provider group. */
function ProviderMark({ provider, size = 16 }: { provider: string; size?: number }) {
  const h = providerHue(provider)
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
        bgcolor: `hsl(${h}, 58%, 52%)`,
        color: 'rgba(0,0,0,0.78)',
        fontSize: size * 0.55,
        fontWeight: 700,
        lineHeight: 1,
      }}
    >
      {provider.charAt(0).toUpperCase()}
    </Box>
  )
}

/** Aggregator model names arrive as "vendor/model" or "vendor/model:variant".
 * The vendor half used to be rendered as part of the name, so every row in a
 * long OpenRouter list opened with repeated low-information text and the part
 * that actually identifies the model was what got truncated away. Split it
 * out so the vendor carries colour instead of characters. */
export function splitVendor(name: string): { vendor: string | null; rest: string } {
  const i = name.indexOf('/')
  if (i <= 0 || i === name.length - 1) return { vendor: null, rest: name }
  return { vendor: name.slice(0, i), rest: name.slice(i + 1) }
}

function VendorChip({ vendor }: { vendor: string }) {
  const theme = useTheme()
  const h = providerHue(vendor)
  // The hue is identical in both themes and only the lightness of the ink
  // changes, so a vendor reads as the same colour identity either way.
  const ink = theme.palette.mode === 'dark' ? `hsl(${h}, 62%, 70%)` : `hsl(${h}, 68%, 33%)`
  return (
    <Box
      component="span"
      sx={{
        flexShrink: 0,
        px: 0.625,
        py: 0.125,
        borderRadius: '6px',
        fontSize: '0.6875rem',
        fontWeight: 600,
        lineHeight: 1.5,
        color: ink,
        bgcolor: alpha(`hsl(${h}, 60%, 50%)`, theme.palette.mode === 'dark' ? 0.18 : 0.12),
      }}
    >
      {vendor}
    </Box>
  )
}

/** A capability tag. Kept as text: the icon set has no eye/vision glyph, and
 * a borrowed wrong-metaphor icon reads worse than the word does. */
function Tag({ label, title }: { label: string; title: string }) {
  return (
    <Tooltip title={title}>
      <Box
        component="span"
        sx={{
          flexShrink: 0,
          px: 0.5,
          py: 0.125,
          borderRadius: '5px',
          fontSize: '0.625rem',
          fontWeight: 600,
          letterSpacing: 0.2,
          lineHeight: 1.6,
          color: 'text.secondary',
          border: 1,
          borderColor: 'divider',
        }}
      >
        {label}
      </Box>
    </Tooltip>
  )
}

/** The filters offered above the list. Every one is derived from a field the
 * backend actually reports on `DiscoveredModel` — nothing here is a filter
 * the model list cannot answer. */
const FILTERS = [
  { id: 'vision', label: 'Vision', title: 'Accepts image input', test: (m: SelectableModel) => m.supportsVision },
  {
    id: 'free',
    label: 'Free',
    title: 'No input or output cost',
    test: (m: SelectableModel) => m.inputCostPerMillion === 0 && m.outputCostPerMillion === 0,
  },
  {
    id: 'long',
    label: '200K+',
    title: 'Context window of at least 200K tokens',
    test: (m: SelectableModel) => (m.contextWindow ?? 0) >= 200_000,
  },
] as const

type FilterId = (typeof FILTERS)[number]['id']

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
  const [active, setActive] = useState<FilterId[]>([])

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

  // Reset the query AND the filters on close, so reopening never presents a
  // silently pre-filtered list that looks like the full one.
  useEffect(() => {
    if (!anchor) {
      setQuery('')
      setActive([])
    }
  }, [anchor])

  const matches = useMemo(() => {
    const q = query.trim().toLowerCase()
    const tests = FILTERS.filter((f) => active.includes(f.id))
    return (m: SelectableModel) => {
      if (q && !m.name.toLowerCase().includes(q) && !m.provider.toLowerCase().includes(q)) return false
      // AND, not OR: each additional chip narrows the list further, which is
      // what a row of enabled filters visually implies.
      return tests.every((f) => f.test(m))
    }
  }, [query, active])

  const filteredCloud = useMemo(() => cloudModels.filter(matches), [cloudModels, matches])
  const filteredLocal = useMemo(() => localModels.filter(matches), [localModels, matches])

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
          .filter(matches),
      })),
    [customProviders, matches],
  )

  const goToLocalSettings = () => {
    setAnchor(null)
    openSettings('local')
  }

  const toggle = (id: FilterId) => setActive((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]))

  const modelRow = (m: SelectableModel, icon: React.ReactNode) => {
    const selected = stats?.modelId === m.id && stats?.provider === m.provider
    const { vendor, rest } = splitVendor(m.name)
    return (
      <MenuItem
        key={`${m.provider}-${m.id}`}
        selected={selected}
        onClick={() => {
          void selectModel(m)
          setAnchor(null)
        }}
        sx={{ gap: 0.75, fontSize: '0.8125rem', py: 0.625 }}
      >
        {icon}
        {vendor && <VendorChip vendor={vendor} />}
        <Typography variant="body2" noWrap sx={{ flex: 1, minWidth: 0 }}>
          {rest}
        </Typography>
        {m.supportsVision && <Tag label="Vision" title="Accepts image input" />}
        {m.contextWindow && (
          <Typography variant="caption" color="text.secondary" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', flexShrink: 0 }}>
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
  const nothingAuthenticated = cloudModels.length === 0 && localModels.length === 0 && totalCustomModelsUnfiltered === 0
  const noResults = !nothingAuthenticated && filteredCloud.length === 0 && filteredLocal.length === 0 && totalCustomModels === 0

  const subheaderSx = {
    fontSize: '0.6875rem',
    textTransform: 'uppercase',
    letterSpacing: 0.4,
    lineHeight: '28px',
    // Provider headers stay pinned too, but below the search block rather
    // than scrolling up underneath it.
    top: STICKY_HEADER_H,
    zIndex: 1,
  } as const

  return (
    <>
      <Button
        size="small"
        className="cq-collapsible"
        onClick={(e) => setAnchor(e.currentTarget)}
        title={`Model: ${label}`}
        aria-label={`Model: ${label}`}
        startIcon={
          isLocalSelected ? (
            <CpuLinear size={13} color="currentColor" />
          ) : (
            // With no model chosen there's no provider logo, and a collapsed
            // composer shows only the icon — so fall back to a generic one
            // rather than an empty pill nobody can identify.
            stats?.provider ? <ProviderMark provider={stats.provider} size={13} /> : <CpuLinear size={13} color="currentColor" />
          )
        }
        sx={{
          textTransform: 'none',
          fontSize: '0.8125rem',
          color: 'text.primary',
          maxWidth: 200,
          minWidth: 0,
          borderRadius: '8px',
          px: 1,
          py: 0.375,
          '&:hover': { bgcolor: 'action.hover' },
        }}
      >
        <Typography className="cq-label" variant="body2" noWrap sx={{ fontWeight: 500, fontSize: '0.8125rem' }}>
          {label}
        </Typography>
      </Button>
      <Menu
        anchorEl={anchor}
        open={!!anchor}
        onClose={() => setAnchor(null)}
        anchorOrigin={{ vertical: 'top', horizontal: 'left' }}
        transformOrigin={{ vertical: 'bottom', horizontal: 'left' }}
        slotProps={{
          // The scroll container has to be the LIST, not the paper. The search
          // block is `position: sticky`, and sticky only resolves against the
          // ancestor that actually scrolls — with the height cap on the paper
          // instead, the "sticky" bar scrolled away with everything else.
          paper: { sx: { width: 380, maxWidth: '92vw', overflow: 'hidden' } },
          list: { sx: { maxHeight: 460, overflowY: 'auto', pt: 0 } },
        }}
      >
        <Box
          sx={{
            position: 'sticky',
            top: 0,
            zIndex: 2,
            bgcolor: 'background.paper',
            borderBottom: 1,
            borderColor: 'divider',
          }}
        >
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 1.5, py: 1 }}>
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
            {query && (
              <Box
                component="button"
                aria-label="Clear search"
                onClick={() => setQuery('')}
                sx={{ display: 'flex', border: 'none', bgcolor: 'transparent', p: 0, cursor: 'pointer', color: 'text.secondary' }}
              >
                <CloseCircleLinear size={14} color="currentColor" />
              </Box>
            )}
          </Stack>
          <Stack direction="row" sx={{ gap: 0.5, px: 1.5, pb: 1 }}>
            {FILTERS.map((f) => {
              const on = active.includes(f.id)
              return (
                <Tooltip key={f.id} title={f.title}>
                  <Box
                    component="button"
                    onClick={() => toggle(f.id)}
                    sx={{
                      px: 1,
                      py: 0.25,
                      borderRadius: 999,
                      fontSize: '0.6875rem',
                      fontWeight: 600,
                      cursor: 'pointer',
                      border: 1,
                      borderColor: on ? 'primary.main' : 'divider',
                      color: on ? 'primary.main' : 'text.secondary',
                      bgcolor: on ? 'action.selected' : 'transparent',
                      transition: 'background-color 0.15s, border-color 0.15s, color 0.15s',
                      '&:hover': { bgcolor: 'action.hover' },
                    }}
                  >
                    {f.label}
                  </Box>
                </Tooltip>
              )
            })}
          </Stack>
        </Box>

        {nothingAuthenticated && (
          <MenuItem disabled>
            <Typography variant="caption">No authenticated models — add a provider in Settings.</Typography>
          </MenuItem>
        )}
        {noResults && (
          <MenuItem disabled>
            <Typography variant="caption">{active.length > 0 ? 'No models match those filters.' : 'No matches.'}</Typography>
          </MenuItem>
        )}

        {Array.from(grouped.entries()).map(([provider, providerModels]) => [
          <ListSubheader key={`h-${provider}`} sx={subheaderSx}>
            {provider}
          </ListSubheader>,
          ...providerModels.map((m) => modelRow(m, <ProviderMark provider={m.provider} />)),
        ])}

        {customEntries
          .filter((e) => e.models.length > 0)
          .map((e) => [
            <ListSubheader key={`h-${e.label}`} sx={subheaderSx}>
              {e.label}
            </ListSubheader>,
            ...e.models.map((m) => modelRow(m, <ProviderMark provider={e.label} />)),
          ])}

        {/* Always its own section (icon, not a provider-name header) — these
            run on this machine, not through a cloud API key, and should read
            as a different kind of thing from the groups above rather than
            blending in as just another provider. */}
        {(filteredLocal.length > 0 || (!query.trim() && active.length === 0)) && (
          <>
            <Divider sx={{ my: 0.5 }} />
            <ListSubheader sx={{ ...subheaderSx, display: 'flex', alignItems: 'center', gap: 0.75 }}>
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
