import { useEffect, useMemo, useState } from 'react'
import { Box, Stack, Typography, IconButton, Button, ButtonBase, TextField, Select, MenuItem, Chip, Checkbox, Switch, CircularProgress, Tooltip } from '../../../ui'
import { AddLinear, RefreshLinear, SearchNormalLinear, ArrowLeftLinear, TrashLinear, EditLinear, UserOctagonLinear } from '../../../icons'
import { call } from '../../../ipc'
import { useModelStore } from '../../ai/store/modelStore'
import { useLlamaStore } from '../store/llamaStore'
import { useCustomProviderStore } from '../store/customProviderStore'
import { llamaModelId, isRunning } from '../lib/llama'
import { useSubagentProfileStore } from '../store/subagentProfileStore'
import { SUBAGENT_COLORS, SUBAGENT_COLOR_HEX, type SubagentModelOverride, type SubagentProfile } from '../lib/subagentProfiles'

interface ToolInfo {
  name: string
  description?: string
}

/** Every model currently usable anywhere in the app, in the shape a
 * subagent's model-override field needs — mirrors `ModelSelector.tsx`'s own
 * cloud+local+custom-provider aggregation (kept as a separate copy rather
 * than a shared import so this section doesn't risk destabilizing the
 * already-verified picker; the two lists are small enough that the
 * duplication is cheap). */
export function useModelOptions(): SubagentModelOverride[] {
  const cloudModels = useModelStore((s) => s.models)
  const init = useModelStore((s) => s.init)
  const llamaEndpoints = useLlamaStore((s) => s.endpoints)
  const refreshLlama = useLlamaStore((s) => s.refresh)
  const customProviders = useCustomProviderStore((s) => s.providers)
  const refreshCustomProviders = useCustomProviderStore((s) => s.refresh)

  useEffect(() => {
    void init()
    void refreshLlama()
    void refreshCustomProviders()
  }, [init, refreshLlama, refreshCustomProviders])

  return useMemo<SubagentModelOverride[]>(() => {
    const cloud = cloudModels.map((m) => ({
      id: m.id,
      name: m.name,
      api: m.api,
      provider: m.provider,
      baseUrl: null,
      maxTokens: m.maxOutputTokens ?? 8192,
      contextWindow: m.contextWindow ?? 128000,
    }))
    const local = llamaEndpoints
      .filter((e) => isRunning(e.status))
      .map((e) => ({ id: llamaModelId(e.id), name: e.name, api: 'openai-chat', provider: 'local', baseUrl: null, maxTokens: 8192, contextWindow: e.ctxSize }))
    const custom = customProviders.flatMap((cp) =>
      cp.models.map((m) => ({
        id: m.id,
        name: `${cp.name}: ${m.id}`,
        api: cp.apiFormat,
        provider: cp.id,
        baseUrl: cp.baseUrl,
        maxTokens: m.maxOutputTokens ?? 8192,
        contextWindow: m.contextWindow,
      })),
    )
    return [...cloud, ...local, ...custom]
  }, [cloudModels, llamaEndpoints, customProviders])
}

function ColorDot({ color, size = 18 }: { color: string | null; size?: number }) {
  const hex = color && color in SUBAGENT_COLOR_HEX ? SUBAGENT_COLOR_HEX[color as keyof typeof SUBAGENT_COLOR_HEX] : '#8a8a8a'
  return <Box sx={{ width: size, height: size, borderRadius: '50%', flexShrink: 0, bgcolor: hex }} />
}

function toolSummary(allowedTools: string[] | null): string {
  return allowedTools === null ? 'All tools' : `${allowedTools.length} tool${allowedTools.length === 1 ? '' : 's'}`
}

