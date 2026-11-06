import { useEffect, useMemo, useRef, useState } from 'react'
import { Box, ButtonBase, InputBase, Popover, Stack, Tooltip, Typography, alpha, useTheme } from '../../../ui'
import { SearchNormalLinear, CpuLinear, ArrowRightLinear, CloseCircleLinear, TickCircleLinear, StarLinear, StarBold, ArrowDownLinear } from '../../../icons'
import { loadPersisted, savePersisted } from '../../../lib/persist'
import { statusOf, useHarnessStore, type Harness } from '../store/harnessStore'
import { useImageModels } from '../store/imageModelsStore'
import { useModelStore, type SelectableModel } from '../store/modelStore'
import { useLlamaStore } from '../../settings/store/llamaStore'
import { useCustomProviderStore } from '../../settings/store/customProviderStore'
import { useSettingsUi } from '../../settings/store/uiStore'
import { llamaModelId, isRunning } from '../../settings/lib/llama'
import type { SubagentModelOverride } from '../../settings/lib/subagentProfiles'
import { TOOLBAR_PILL } from '../lib/toolbarPill'

const HARNESS = 'harness'

/** The agent as a model: the backend routes turns for provider `harness` to it. */
const harnessModel = (h: Harness): SelectableModel => ({
  id: h.id,
  name: h.name,
  provider: HARNESS,
  api: 'acp',
  contextWindow: null,
  maxOutputTokens: null,
  inputCostPerMillion: null,
  outputCostPerMillion: null,
  isImageModel: false,
  isAudioModel: false,
  supportsVision: false,
})

const MONO = '"Geist Mono", ui-monospace, monospace'

/** Names a reader recognises for the providers whose ids aren't words. */
const PROVIDER_LABEL: Record<string, string> = {
  router: 'Routing',
  chatgpt: 'ChatGPT plan',
  openai: 'OpenAI',
  anthropic: 'Anthropic',
  google: 'Google',
  xai: 'xAI',
  openrouter: 'OpenRouter',
  nebius: 'Nebius',
  'nvidia-nim': 'NVIDIA NIM',
  local: 'On this computer',
  sd: 'Images · on this computer',
  harness: 'Coding agent',
}
const providerLabel = (p: string) => PROVIDER_LABEL[p] ?? p.charAt(0).toUpperCase() + p.slice(1)

function ProviderMark({ provider, size = 16 }: { provider: string; size?: number }) {
  if (provider === 'local' || provider === 'sd') {
    return (
      <Box sx={{ display: 'flex', color: 'text.secondary', flexShrink: 0 }}>
        <CpuLinear size={size} color="currentColor" />
      </Box>
    )
  }
  return (
    <Box
      sx={{
        width: size,
        height: size,
        borderRadius: '30%',
        flexShrink: 0,
        display: 'grid',
        placeItems: 'center',
        // One accent in the interface: a provider is a neutral monogram, not a colour of its own.
        bgcolor: 'action.selected',
        color: 'text.secondary',
        fontSize: size * 0.55,
        fontWeight: 600,
        lineHeight: 1,
      }}
    >
      {provider.charAt(0).toUpperCase()}
    </Box>
  )
}

/** "vendor/model" names from aggregators: the vendor becomes a chip so the
 * part that identifies the model isn't what gets truncated. */
function splitVendor(name: string): { vendor: string | null; rest: string } {
  const i = name.indexOf('/')
  if (i <= 0 || i === name.length - 1) return { vendor: null, rest: name }
  return { vendor: name.slice(0, i), rest: name.slice(i + 1) }
}

/** A model the pickers can offer: any signed-in provider's, a custom
 * provider's, or a local llama.cpp server that's running. */
interface PickableModel {
  key: string
  /** The group header: a provider, a custom provider's name, or local. */
  group: string
  model: SelectableModel
}

const modelKey = (m: Pick<SelectableModel, 'provider' | 'id'>) => `${m.provider}::${m.id}`

/** Every model the user can pick right now, loaded on first use. */
/** The "Auto" entry: not a model, but the cost-aware router, which picks one
 * per task from the Easy, Medium and Hard models set in Settings. */
