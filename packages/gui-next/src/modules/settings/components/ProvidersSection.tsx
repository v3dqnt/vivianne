import { useEffect, useState } from 'react'
import { Box, Stack, Typography, ButtonBase, Button, IconButton, TextField, Select, MenuItem, Dialog, DialogTitle, DialogContent, DialogActions, Collapse, CircularProgress } from '../../../ui'
import { AddLinear, TrashLinear, ArrowDownLinear } from '../../../icons'
import { CLOUD_PROVIDERS } from '../lib/providers'
import { API_FORMAT_OPTIONS, type CustomApiFormat, type CustomModel } from '../lib/customProviders'
import { useAuthStore } from '../store/authStore'
import { useCustomProviderStore } from '../store/customProviderStore'
import { ProviderRow } from './ProviderRow'
import { ChatGptPlanPanel, chatGptStatus } from './ChatGptPlanPanel'
import { RoutingPanel } from './RoutingPanel'

// Deterministic color per provider id — same trick `ModelPicker.tsx` uses
// for its picker marks, duplicated locally rather than shared since it's a
// five-line pure function and the two call sites want slightly different
// sizes/contexts.
function markColor(id: string): string {
  let hash = 0
  for (let i = 0; i < id.length; i++) hash = (hash * 31 + id.charCodeAt(i)) >>> 0
  return `hsl(${hash % 360}, 55%, 55%)`
}

function Mark({ id, size = 20 }: { id: string; size?: number }) {
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
        bgcolor: markColor(id),
        color: 'rgba(0,0,0,0.75)',
        fontSize: size * 0.5,
        fontWeight: 700,
        lineHeight: 1,
      }}
    >
      {id.charAt(0).toUpperCase()}
    </Box>
  )
}

function ListRow({ label, markId, connected, selected, onClick }: { label: string; markId: string; connected: boolean; selected: boolean; onClick: () => void }) {
  return (
    <ButtonBase
      onClick={onClick}
      sx={{
        display: 'flex',
        alignItems: 'center',
        gap: 1,
        px: 1,
        py: 0.75,
        borderRadius: 1.5,
        width: '100%',
        justifyContent: 'flex-start',
        bgcolor: selected ? 'action.selected' : 'transparent',
        transition: 'background-color 0.15s',
        '&:hover': { bgcolor: selected ? 'action.selected' : 'action.hover' },
      }}
    >
      <Mark id={markId} />
      <Typography variant="body2" sx={{ flex: 1, minWidth: 0, textAlign: 'left', fontWeight: selected ? 600 : 400 }} noWrap>
        {label}
      </Typography>
      <Box sx={{ width: 6, height: 6, borderRadius: '50%', flexShrink: 0, bgcolor: connected ? 'success.main' : 'text.disabled' }} />
    </ButtonBase>
  )
}

function AddModelDialog({ open, onClose, onAdd }: { open: boolean; onClose: () => void; onAdd: (m: CustomModel) => void }) {
  const [id, setId] = useState('')
  const [contextWindow, setContextWindow] = useState(200000)
  const [advancedOpen, setAdvancedOpen] = useState(false)
  const [maxOutputTokens, setMaxOutputTokens] = useState('')

  useEffect(() => {
    if (!open) return
    setId('')
    setContextWindow(200000)
    setAdvancedOpen(false)
    setMaxOutputTokens('')
  }, [open])

  const submit = () => {
    if (!id.trim()) return
    onAdd({ id: id.trim(), contextWindow, maxOutputTokens: maxOutputTokens.trim() ? Number(maxOutputTokens) : null })
    onClose()
  }

  return (
    <Dialog open={open} onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle>Add model</DialogTitle>
      <DialogContent>
        <Stack spacing={2} sx={{ pt: 0.5 }}>
          <TextField label="Model ID" size="small" placeholder="Model ID" value={id} onChange={(e) => setId(e.target.value)} autoFocus />
          <TextField
            label="Context window"
            size="small"
            type="number"
            value={contextWindow}
            onChange={(e) => setContextWindow(Math.max(1, Number(e.target.value) || 0))}
          />
          <ButtonBase
            onClick={() => setAdvancedOpen((v) => !v)}
            sx={{ display: 'flex', alignItems: 'center', gap: 0.5, alignSelf: 'flex-start', color: 'text.secondary', fontSize: '0.8125rem', fontWeight: 600 }}
          >
            <ArrowDownLinear size={12} color="currentColor" style={{ transform: advancedOpen ? 'rotate(180deg)' : undefined, transition: 'transform 0.15s' }} />
            Advanced
          </ButtonBase>
          <Collapse in={advancedOpen}>
            <TextField
              label="Max output tokens"
              size="small"
              fullWidth
              type="number"
              value={maxOutputTokens}
              onChange={(e) => setMaxOutputTokens(e.target.value)}
            />
          </Collapse>
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} color="inherit">
          Cancel
        </Button>
        <Button onClick={submit} variant="contained" disableElevation disabled={!id.trim()}>
          Save
        </Button>
      </DialogActions>
    </Dialog>
  )
}