function BuiltinRow({
  profile,
  overrideModel,
  modelOptions,
  onModelChange,
}: {
  profile: SubagentProfile
  overrideModel: SubagentModelOverride | null
  modelOptions: SubagentModelOverride[]
  onModelChange: (model: SubagentModelOverride | null) => void
}) {
  return (
    <Stack direction="row" sx={{ alignItems: 'flex-start', gap: 1.5, p: 2, borderRadius: 2, border: 1, borderColor: 'divider' }}>
      <Box sx={{ position: 'relative', flexShrink: 0, mt: 0.25 }}>
        <UserOctagonLinear size={22} color="currentColor" />
        <Box sx={{ position: 'absolute', bottom: -2, right: -2 }}>
          <ColorDot color={profile.color} size={10} />
        </Box>
      </Box>
      <Stack sx={{ flex: 1, minWidth: 0, gap: 0.5 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, flexWrap: 'wrap' }}>
          <Typography variant="body2" sx={{ fontWeight: 700 }}>
            {profile.name}
          </Typography>
          <Chip size="small" label="Built-in" sx={{ fontSize: '0.625rem', height: 18 }} />
          <Chip size="small" label={toolSummary(profile.allowedTools)} sx={{ fontSize: '0.625rem', height: 18 }} />
        </Stack>
        <Typography variant="caption" color="text.secondary">
          {profile.description}
        </Typography>
      </Stack>
      <Select
        size="small"
        value={overrideModel?.id ?? '__inherit__'}
        onChange={(e) => onModelChange(e.target.value === '__inherit__' ? null : (modelOptions.find((m) => m.id === e.target.value) ?? null))}
        sx={{ minWidth: 160, fontSize: '0.75rem', flexShrink: 0 }}
      >
        <MenuItem value="__inherit__" sx={{ fontSize: '0.8125rem' }}>
          Inherit default
        </MenuItem>
        {modelOptions.map((m) => (
          <MenuItem key={`${m.provider}-${m.id}`} value={m.id} sx={{ fontSize: '0.8125rem' }}>
            {m.name}
          </MenuItem>
        ))}
      </Select>
    </Stack>
  )
}

function CustomRow({ profile, onEdit, onDelete }: { profile: SubagentProfile; onEdit: () => void; onDelete: () => void }) {
  const [busy, setBusy] = useState(false)
  const remove = async () => {
    setBusy(true)
    try {
      await onDelete()
    } finally {
      setBusy(false)
    }
  }
  return (
    <Stack direction="row" sx={{ alignItems: 'flex-start', gap: 1.5, p: 2, borderRadius: 2, border: 1, borderColor: 'divider', '&:hover': { bgcolor: 'action.hover' } }}>
      <Box sx={{ position: 'relative', flexShrink: 0, mt: 0.25 }}>
        <UserOctagonLinear size={22} color="currentColor" />
        <Box sx={{ position: 'absolute', bottom: -2, right: -2 }}>
          <ColorDot color={profile.color} size={10} />
        </Box>
      </Box>
      <Stack sx={{ flex: 1, minWidth: 0, gap: 0.5 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, flexWrap: 'wrap' }}>
          <Typography variant="body2" sx={{ fontWeight: 700 }}>
            {profile.name}
          </Typography>
          <Chip size="small" label={toolSummary(profile.allowedTools)} sx={{ fontSize: '0.625rem', height: 18 }} />
          {profile.model && <Chip size="small" label={profile.model.name} sx={{ fontSize: '0.625rem', height: 18 }} />}
        </Stack>
        <Typography variant="caption" color="text.secondary">
          {profile.description || 'No description.'}
        </Typography>
      </Stack>
      <IconButton size="small" onClick={onEdit}>
        <EditLinear size={14} color="currentColor" />
      </IconButton>
      <IconButton size="small" onClick={() => void remove()} disabled={busy}>
        {busy ? <CircularProgress size={14} /> : <TrashLinear size={14} color="currentColor" />}
      </IconButton>
    </Stack>
  )
}