const ROUTER_MODEL: SelectableModel = {
  id: 'viv-auto',
  name: 'Auto (cost-aware router)',
  provider: 'router',
  api: 'openai-chat',
  contextWindow: 128000,
  maxOutputTokens: 8192,
  inputCostPerMillion: null,
  outputCostPerMillion: null,
  isImageModel: false,
  isAudioModel: false,
  supportsVision: false,
}

function usePickableModels(withRouter = false): PickableModel[] {
  const cloud = useModelStore((s) => s.models)
  const init = useModelStore((s) => s.init)
  const llama = useLlamaStore((s) => s.endpoints)
  const refreshLlama = useLlamaStore((s) => s.refresh)
  const custom = useCustomProviderStore((s) => s.providers)
  const refreshCustom = useCustomProviderStore((s) => s.refresh)
  const imageFiles = useImageModels((s) => s.models)
  const imageReady = useImageModels((s) => s.installed)
  const refreshImages = useImageModels((s) => s.refresh)
  useEffect(() => {
    void init()
    void refreshLlama()
    void refreshCustom()
    void refreshImages()
  }, [init, refreshLlama, refreshCustom, refreshImages])

  return useMemo(() => {
    const out: PickableModel[] = []
    if (withRouter) out.push({ key: modelKey(ROUTER_MODEL), group: 'router', model: ROUTER_MODEL })
    for (const m of cloud) out.push({ key: modelKey(m), group: m.provider, model: m })
    for (const cp of custom) {
      for (const m of cp.models) {
        const model: SelectableModel = {
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
        }
        out.push({ key: modelKey(model), group: cp.name, model })
      }
    }
    for (const e of llama) {
      if (!isRunning(e.status)) continue
      const model: SelectableModel = {
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
      }
      out.push({ key: modelKey(model), group: 'local', model })
    }
    // Image models run by stable-diffusion.cpp: one per downloaded file.
    if (imageReady) {
      for (const f of imageFiles) {
        const model: SelectableModel = {
          id: f.file,
          name: f.file.replace(/\.(safetensors|gguf|ckpt)$/i, ''),
          provider: 'sd',
          api: 'image',
          contextWindow: null,
          maxOutputTokens: null,
          inputCostPerMillion: 0,
          outputCostPerMillion: 0,
          isImageModel: true,
          isAudioModel: false,
          supportsVision: false,
        }
        out.push({ key: modelKey(model), group: 'sd', model })
      }
    }
    return out
  }, [cloud, custom, llama, imageFiles, imageReady])
}

/** What an agent stores to run on a model. */
function toOverride(m: SelectableModel): SubagentModelOverride {
  return {
    id: m.id,
    name: m.name,
    api: m.api,
    provider: m.provider,
    baseUrl: m.baseUrl ?? null,
    maxTokens: m.maxOutputTokens ?? 8192,
    contextWindow: m.contextWindow ?? 128000,
  }
}

// Favourites and recents, per user, across every picker.
const FAV_KEY = 'viv.models.favourites'
const RECENT_KEY = 'viv.models.recent'
function readList(key: string): string[] {
  const v = loadPersisted<unknown>(key, [])
  return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []
}
const writeList = savePersisted

const FILTERS = [
  { id: 'vision', label: 'Vision', title: 'Accepts images', test: (m: SelectableModel) => m.supportsVision },
  { id: 'free', label: 'Free', title: 'No input or output cost', test: (m: SelectableModel) => m.inputCostPerMillion === 0 && m.outputCostPerMillion === 0 },
  { id: 'long', label: '200K+', title: 'Context window of 200K tokens or more', test: (m: SelectableModel) => (m.contextWindow ?? 0) >= 200_000 },
  { id: 'image', label: 'Image', title: 'Draws pictures instead of writing text', test: (m: SelectableModel) => m.isImageModel },
  { id: 'local', label: 'Local', title: 'Runs on this computer', test: (m: SelectableModel) => m.provider === 'local' },
] as const
type FilterId = (typeof FILTERS)[number]['id']