/** Add/edit form for a custom provider. For a brand-new one (`providerId
 * === null`) models are staged in local state and only persisted once the
 * provider itself is created — mirrors the reference flow where the whole
 * thing (including its model list) is built up before the first save.
 * Editing an existing provider writes each model change immediately since
 * there's already an id to attach it to. */
function CustomProviderForm({ providerId, onSaved, onRemoved }: { providerId: string | null; onSaved: (id: string) => void; onRemoved: () => void }) {
  const providers = useCustomProviderStore((s) => s.providers)
  const addProvider = useCustomProviderStore((s) => s.addProvider)
  const updateProvider = useCustomProviderStore((s) => s.updateProvider)
  const setApiKey = useCustomProviderStore((s) => s.setApiKey)
  const removeProvider = useCustomProviderStore((s) => s.removeProvider)
  const addModel = useCustomProviderStore((s) => s.addModel)
  const removeModel = useCustomProviderStore((s) => s.removeModel)

  const existing = providerId ? (providers.find((p) => p.id === providerId) ?? null) : null

  const [name, setName] = useState('')
  const [baseUrl, setBaseUrl] = useState('')
  const [apiKey, setApiKeyDraft] = useState('')
  const [apiFormat, setApiFormat] = useState<CustomApiFormat>('anthropicMessages')
  const [draftModels, setDraftModels] = useState<CustomModel[]>([])
  const [modelDialogOpen, setModelDialogOpen] = useState(false)
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    setName(existing?.name ?? '')
    setBaseUrl(existing?.baseUrl ?? '')
    setApiFormat(existing?.apiFormat ?? 'anthropicMessages')
    setDraftModels([])
    setApiKeyDraft('')
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [providerId])

  const models = existing ? existing.models : draftModels

  const handleAddModel = async (m: CustomModel) => {
    if (existing) await addModel(existing.id, m)
    else setDraftModels((prev) => [...prev, m])
  }

  const handleRemoveModel = async (modelId: string) => {
    if (existing) await removeModel(existing.id, modelId)
    else setDraftModels((prev) => prev.filter((m) => m.id !== modelId))
  }

  const submit = async () => {
    if (!name.trim() || !baseUrl.trim()) return
    setSaving(true)
    try {
      if (existing) {
        await updateProvider(existing.id, { name: name.trim(), baseUrl: baseUrl.trim(), apiFormat })
        if (apiKey.trim()) await setApiKey(existing.id, apiKey.trim())
        onSaved(existing.id)
      } else {
        const id = await addProvider({ name: name.trim(), baseUrl: baseUrl.trim(), apiFormat, apiKey })
        for (const m of draftModels) await addModel(id, m)
        onSaved(id)
      }
    } finally {
      setSaving(false)
    }
  }

  const remove = async () => {
    if (!existing) return
    await removeProvider(existing.id)
    onRemoved()
  }

  return (
    <Stack sx={{ gap: 2.5, maxWidth: 480 }}>
      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
          {existing ? 'Edit model provider' : 'Add model provider'}
        </Typography>
        <Typography variant="caption" color="text.secondary">
          Configure a custom API endpoint and initial model.
        </Typography>
      </Stack>

      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Name
        </Typography>
        <TextField size="small" placeholder="e.g. DeepSeek" value={name} onChange={(e) => setName(e.target.value)} />
      </Stack>

      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Base URL
        </Typography>
        <TextField size="small" placeholder="https://api.example.com/v1" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} />
      </Stack>

      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          API key
        </Typography>
        <TextField
          size="small"
          type="password"
          placeholder={existing?.hasKey ? 'Key set — leave blank to keep it' : 'Enter API key'}
          value={apiKey}
          onChange={(e) => setApiKeyDraft(e.target.value)}
        />
      </Stack>

      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          API format
        </Typography>
        <Select size="small" value={apiFormat} disabled={!!existing} onChange={(e) => setApiFormat(e.target.value as CustomApiFormat)}>
          {API_FORMAT_OPTIONS.map((o) => (
            <MenuItem key={o.value} value={o.value}>
              {o.label}
            </MenuItem>
          ))}
        </Select>
        {existing && (
          <Typography variant="caption" color="text.secondary">
            Fixed once created — remove and re-add this provider to change it.
          </Typography>
        )}
      </Stack>

      <Stack sx={{ gap: 0.75 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Model list
        </Typography>
        {models.length === 0 ? (
          <Typography variant="caption" color="text.secondary">
            No models added yet.
          </Typography>
        ) : (
          models.map((m) => (
            <Stack key={m.id} direction="row" sx={{ alignItems: 'center', gap: 1, py: 0.5, px: 1, borderRadius: 1.5, bgcolor: 'action.hover' }}>
              <Typography variant="body2" sx={{ flex: 1, minWidth: 0, fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.75rem' }} noWrap>
                {m.id}
              </Typography>
              <Typography variant="caption" color="text.secondary" sx={{ flexShrink: 0 }}>
                {Math.round(m.contextWindow / 1000)}K ctx
              </Typography>
              <IconButton size="small" onClick={() => void handleRemoveModel(m.id)}>
                <TrashLinear size={13} color="currentColor" />
              </IconButton>
            </Stack>
          ))
        )}
        <Button
          size="small"
          onClick={() => setModelDialogOpen(true)}
          startIcon={<AddLinear size={14} color="currentColor" />}
          sx={{ alignSelf: 'flex-start', textTransform: 'none' }}
        >
          Add model
        </Button>
      </Stack>

      <Stack direction="row" sx={{ gap: 1 }}>
        <Button
          variant="contained"
          disableElevation
          onClick={() => void submit()}
          disabled={saving || !name.trim() || !baseUrl.trim()}
          startIcon={saving ? <CircularProgress size={14} /> : undefined}
        >
          {existing ? 'Save provider' : 'Add provider'}
        </Button>
        {existing && (
          <Button color="error" onClick={() => void remove()}>
            Remove
          </Button>
        )}
      </Stack>

      <AddModelDialog open={modelDialogOpen} onClose={() => setModelDialogOpen(false)} onAdd={(m) => void handleAddModel(m)} />
    </Stack>
  )
}

export function ProvidersSection() {
  const refreshAuth = useAuthStore((s) => s.refresh)
  const authStatus = useAuthStore((s) => s.status)
  const customProviders = useCustomProviderStore((s) => s.providers)
  const refreshCustom = useCustomProviderStore((s) => s.refresh)
  const [selected, setSelected] = useState<string | null>(null)
  const [planConnected, setPlanConnected] = useState(false)

  useEffect(() => {
    void refreshAuth()
    void refreshCustom()
    void chatGptStatus().then((s) => setPlanConnected(!!s?.signedIn && s.sharing))
  }, [refreshAuth, refreshCustom])

  const selectedCloud = CLOUD_PROVIDERS.find((p) => p.id === selected) ?? null
  const selectedCustom = selected && selected !== '__new__' ? (customProviders.find((p) => p.id === selected) ?? null) : null
  const showCustomForm = selected === '__new__' || !!selectedCustom

  return (
    <Stack direction="row" sx={{ gap: 3, height: '100%', minHeight: 0 }}>
      <Stack sx={{ width: 200, flexShrink: 0, gap: 2, overflowY: 'auto' }}>
        <Stack sx={{ gap: 0.25 }}>
          <Typography
            variant="caption"
            color="text.secondary"
            sx={{ fontWeight: 600, letterSpacing: 0.5, textTransform: 'uppercase', fontSize: '0.6875rem', px: 1, mb: 0.5 }}
          >
            Providers
          </Typography>
          <ListRow label="ChatGPT plan" markId="openai" connected={planConnected} selected={selected === '__chatgpt__'} onClick={() => setSelected('__chatgpt__')} />
          {CLOUD_PROVIDERS.map((p) => (
            <ListRow key={p.id} label={p.label} markId={p.id} connected={authStatus[p.id] ?? false} selected={selected === p.id} onClick={() => setSelected(p.id)} />
          ))}
        </Stack>

        <Stack sx={{ gap: 0.25 }}>
          <Typography
            variant="caption"
            color="text.secondary"
            sx={{ fontWeight: 600, letterSpacing: 0.5, textTransform: 'uppercase', fontSize: '0.6875rem', px: 1, mb: 0.5 }}
          >
            Models
          </Typography>
          <ButtonBase
            onClick={() => setSelected('__routing__')}
            sx={{
              display: 'flex',
              alignItems: 'center',
              gap: 1,
              px: 1,
              py: 0.75,
              borderRadius: 1.5,
              width: '100%',
              justifyContent: 'flex-start',
              bgcolor: selected === '__routing__' ? 'action.selected' : 'transparent',
              '&:hover': { bgcolor: 'action.hover' },
            }}
          >
            <Typography variant="body2" sx={{ flex: 1, textAlign: 'left' }}>
              Cost-aware routing
            </Typography>
            <Typography variant="caption" sx={{ fontWeight: 700, color: 'warning.main', fontSize: '0.625rem' }}>
              BETA
            </Typography>
          </ButtonBase>
        </Stack>

        <Stack sx={{ gap: 0.25 }}>
          <Typography
            variant="caption"
            color="text.secondary"
            sx={{ fontWeight: 600, letterSpacing: 0.5, textTransform: 'uppercase', fontSize: '0.6875rem', px: 1, mb: 0.5 }}
          >
            Custom providers
          </Typography>
          {customProviders.map((p) => (
            <ListRow key={p.id} label={p.name} markId={p.id} connected={p.hasKey} selected={selected === p.id} onClick={() => setSelected(p.id)} />
          ))}
          <ButtonBase
            onClick={() => setSelected('__new__')}
            sx={{
              display: 'flex',
              alignItems: 'center',
              gap: 1,
              px: 1,
              py: 0.75,
              borderRadius: 1.5,
              width: '100%',
              justifyContent: 'flex-start',
              border: 1,
              borderStyle: 'dashed',
              borderColor: 'divider',
              color: 'text.secondary',
              transition: 'border-color 0.15s, color 0.15s',
              '&:hover': { borderColor: 'text.secondary', color: 'text.primary' },
            }}
          >
            <AddLinear size={14} color="currentColor" />
            <Typography variant="body2">Add provider</Typography>
          </ButtonBase>
        </Stack>
      </Stack>

      <Box sx={{ flex: 1, minWidth: 0, overflowY: 'auto' }}>
        {selectedCloud && (
          <Stack sx={{ gap: 0.5, maxWidth: 480 }}>
            <Typography variant="caption" color="text.secondary" sx={{ mb: 1 }}>
              Keys are stored in your OS keychain, never written to disk in plain text.
            </Typography>
            <ProviderRow provider={selectedCloud} />
          </Stack>
        )}
        {showCustomForm && <CustomProviderForm providerId={selectedCustom ? selectedCustom.id : null} onSaved={setSelected} onRemoved={() => setSelected(null)} />}
        {selected === '__chatgpt__' && <ChatGptPlanPanel onChange={(s) => setPlanConnected(!!s?.signedIn && s.sharing)} />}
        {selected === '__routing__' && <RoutingPanel />}
        {!selectedCloud && !showCustomForm && selected !== '__chatgpt__' && selected !== '__routing__' && (
          <Stack sx={{ height: '100%', alignItems: 'center', justifyContent: 'center' }}>
            <Typography variant="body2" color="text.secondary">
              Select a provider to configure it.
            </Typography>
          </Stack>
        )}
      </Box>
    </Stack>
  )
}