function SubagentForm({ profile, presetName, onDone, onCancel }: { profile: SubagentProfile | null; presetName?: string; onDone: () => void; onCancel: () => void }) {
  const saveProfile = useSubagentProfileStore((s) => s.saveProfile)
  const modelOptions = useModelOptions()
  const [allTools, setAllTools] = useState<ToolInfo[]>([])

  useEffect(() => {
    void call<ToolInfo[]>('cmd_list_tool_names').then((t) => setAllTools(t ?? []))
  }, [])

  const [name, setName] = useState(profile?.name ?? presetName ?? '')
  const [color, setColor] = useState<string>(profile?.color ?? SUBAGENT_COLORS[0])
  const [modelId, setModelId] = useState<string>(profile?.model?.id ?? '__inherit__')
  const [description, setDescription] = useState(profile?.description ?? '')
  const [toolMode, setToolMode] = useState<'all' | 'custom'>(profile?.allowedTools === null || profile === null ? 'all' : 'custom')
  const [allowedTools, setAllowedTools] = useState<Set<string>>(new Set(profile?.allowedTools ?? []))
  const [systemPrompt, setSystemPrompt] = useState(profile?.systemPrompt ?? '')
  const [injectAgentsMd, setInjectAgentsMd] = useState(profile?.injectAgentsMd ?? true)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const submit = async () => {
    if (!name.trim()) return
    setSaving(true)
    setError(null)
    try {
      const model = modelId === '__inherit__' ? null : (modelOptions.find((m) => m.id === modelId) ?? null)
      await saveProfile({
        originalId: profile?.id ?? null,
        name: name.trim(),
        color,
        model,
        description: description.trim(),
        allowedTools: toolMode === 'all' ? null : Array.from(allowedTools),
        systemPrompt,
        injectAgentsMd,
      })
      onDone()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSaving(false)
    }
  }

  const toggleTool = (toolName: string) => {
    setAllowedTools((prev) => {
      const next = new Set(prev)
      if (next.has(toolName)) next.delete(toolName)
      else next.add(toolName)
      return next
    })
  }

  return (
    <Stack sx={{ gap: 2.5, maxWidth: 640 }}>
      <ButtonBase onClick={onCancel} sx={{ display: 'flex', alignItems: 'center', gap: 0.5, alignSelf: 'flex-start', color: 'text.secondary', fontSize: '0.8125rem' }}>
        <ArrowLeftLinear size={14} color="currentColor" />
        Back
      </ButtonBase>

      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
          {profile ? 'Edit subagent' : 'New subagent'}
        </Typography>
        <Typography variant="caption" color="text.secondary">
          Fill in the subagent name, tools, and system prompt, then save to return to the list.
        </Typography>
      </Stack>

      <Stack direction="row" sx={{ gap: 2 }}>
        <Stack sx={{ flex: 1, gap: 0.5 }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            Name
          </Typography>
          <TextField size="small" placeholder="code-reviewer" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
        </Stack>
        <Stack sx={{ gap: 0.5 }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            Color
          </Typography>
          <Stack direction="row" sx={{ gap: 0.5, alignItems: 'center', height: 40 }}>
            {SUBAGENT_COLORS.map((c) => (
              <ButtonBase
                key={c}
                onClick={() => setColor(c)}
                sx={{
                  width: 22,
                  height: 22,
                  borderRadius: '50%',
                  bgcolor: SUBAGENT_COLOR_HEX[c],
                  border: color === c ? '2px solid' : '2px solid transparent',
                  borderColor: color === c ? 'text.primary' : 'transparent',
                  outline: color === c ? '1px solid' : 'none',
                  outlineColor: SUBAGENT_COLOR_HEX[c],
                }}
              />
            ))}
          </Stack>
        </Stack>
        <Stack sx={{ width: 160, gap: 0.5 }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            Model
          </Typography>
          <Select size="small" value={modelId} onChange={(e) => setModelId(e.target.value)}>
            <MenuItem value="__inherit__">Inherit</MenuItem>
            {modelOptions.map((m) => (
              <MenuItem key={`${m.provider}-${m.id}`} value={m.id}>
                {m.name}
              </MenuItem>
            ))}
          </Select>
        </Stack>
      </Stack>

      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Description
        </Typography>
        <TextField size="small" placeholder="Short description shown to the model" value={description} onChange={(e) => setDescription(e.target.value)} />
      </Stack>

      <Stack sx={{ gap: 0.5 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            Allowed tools
          </Typography>
          <Select size="small" value={toolMode} onChange={(e) => setToolMode(e.target.value as 'all' | 'custom')} sx={{ fontSize: '0.8125rem' }}>
            <MenuItem value="all">Default all permissions</MenuItem>
            <MenuItem value="custom">Custom allowed tools</MenuItem>
          </Select>
          <Typography variant="caption" color="text.secondary">
            Control which tools this subagent can use.
          </Typography>
        </Stack>
        {toolMode === 'custom' && (
          <Box
            sx={{
              display: 'grid',
              gridTemplateColumns: 'repeat(3, 1fr)',
              gap: 0.5,
              p: 1.5,
              mt: 0.5,
              border: 1,
              borderColor: 'divider',
              borderRadius: 1.5,
            }}
          >
            {allTools.map((t) => (
              <Tooltip key={t.name} title={t.description ?? ''} placement="top">
                <Stack
                  direction="row"
                  onClick={() => toggleTool(t.name)}
                  sx={{ alignItems: 'center', gap: 0.25, cursor: 'pointer', borderRadius: 1, px: 0.5, '&:hover': { bgcolor: 'action.hover' } }}
                >
                  <Checkbox size="small" checked={allowedTools.has(t.name)} sx={{ p: 0.5 }} />
                  <Typography variant="body2" sx={{ fontSize: '0.8125rem' }}>
                    {t.name}
                  </Typography>
                </Stack>
              </Tooltip>
            ))}
          </Box>
        )}
      </Stack>

      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          System prompt
        </Typography>
        <TextField
          multiline
          minRows={3}
          placeholder="Describe this subagent's role and rules…"
          value={systemPrompt}
          onChange={(e) => setSystemPrompt(e.target.value)}
        />
      </Stack>

      <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', p: 1.5, borderRadius: 1.5, bgcolor: 'action.hover' }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Inject AGENTS.md
        </Typography>
        <Switch size="small" checked={injectAgentsMd} onChange={(e) => setInjectAgentsMd(e.target.checked)} />
      </Stack>

      {error && (
        <Typography variant="caption" color="error.main">
          {error}
        </Typography>
      )}

      <Stack direction="row" sx={{ gap: 1 }}>
        <Button
          variant="contained"
          disableElevation
          onClick={() => void submit()}
          disabled={saving || !name.trim()}
          startIcon={saving ? <CircularProgress size={14} /> : undefined}
        >
          Save
        </Button>
        <Button color="inherit" onClick={onCancel}>
          Cancel
        </Button>
      </Stack>
    </Stack>
  )
}

export function SubagentsSection() {
  const builtin = useSubagentProfileStore((s) => s.builtin)
  const custom = useSubagentProfileStore((s) => s.custom)
  const refresh = useSubagentProfileStore((s) => s.refresh)
  const saveProfile = useSubagentProfileStore((s) => s.saveProfile)
  const removeProfile = useSubagentProfileStore((s) => s.removeProfile)
  const modelOptions = useModelOptions()
  const [query, setQuery] = useState('')
  const [view, setView] = useState<{ mode: 'list' } | { mode: 'new' } | { mode: 'edit'; profile: SubagentProfile }>({ mode: 'list' })

  useEffect(() => {
    void refresh()
  }, [refresh])

  const builtinNames = useMemo(() => new Set(builtin.map((b) => b.name)), [builtin])
  const shadowByName = useMemo(() => new Map(custom.filter((c) => builtinNames.has(c.name)).map((c) => [c.name, c])), [custom, builtinNames]);
  const standaloneCustom = useMemo(() => custom.filter((c) => !builtinNames.has(c.name)), [custom, builtinNames])

  const q = query.trim().toLowerCase()
  const filteredBuiltin = q ? builtin.filter((b) => b.name.toLowerCase().includes(q) || b.description.toLowerCase().includes(q)) : builtin
  const filteredCustom = q ? standaloneCustom.filter((c) => c.name.toLowerCase().includes(q) || c.description.toLowerCase().includes(q)) : standaloneCustom

  const setBuiltinModel = async (b: SubagentProfile, model: SubagentModelOverride | null) => {
    const shadow = shadowByName.get(b.name)
    if (model === null) {
      // Reverting to "Inherit default" — drop the shadow file if one
      // exists so resolution falls back to the true built-in again.
      if (shadow) await removeProfile(shadow.id)
      return
    }
    await saveProfile({
      originalId: shadow?.id ?? null,
      name: b.name,
      color: b.color,
      model,
      description: b.description,
      allowedTools: b.allowedTools,
      systemPrompt: '',
      injectAgentsMd: true,
    })
  }

  if (view.mode !== 'list') {
    return (
      <SubagentForm
        profile={view.mode === 'edit' ? view.profile : null}
        onDone={() => setView({ mode: 'list' })}
        onCancel={() => setView({ mode: 'list' })}
      />
    )
  }

  return (
    <Stack sx={{ gap: 2.5 }}>
      <Stack direction="row" sx={{ justifyContent: 'space-between', alignItems: 'flex-start' }}>
        <Stack sx={{ gap: 0.5 }}>
          <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
            Subagents
          </Typography>
          <Typography variant="caption" color="text.secondary">
            Manage subagent profiles the task tool can spawn — a name, model, tool permissions, and system prompt.
          </Typography>
        </Stack>
        <Stack direction="row" sx={{ gap: 0.5 }}>
          <Tooltip title="New subagent">
            <IconButton size="small" onClick={() => setView({ mode: 'new' })}>
              <AddLinear size={16} color="currentColor" />
            </IconButton>
          </Tooltip>
          <Tooltip title="Refresh">
            <IconButton size="small" onClick={() => void refresh()}>
              <RefreshLinear size={15} color="currentColor" />
            </IconButton>
          </Tooltip>
        </Stack>
      </Stack>

      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 1.25, py: 0.5, border: 1, borderColor: 'divider', borderRadius: 1.5 }}>
        <SearchNormalLinear size={14} color="currentColor" />
        <TextField
          variant="standard"
          fullWidth
          placeholder="Search subagents…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          slotProps={{ input: { disableUnderline: true } }}
          sx={{ '& input': { fontSize: '0.8125rem' } }}
        />
      </Stack>

      <Stack sx={{ gap: 1 }}>
        <Stack direction="row" sx={{ alignItems: 'baseline', gap: 1 }}>
          <Typography variant="body2" sx={{ fontWeight: 700 }}>
            Built-in subagents
          </Typography>
          <Typography variant="caption" color="text.secondary">
            {filteredBuiltin.length} items
          </Typography>
        </Stack>
        <Typography variant="caption" color="text.secondary">
          Built-in profiles are runtime defaults and cannot be edited here — pick a model override per-agent instead.
        </Typography>
        {filteredBuiltin.map((b) => (
          <BuiltinRow key={b.id} profile={b} overrideModel={shadowByName.get(b.name)?.model ?? null} modelOptions={modelOptions} onModelChange={(m) => void setBuiltinModel(b, m)} />
        ))}
      </Stack>

      <Stack sx={{ gap: 1 }}>
        <Stack direction="row" sx={{ alignItems: 'baseline', gap: 1 }}>
          <Typography variant="body2" sx={{ fontWeight: 700 }}>
            Custom subagents
          </Typography>
          <Typography variant="caption" color="text.secondary">
            {filteredCustom.length} items
          </Typography>
        </Stack>
        {filteredCustom.length === 0 ? (
          <Typography variant="caption" color="text.secondary">
            No custom subagents yet.
          </Typography>
        ) : (
          filteredCustom.map((c) => (
            <CustomRow key={c.id} profile={c} onEdit={() => setView({ mode: 'edit', profile: c })} onDelete={() => removeProfile(c.id)} />
          ))
        )}
      </Stack>
    </Stack>
  )
}