const INHERIT = '__inherit__'

type Row = { kind: 'header'; label: string; provider: string } | { kind: 'model'; item: PickableModel; section: string } | { kind: 'inherit' }

function fmtCtx(n: number | null | undefined) {
  if (!n) return null
  return n >= 1_000_000 ? `${(n / 1_000_000).toFixed(n % 1_000_000 ? 1 : 0)}M` : `${Math.round(n / 1000)}K`
}
function fmtCost(n: number | null | undefined) {
  if (n === null || n === undefined) return '—'
  if (n === 0) return 'free'
  return `$${n < 1 ? n.toFixed(2) : n.toFixed(n < 10 ? 2 : 0)}`
}

/**
 * The one model picker. `mode="chat"` sets the chat's model; `mode="agent"`
 * picks an agent's model, with "Same as the chat" first. Search, filters,
 * favourites and recents, grouped by provider, all from the keyboard.
 */
export function ModelPicker(
  props:
    | { mode: 'chat'; variant?: 'pill' | 'field' }
    | { mode: 'agent'; value: SubagentModelOverride | null; onChange: (m: SubagentModelOverride | null) => void; variant?: 'pill' | 'field' },
) {
  const theme = useTheme()
  // Agents get Auto too: the cost-aware router picks a tier for each of
  // their messages, the same as it does for the chat.
  const items = usePickableModels(true)
  const stats = useModelStore((s) => s.stats)
  const routed = useModelStore((s) => s.routed)
  const selectModel = useModelStore((s) => s.selectModel)
  const openSettings = useSettingsUi((s) => s.openSettings)
  const [anchor, setAnchor] = useState<HTMLElement | null>(null)
  const [query, setQuery] = useState('')
  const [active, setActive] = useState<FilterId[]>([])
  const [cursor, setCursor] = useState(0)
  const [tab, setTab] = useState<'models' | 'agents'>('models')
  const harnesses = useHarnessStore((s) => s.list)
  const harnessError = useHarnessStore((s) => s.error)
  const refreshHarnesses = useHarnessStore((s) => s.refresh)
  const [favs, setFavs] = useState<string[]>(() => readList(FAV_KEY))
  const listRef = useRef<HTMLDivElement | null>(null)
  const variant = props.variant ?? 'pill'

  const currentKey = props.mode === 'chat' ? (stats ? `${stats.provider}::${stats.modelId}` : null) : props.value ? modelKey(props.value) : INHERIT
  const currentName =
    props.mode === 'chat' ? (stats?.provider === 'router' && routed ? `Auto · ${routed.model}` : stats?.modelName || 'Select model') : props.value ? (items.find((i) => i.key === currentKey)?.model.name ?? props.value.name) : 'Same as the chat'
  const currentProvider = props.mode === 'chat' ? (stats?.provider ?? null) : (props.value?.provider ?? null)

  // Coding agents are listed on first open, and only where they can be used:
  // the chat. An agent's own model is always one of Vivianne's.
  useEffect(() => {
    if (anchor && props.mode === 'chat') void refreshHarnesses()
  }, [anchor, props.mode, refreshHarnesses])

  useEffect(() => {
    if (anchor) return
    setQuery('')
    setActive([])
    setCursor(0)
    setTab(currentProvider === HARNESS ? 'agents' : 'models')
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [anchor])

  const rows = useMemo<Row[]>(() => {
    const q = query.trim().toLowerCase()
    const tests = FILTERS.filter((f) => active.includes(f.id))
    const match = (i: PickableModel) =>
      (!q || i.model.name.toLowerCase().includes(q) || i.model.id.toLowerCase().includes(q) || providerLabel(i.group).toLowerCase().includes(q)) && tests.every((f) => f.test(i.model))
    const visible = items.filter(match)
    const byKey = new Map(visible.map((i) => [i.key, i]))
    const out: Row[] = []
    if (props.mode === 'agent' && !q && active.length === 0) out.push({ kind: 'inherit' })
    const pinned = (label: string, keys: string[]) => {
      const list = keys.map((k) => byKey.get(k)).filter((i): i is PickableModel => !!i)
      if (!list.length) return
      out.push({ kind: 'header', label, provider: '' })
      for (const item of list) out.push({ kind: 'model', item, section: label })
    }
    if (!q) {
      pinned('Favourites', favs)
      pinned(
        'Recent',
        readList(RECENT_KEY)
          .filter((k) => !favs.includes(k))
          .slice(0, 4),
      )
    }
    // A model already shown under Favourites or Recent is not listed a second time below them.
    const pinnedKeys = new Set(out.filter((r): r is Extract<Row, { kind: 'model' }> => r.kind === 'model').map((r) => r.item.key))
    const groups = new Map<string, PickableModel[]>()
    for (const i of visible) if (!pinnedKeys.has(i.key)) groups.set(i.group, [...(groups.get(i.group) ?? []), i])
    // Local last: a different kind of thing from the cloud providers.
    const last = (g: string) => (g === 'local' ? 1 : g === 'sd' ? 2 : 0)
    const order = [...groups.keys()].sort((a, b) => last(a) - last(b))
    for (const g of order) {
      out.push({ kind: 'header', label: providerLabel(g), provider: g })
      for (const item of groups.get(g)!) out.push({ kind: 'model', item, section: g })
    }
    return out
  }, [items, query, active, favs, props.mode])

  const selectable = rows.map((r, i) => (r.kind === 'header' ? -1 : i)).filter((i) => i >= 0)
  const cursorRow = rows[selectable[Math.min(cursor, selectable.length - 1)]]
  const detail = cursorRow?.kind === 'model' ? cursorRow.item.model : null

  // Keep the cursor's row in view as the arrows move it.
  useEffect(() => {
    listRef.current?.querySelector('[data-cursor="true"]')?.scrollIntoView({ block: 'nearest' })
  }, [cursor])

  const choose = (row: Row) => {
    if (row.kind === 'header') return
    if (row.kind === 'inherit') {
      if (props.mode === 'agent') props.onChange(null)
    } else {
      const k = row.item.key
      writeList(RECENT_KEY, [k, ...readList(RECENT_KEY).filter((x) => x !== k)].slice(0, 8))
      if (props.mode === 'chat') void selectModel(row.item.model)
      else props.onChange(toOverride(row.item.model))
    }
    setAnchor(null)
  }

  const toggleFav = (k: string) => {
    const next = favs.includes(k) ? favs.filter((x) => x !== k) : [...favs, k]
    setFavs(next)
    writeList(FAV_KEY, next)
  }

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      if (!selectable.length) return
      setCursor((c) => (e.key === 'ArrowDown' ? (c + 1) % selectable.length : (c - 1 + selectable.length) % selectable.length))
    } else if (e.key === 'Enter') {
      e.preventDefault()
      if (cursorRow) choose(cursorRow)
    }
  }

  const empty = items.length === 0
  const ring = theme.palette.tokens.ring

  return (
    <>
      <Tooltip title={`Model: ${currentName}`}>
        <ButtonBase
          className="cq-collapsible"
          onClick={(e) => setAnchor(e.currentTarget)}
          aria-label={`Model: ${currentName}`}
          aria-haspopup="dialog"
          sx={variant === 'field' ? {
            gap: 0.75,
            px: 1,
            height: 32,
            minWidth: 0,
            maxWidth: variant === 'field' ? '100%' : 220,
            width: variant === 'field' ? '100%' : undefined,
            justifyContent: 'flex-start',
            borderRadius: variant === 'field' ? '8px' : '999px',
            border: variant === 'field' ? 1 : 0,
            borderColor: 'divider',
            fontSize: '0.8125rem',
            color: 'text.primary',
            '&:hover': { bgcolor: 'action.hover' },
          } : { ...TOOLBAR_PILL, maxWidth: 220, color: 'text.primary' }}
        >
          {variant === 'field' && (currentProvider ? <ProviderMark provider={currentProvider} size={14} /> : <CpuLinear size={14} color="currentColor" />)}
          {/* Never hidden when the composer narrows (`cq-model` only shortens it):
              which model is selected is the one thing that must stay readable. */}
          <Typography className="cq-model" noWrap sx={{ fontSize: variant === 'field' ? '0.8125rem' : 'inherit', fontWeight: 500, minWidth: 0, flex: variant === 'field' ? 1 : undefined, textAlign: 'left' }}>
            {currentName}
          </Typography>
          {variant === 'field' && (
            <Box sx={{ display: 'flex', color: 'text.secondary', flexShrink: 0 }}>
              <ArrowDownLinear size={12} color="currentColor" />
            </Box>
          )}
        </ButtonBase>
      </Tooltip>
      <Popover
        open={!!anchor}
        anchorEl={anchor}
        onClose={() => setAnchor(null)}
        anchorOrigin={{ vertical: 'top', horizontal: variant === 'field' ? 'left' : 'right' }}
        transformOrigin={{ vertical: 'bottom', horizontal: variant === 'field' ? 'left' : 'right' }}
        slotProps={{ paper: { sx: { width: 400, p: 0, overflow: 'hidden', display: 'flex', flexDirection: 'column', borderRadius: '12px' } } }}
      >
        <Box onKeyDown={onKey} sx={{ display: 'flex', flexDirection: 'column', minHeight: 0, maxHeight: 'min(520px, 70vh)' }}>
          {/* Search and filters stay put; only the list scrolls, so group
              headers stick under them instead of sliding beneath. */}
          <Box sx={{ flexShrink: 0, borderBottom: 1, borderColor: 'divider' }}>
            {props.mode === 'chat' && (
              <Stack direction="row" role="tablist" sx={{ gap: 0.5, px: 1.25, pt: 1.25 }}>
                {([['models', 'Models'], ['agents', 'Coding agents']] as const).map(([id, label]) => (
                  <ButtonBase
                    key={id}
                    role="tab"
                    aria-selected={tab === id}
                    onClick={() => {
                      setTab(id)
                      setCursor(0)
                    }}
                    sx={{
                      px: 1.25,
                      height: 28,
                      borderRadius: '8px',
                      fontSize: '0.75rem',
                      fontWeight: 500,
                      color: tab === id ? 'text.primary' : 'text.secondary',
                      bgcolor: tab === id ? alpha(ring, 0.16) : 'transparent',
                      '&:hover': { bgcolor: tab === id ? alpha(ring, 0.2) : 'action.hover' },
                    }}
                  >
                    {label}
                  </ButtonBase>
                ))}
              </Stack>
            )}
            <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 1.5, pt: 1, pb: 0.75 }}>
              <SearchNormalLinear size={14} color="currentColor" />
              <InputBase
                autoFocus
                fullWidth
                placeholder={tab === 'agents' ? 'Search coding agents' : 'Search models'}
                value={query}
                onChange={(e) => {
                  setQuery(e.target.value)
                  setCursor(0)
                }}
                sx={{ fontSize: '0.875rem' }}
              />
              {query && (
                <ButtonBase aria-label="Clear search" onClick={() => setQuery('')} sx={{ color: 'text.secondary', borderRadius: '50%' }}>
                  <CloseCircleLinear size={14} color="currentColor" />
                </ButtonBase>
              )}
            </Stack>
            {tab === 'models' && (
            <Stack direction="row" sx={{ gap: 0.5, px: 1.5, pb: 1, flexWrap: 'wrap' }}>
              {FILTERS.map((f) => {
                const on = active.includes(f.id)
                return (
                  <Tooltip key={f.id} title={f.title}>
                    <ButtonBase
                      aria-pressed={on}
                      onClick={() => {
                        setActive((p) => (on ? p.filter((x) => x !== f.id) : [...p, f.id]))
                        setCursor(0)
                      }}
                      sx={{
                        px: 1,
                        height: 24,
                        borderRadius: '999px',
                        fontSize: '0.6875rem',
                        fontWeight: 500,
                        border: 1,
                        borderColor: on ? ring : 'divider',
                        color: on ? 'text.primary' : 'text.secondary',
                        bgcolor: on ? alpha(ring, 0.14) : 'transparent',
                        '&:hover': { bgcolor: on ? alpha(ring, 0.2) : 'action.hover' },
                      }}
                    >
                      {f.label}
                    </ButtonBase>
                  </Tooltip>
                )
              })}
            </Stack>
            )}
          </Box>

          <Box ref={listRef} role="listbox" sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: 0.5, pb: 0.5 }}>
            {tab === 'agents' && (
              <AgentList
                harnesses={harnesses}
                error={harnessError}
                query={query}
                currentId={currentProvider === HARNESS ? (stats?.modelId ?? null) : null}
                onSettings={() => {
                  setAnchor(null)
                  openSettings('providers')
                }}
                onPick={(h) => {
                  void selectModel(harnessModel(h))
                  setAnchor(null)
                }}
              />
            )}
            {tab === 'models' && empty && (
              <Typography sx={{ px: 1.5, py: 2, fontSize: '0.8125rem', color: 'text.secondary' }}>No models yet. Add a provider in Settings.</Typography>
            )}
            {tab === 'models' && !empty && selectable.length === 0 && (
              <Typography sx={{ px: 1.5, py: 2, fontSize: '0.8125rem', color: 'text.secondary' }}>{active.length ? 'No models match those filters.' : 'No matches.'}</Typography>
            )}
            {tab === 'models' && rows.map((r, i) => {
              if (r.kind === 'header') {
                return (
                  <Stack
                    key={`h${i}`}
                    direction="row"
                    sx={{ position: 'sticky', top: 0, zIndex: 1, bgcolor: 'background.paper', alignItems: 'center', gap: 0.75, px: 1, pt: 1, pb: 0.5 }}
                  >
                    {r.provider && <ProviderMark provider={r.provider} size={12} />}
                    <Typography sx={{ fontSize: '0.6875rem', fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.4, color: 'text.secondary' }}>{r.label}</Typography>
                  </Stack>
                )
              }
              const isCursor = cursorRow === r
              const key = r.kind === 'inherit' ? INHERIT : r.item.key
              const selected = key === currentKey
              const m = r.kind === 'model' ? r.item.model : null
              const split = m ? splitVendor(m.name) : null
              return (
                <Stack
                  key={`${key}${r.kind === 'model' ? r.section : ''}`}
                  role="option"
                  aria-selected={selected}
                  data-cursor={isCursor}
                  direction="row"
                  onClick={() => choose(r)}
                  onMouseMove={() => {
                    const pos = selectable.indexOf(i)
                    if (pos >= 0 && pos !== cursor) setCursor(pos)
                  }}
                  sx={{
                    alignItems: 'center',
                    gap: 0.875,
                    px: 1,
                    height: 32,
                    borderRadius: '8px',
                    cursor: 'pointer',
                    bgcolor: isCursor ? 'action.hover' : 'transparent',
                    '&:hover .fav, & .fav[data-on="true"]': { opacity: 1 },
                  }}
                >
                  {r.kind === 'inherit' ? (
                    <>
                      <Box sx={{ width: 16, display: 'grid', placeItems: 'center', color: 'text.secondary' }}>↺</Box>
                      <Typography noWrap sx={{ fontSize: '0.8125rem', flex: 1 }}>
                        Same as the chat
                      </Typography>
                    </>
                  ) : (
                    <>
                      <ProviderMark provider={m!.provider} size={16} />
                      {split!.vendor && (
                        <Box
                          component="span"
                          sx={{
                            flexShrink: 0,
                            px: 0.625,
                            borderRadius: '6px',
                            fontSize: '0.6875rem',
                            fontWeight: 500,
                            lineHeight: '18px',
                            color: 'text.secondary',
                            bgcolor: 'action.hover',
                          }}
                        >
                          {split!.vendor}
                        </Box>
                      )}
                      <Typography noWrap sx={{ fontSize: '0.8125rem', flex: 1, minWidth: 0, fontWeight: selected ? 600 : 400 }}>
                        {split!.rest}
                      </Typography>
                      {m!.supportsVision && (
                        <Box component="span" sx={{ flexShrink: 0, px: 0.5, borderRadius: '4px', fontSize: '0.6875rem', fontWeight: 500, lineHeight: '16px', color: 'text.secondary', border: 1, borderColor: 'divider' }}>
                          Vision
                        </Box>
                      )}
                      {fmtCtx(m!.contextWindow) && (
                        <Typography sx={{ fontSize: '0.6875rem', color: 'text.disabled', fontFamily: MONO, flexShrink: 0, width: 38, textAlign: 'right' }}>{fmtCtx(m!.contextWindow)}</Typography>
                      )}
                      <ButtonBase
                        className="fav"
                        data-on={favs.includes(r.item.key)}
                        aria-label={favs.includes(r.item.key) ? 'Remove from favourites' : 'Add to favourites'}
                        onClick={(e) => {
                          e.stopPropagation()
                          toggleFav(r.item.key)
                        }}
                        sx={{ opacity: 0, borderRadius: '6px', p: 0.25, color: favs.includes(r.item.key) ? 'warning.main' : 'text.secondary', flexShrink: 0 }}
                      >
                        {favs.includes(r.item.key) ? <StarBold size={13} color="currentColor" /> : <StarLinear size={13} color="currentColor" />}
                      </ButtonBase>
                    </>
                  )}
                  <Box sx={{ width: 14, display: 'flex', color: 'primary.main', flexShrink: 0 }}>{selected && <TickCircleLinear size={14} color="currentColor" />}</Box>
                </Stack>
              )
            })}
            {tab === 'models' && !query && active.length === 0 && !items.some((i) => i.group === 'local') && (
              <Stack
                direction="row"
                onClick={() => {
                  setAnchor(null)
                  openSettings('local')
                }}
                sx={{ alignItems: 'center', gap: 1, px: 1, height: 32, mt: 0.5, borderRadius: '8px', cursor: 'pointer', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}
              >
                <CpuLinear size={16} color="currentColor" />
                <Typography sx={{ fontSize: '0.8125rem', flex: 1 }}>Get a model that runs on this computer</Typography>
                <ArrowRightLinear size={14} color="currentColor" />
              </Stack>
            )}
          </Box>

          {/* What the highlighted model costs and holds. */}
          {tab === 'models' && detail && (detail.contextWindow || detail.maxOutputTokens || detail.inputCostPerMillion != null || detail.outputCostPerMillion != null) && (
            <Stack direction="row" sx={{ flexShrink: 0, gap: 2, px: 1.5, py: 0.875, borderTop: 1, borderColor: 'divider', bgcolor: alpha(theme.palette.text.primary, 0.02) }}>
              <Stat label="Context" value={fmtCtx(detail.contextWindow) ?? '—'} />
              <Stat label="Max out" value={fmtCtx(detail.maxOutputTokens) ?? '—'} />
              <Stat label="In / 1M" value={fmtCost(detail.inputCostPerMillion)} />
              <Stat label="Out / 1M" value={fmtCost(detail.outputCostPerMillion)} />
            </Stack>
          )}
        </Box>
      </Popover>
    </>
  )
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <Stack sx={{ gap: 0.125, minWidth: 0 }}>
      <Typography sx={{ fontSize: '0.6875rem', textTransform: 'uppercase', letterSpacing: 0.5, color: 'text.disabled' }}>{label}</Typography>
      <Typography sx={{ fontSize: '0.75rem', fontFamily: MONO }}>{value}</Typography>
    </Stack>
  )
}

function AgentList({
  harnesses,
  error,
  query,
  currentId,
  onPick,
  onSettings,
}: {
  harnesses: Harness[] | null
  error: boolean
  query: string
  currentId: string | null
  onPick: (h: Harness) => void
  onSettings: () => void
}) {
  const busy = useHarnessStore((s) => s.busy)
  const install = useHarnessStore((s) => s.install)
  const progress = useHarnessStore((s) => s.progress)
  const failures = useHarnessStore((s) => s.failures)
  if (!harnesses) return <Typography sx={{ px: 1.5, py: 2, fontSize: '0.8125rem', color: 'text.secondary' }}>Looking up coding agents…</Typography>
  const q = query.trim().toLowerCase()
  const list = harnesses.filter((h) => !q || h.name.toLowerCase().includes(q) || h.id.toLowerCase().includes(q))
  if (!list.length) {
    return (
      <Typography sx={{ px: 1.5, py: 2, fontSize: '0.8125rem', color: 'text.secondary' }}>
        {error ? 'Could not load the agent list.' : 'No coding agents match.'}
      </Typography>
    )
  }
  return (
    <>
      <Typography sx={{ px: 1.25, pt: 1, pb: 0.5, fontSize: '0.75rem', color: 'text.secondary', lineHeight: 1.4 }}>
        The thread's turns run in that agent, with its own tools. Manage them in{' '}
        <Box component="span" onClick={onSettings} sx={{ color: 'primary.main', cursor: 'pointer', textDecoration: 'underline' }}>
          Settings
        </Box>
        .
      </Typography>
      {list.map((h) => {
        const selected = h.id === currentId
        const status = statusOf(h)
        const working = busy[h.id] === 'install'
        const failure = failures[h.id]
        return (
          <Stack key={h.id} sx={{ borderRadius: '8px', '&:hover': { bgcolor: 'action.hover' } }}>
            <Stack
              role="option"
              aria-selected={selected}
              aria-disabled={status === 'missing'}
              direction="row"
              onClick={() => status !== 'missing' && !working && onPick(h)}
              sx={{ alignItems: 'center', gap: 1, px: 1, py: 0.75, cursor: status === 'missing' ? 'not-allowed' : 'pointer' }}
            >
              <Box sx={{ opacity: status === 'missing' ? 0.45 : 1, display: 'flex' }}>
                <ProviderMark provider={h.id} size={20} />
              </Box>
              <Stack sx={{ flex: 1, minWidth: 0, opacity: status === 'missing' ? 0.55 : 1 }}>
                <Typography noWrap sx={{ fontSize: '0.8125rem', fontWeight: selected ? 600 : 500 }}>
                  {h.name}
                </Typography>
                <Typography noWrap sx={{ fontSize: '0.6875rem', color: 'text.secondary' }}>
                  {working ? (progress[h.id] ? `${(progress[h.id].bytes / 1e6).toFixed(0)} MB downloaded · ${progress[h.id].line}` : 'Starting the download…') : h.description}
                </Typography>
              </Stack>
              {status === 'missing' && (
                <Tooltip title="Needs Node.js (npx) or uv (uvx) installed to start this agent.">
                  <Box component="span" sx={{ flexShrink: 0, px: 0.75, borderRadius: '6px', fontSize: '0.6875rem', fontWeight: 500, lineHeight: '18px', color: 'text.secondary', border: 1, borderColor: 'divider' }}>
                    Not installed
                  </Box>
                </Tooltip>
              )}
              {status === 'setup' && (
                <ButtonBase
                  disabled={working}
                  onClick={(e) => {
                    e.stopPropagation()
                    void install(h.id)
                  }}
                  sx={{ flexShrink: 0, px: 1, height: 24, borderRadius: '6px', fontSize: '0.6875rem', fontWeight: 500, border: 1, borderColor: 'divider', '&:hover': { bgcolor: 'action.selected' } }}
                >
                  {working ? 'Setting up…' : 'Set up'}
                </ButtonBase>
              )}
              <Box sx={{ width: 14, display: 'flex', color: 'primary.main', flexShrink: 0 }}>{selected && <TickCircleLinear size={14} color="currentColor" />}</Box>
            </Stack>
            {failure && <Typography sx={{ px: 1, pb: 0.75, fontSize: '0.6875rem', color: 'error.main', whiteSpace: 'pre-wrap' }}>{failure}</Typography>}
          </Stack>
        )
      })}
    </>
  )
}
