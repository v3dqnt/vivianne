import { useEffect, useState } from 'react'
import { Stack, Typography, Divider, Button, IconButton, CircularProgress, Dialog, DialogTitle, DialogContent, DialogActions, TextField, Select, MenuItem, InputLabel, FormControl, FormControlLabel, Checkbox, Collapse, Box } from '../../../ui'
import { ImportLinear, PlayCircleLinear, StopCircleLinear, TrashLinear, CpuLinear, SettingLinear } from '../../../icons'
import { useLlamaStore, DEFAULT_CTX_SIZE, DEFAULT_NGL } from '../store/llamaStore'
import { statusLabel, isRunning, isBusyStatus, engineLabel, type LlamaEndpoint, type LocalEngine } from '../lib/llama'

function fileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() || path
}

/** Shared add/edit form. For add, `endpoint` is null and `modelPath` is the
 * freshly-picked file; for edit, `endpoint` carries the existing config. */
function EndpointDialog({
  open,
  onClose,
  endpoint,
  modelPath,
  defaultName,
}: {
  open: boolean
  onClose: () => void
  endpoint: LlamaEndpoint | null
  modelPath: string | null
  defaultName: string
}) {
  const addEndpoint = useLlamaStore((s) => s.addEndpoint)
  const updateEndpoint = useLlamaStore((s) => s.updateEndpoint)
  const [name, setName] = useState('')
  const [ctxSize, setCtxSize] = useState(DEFAULT_CTX_SIZE)
  const [ngl, setNgl] = useState(DEFAULT_NGL)
  const [engine, setEngine] = useState<LocalEngine>('llamaCpp')
  const [saving, setSaving] = useState(false)
  // Tuning is collapsed by default: the defaults are correct on a normal
  // machine (llama.cpp's own device auto-selection was measured picking the
  // discrete GPU here), so these are for the cases where they are not.
  const [showAdvanced, setShowAdvanced] = useState(false)
  const [kvCacheType, setKvCacheType] = useState('')
  const [threads, setThreads] = useState('')
  const [device, setDevice] = useState('')
  const [cacheReuse, setCacheReuse] = useState('')
  const [speculative, setSpeculative] = useState('')
  const [cacheRamMib, setCacheRamMib] = useState('')
  const [sleepIdleSeconds, setSleepIdleSeconds] = useState('')
  const [mlock, setMlock] = useState(false)
  const [extraArgs, setExtraArgs] = useState('')

  useEffect(() => {
    if (!open) return
    setName(endpoint?.name ?? defaultName)
    setCtxSize(endpoint?.ctxSize ?? DEFAULT_CTX_SIZE)
    setNgl(endpoint?.ngl ?? DEFAULT_NGL)
    setEngine(endpoint?.engine ?? 'llamaCpp')
    const t = endpoint?.tuning
    setKvCacheType(t?.kvCacheType ?? '')
    setThreads(t?.threads != null ? String(t.threads) : '')
    setDevice(t?.device ?? '')
    setCacheReuse(t?.cacheReuse != null ? String(t.cacheReuse) : '')
    setSpeculative(t?.speculative ?? '')
    setCacheRamMib(t?.cacheRamMib != null ? String(t.cacheRamMib) : '')
    setSleepIdleSeconds(t?.sleepIdleSeconds != null ? String(t.sleepIdleSeconds) : '')
    setMlock(t?.mlock ?? false)
    setExtraArgs(t?.extraArgs ?? '')
    // Open the section when this endpoint already has something set, so a
    // non-default tuning is never hidden behind a collapsed header.
    setShowAdvanced(
      !!(
        t &&
        (t.kvCacheType ||
          t.threads ||
          t.device ||
          t.cacheReuse != null ||
          t.speculative ||
          t.cacheRamMib != null ||
          t.sleepIdleSeconds ||
          t.mlock ||
          t.extraArgs)
      ),
    )
  }, [open, endpoint, defaultName])

  const running = endpoint ? isRunning(endpoint.status) : false

  const submit = async () => {
    setSaving(true)
    try {
      // Empty string means "unset" for every field here, which is what makes
      // the engine fall back to its own default rather than to a value the UI
      // invented.
      const num = (v: string) => (v.trim() === '' ? null : Math.max(0, Number(v) || 0))
      const str = (v: string) => (v.trim() === '' ? null : v.trim())
      const tuning = {
        kvCacheType: str(kvCacheType),
        threads: num(threads),
        device: str(device),
        cacheReuse: num(cacheReuse),
        speculative: str(speculative),
        // -1 (unlimited) is a real value here, so not `num`, which clamps at 0.
        cacheRamMib: cacheRamMib.trim() === '' ? null : Math.max(-1, Math.trunc(Number(cacheRamMib) || 0)),
        sleepIdleSeconds: num(sleepIdleSeconds),
        mlock,
        extraArgs: str(extraArgs),
      }
      if (endpoint) {
        await updateEndpoint(endpoint.id, { name: name.trim() || endpoint.name, ctxSize, ngl, engine, tuning })
      } else if (modelPath) {
        await addEndpoint(modelPath, name.trim() || fileName(modelPath).replace(/\.gguf$/i, ''), ctxSize, ngl, engine, tuning)
      }
      onClose()
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open={open} onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle>{endpoint ? 'Edit local model' : 'Add local model'}</DialogTitle>
      <DialogContent>
        <Stack spacing={2} sx={{ pt: 0.5 }}>
          <TextField label="Name" size="small" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
          <FormControl size="small" fullWidth>
            <InputLabel id="local-engine-label">Engine</InputLabel>
            <Select
              labelId="local-engine-label"
              label="Engine"
              value={engine}
              onChange={(e) => setEngine(e.target.value as LocalEngine)}
            >
              <MenuItem value="llamaCpp">llama.cpp</MenuItem>
              <MenuItem value="mistralRs">mistral.rs</MenuItem>
            </Select>
            <Typography variant="caption" color="text.secondary" sx={{ mt: 0.5 }}>
              {engine === 'mistralRs'
                ? 'Pure-Rust engine, CPU or GPU. Requires `mistralrs` on your PATH.'
                : 'The established default. Requires `llama-server` on your PATH.'}
            </Typography>
          </FormControl>
          <TextField
            label="Context size (tokens)"
            size="small"
            type="number"
            value={ctxSize}
            onChange={(e) => setCtxSize(Math.max(512, Number(e.target.value) || 0))}
            helperText="Local models get a lean tool set, so the agent's first request is ~6–8k tokens — 32768 leaves room for real work. Larger = more VRAM/RAM for the KV cache."
          />
          <TextField
            label="GPU layers (ngl)"
            size="small"
            type="number"
            value={ngl}
            onChange={(e) => setNgl(Math.max(0, Number(e.target.value) || 0))}
            helperText="999 = as much as fits: llama.cpp measures free VRAM and places layers itself (MoE experts go to the CPU first). A lower number pins exactly that many. 0 = CPU only."
          />
          {/* llama.cpp only — mistral.rs takes none of these flags. */}
          {engine === 'llamaCpp' && (
            <Box>
              <Button
                size="small"
                onClick={() => setShowAdvanced((v) => !v)}
                sx={{ textTransform: 'none', px: 0.5, color: 'text.secondary', fontWeight: 600 }}
              >
                {showAdvanced ? 'Hide' : 'Show'} performance options
              </Button>
              <Collapse in={showAdvanced}>
                <Stack spacing={2} sx={{ pt: 1.5 }}>
                  <FormControl size="small" fullWidth>
                    <InputLabel id="kv-cache-label">KV cache type</InputLabel>
                    <Select
                      labelId="kv-cache-label"
                      label="KV cache type"
                      value={kvCacheType}
                      onChange={(e) => setKvCacheType(e.target.value)}
                    >
                      <MenuItem value="">q8_0 — half the memory (default)</MenuItem>
                      <MenuItem value="f16">f16 — full precision</MenuItem>
                      <MenuItem value="q4_0">q4_0 — quarter the memory</MenuItem>
                    </Select>
                    <Typography variant="caption" color="text.secondary" sx={{ mt: 0.5 }}>
                      The KV cache, not the model, is what usually decides how much context fits. q8_0 measured
                      identical throughput here while halving it, so it buys context for free. q4_0 costs some
                      accuracy on long contexts.
                    </Typography>
                  </FormControl>
                  <TextField
                    label="Device"
                    size="small"
                    value={device}
                    onChange={(e) => setDevice(e.target.value)}
                    placeholder="auto"
                    helperText={'Blank auto-selects, which is usually right. Set it (e.g. "Vulkan1") if the wrong GPU is picked — run `llama-server --list-devices` to see the names.'}
                  />
                  <TextField
                    label="CPU threads"
                    size="small"
                    type="number"
                    value={threads}
                    onChange={(e) => setThreads(e.target.value)}
                    placeholder="auto"
                    helperText="Blank lets llama.cpp decide. On a hybrid P+E-core CPU its default can over-subscribe; the physical performance-core count is often faster."
                  />
                  <TextField
                    label="Cache reuse (tokens)"
                    size="small"
                    type="number"
                    value={cacheReuse}
                    onChange={(e) => setCacheReuse(e.target.value)}
                    placeholder="256"
                    helperText="Lets the server salvage KV cache when the prompt changes in the middle rather than only at the end — which is what compacting a conversation does. 0 turns it off."
                  />
                  <FormControl size="small" fullWidth>
                    <InputLabel id="spec-label">Speculative decoding</InputLabel>
                    <Select
                      labelId="spec-label"
                      label="Speculative decoding"
                      value={speculative}
                      onChange={(e) => setSpeculative(e.target.value)}
                    >
                      <MenuItem value="">n-gram (default)</MenuItem>
                      <MenuItem value="none">Off</MenuItem>
                    </Select>
                    <Typography variant="caption" color="text.secondary" sx={{ mt: 0.5 }}>
                      Drafts tokens from text already in the conversation and verifies them in one pass. Output is
                      identical; code the agent is rewriting streams up to ~2× faster. Costs ~16 MB, no extra model.
                    </Typography>
                  </FormControl>
                  <TextField
                    label="Prompt cache RAM (MiB)"
                    size="small"
                    type="number"
                    value={cacheRamMib}
                    onChange={(e) => setCacheRamMib(e.target.value)}
                    placeholder="2048"
                    helperText="System RAM the server may keep earlier conversations' prompts in, for instant resumes. llama.cpp's own default is 8192. 0 disables it, -1 is unlimited."
                  />
                  <TextField
                    label="Unload when idle (seconds)"
                    size="small"
                    type="number"
                    value={sleepIdleSeconds}
                    onChange={(e) => setSleepIdleSeconds(e.target.value)}
                    placeholder="never"
                    helperText="Frees all of the model's VRAM and RAM after this long without a request; the next message reloads it (a few seconds). 600 is a good value on a shared machine."
                  />
                  <FormControlLabel
                    control={<Checkbox size="small" checked={mlock} onChange={(e) => setMlock(e.target.checked)} />}
                    label={<Typography variant="body2">Lock weights in RAM (--mlock)</Typography>}
                  />
                  <TextField
                    label="Extra flags"
                    size="small"
                    value={extraArgs}
                    onChange={(e) => setExtraArgs(e.target.value)}
                    placeholder="--no-context-shift"
                    helperText="Passed to llama-server verbatim, split on spaces. No shell is involved, so quoting does nothing."
                  />
                </Stack>
              </Collapse>
            </Box>
          )}
          {endpoint && running && (
            <Typography variant="caption" color="warning.main">
              This model is running — saving will stop it. Start it again to apply the new settings.
            </Typography>
          )}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} color="inherit">
          Cancel
        </Button>
        <Button onClick={() => void submit()} variant="contained" disabled={saving} startIcon={saving ? <CircularProgress size={14} /> : undefined}>
          {endpoint ? 'Save' : 'Add'}
        </Button>
      </DialogActions>
    </Dialog>
  )
}

function EndpointRow({ endpoint, onEdit }: { endpoint: LlamaEndpoint; onEdit: () => void }) {
  const start = useLlamaStore((s) => s.start)
  const stop = useLlamaStore((s) => s.stop)
  const removeEndpoint = useLlamaStore((s) => s.removeEndpoint)
  const [busy, setBusy] = useState(false)

  const running = isRunning(endpoint.status)
  const busyStatus = isBusyStatus(endpoint.status)

  const toggle = async () => {
    setBusy(true)
    try {
      await (running ? stop(endpoint.id) : start(endpoint.id))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5, py: 1.25, px: 1.5, borderRadius: 2, '&:hover': { bgcolor: 'action.hover' } }}>
      <CpuLinear size={16} color="currentColor" />
      <Stack sx={{ flex: 1, minWidth: 0 }}>
        <Typography variant="body2" sx={{ fontWeight: 500 }} noWrap>
          {endpoint.name}
        </Typography>
        <Typography variant="caption" color="text.secondary" noWrap title={endpoint.modelPath}>
          {engineLabel(endpoint.engine)} · {fileName(endpoint.modelPath)} · port {endpoint.port} · {(endpoint.ctxSize / 1024).toFixed(0)}k ctx
        </Typography>
      </Stack>
      <Typography
        variant="caption"
        sx={{ color: running ? 'success.main' : typeof endpoint.status === 'object' ? 'error.main' : 'text.secondary' }}
      >
        {statusLabel(endpoint.status)}
      </Typography>
      <IconButton size="small" onClick={onEdit} disabled={busy} title="Edit context size / GPU layers">
        <SettingLinear size={15} color="currentColor" />
      </IconButton>
      <IconButton size="small" onClick={() => void toggle()} disabled={busy || busyStatus}>
        {busy || busyStatus ? <CircularProgress size={14} /> : running ? <StopCircleLinear size={16} color="currentColor" /> : <PlayCircleLinear size={16} color="currentColor" />}
      </IconButton>
      <IconButton size="small" onClick={() => void removeEndpoint(endpoint.id)} disabled={busy}>
        <TrashLinear size={15} color="currentColor" />
      </IconButton>
    </Stack>
  )
}

export function LocalModelsSection() {
  const endpoints = useLlamaStore((s) => s.endpoints)
  const refresh = useLlamaStore((s) => s.refresh)
  const pickGgufFile = useLlamaStore((s) => s.pickGgufFile)
  const [dialog, setDialog] = useState<{ endpoint: LlamaEndpoint | null; modelPath: string | null } | null>(null)

  useEffect(() => {
    void refresh()
  }, [refresh])

  const startAdd = async () => {
    const path = await pickGgufFile()
    if (!path) return
    setDialog({ endpoint: null, modelPath: path })
  }

  return (
    <Stack sx={{ gap: 0.5 }}>
      <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', mb: 0.5 }}>
        <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
          Local models
        </Typography>
        <Button
          size="small"
          onClick={() => void startAdd()}
          startIcon={<ImportLinear size={14} color="currentColor" />}
          sx={{ textTransform: 'none' }}
        >
          Add GGUF model
        </Button>
      </Stack>
      <Typography variant="caption" color="text.secondary" sx={{ mb: 1 }}>
        Runs a local server process against a GGUF file on disk — llama.cpp (github.com/ggml-org/llama.cpp) or mistral.rs
        (github.com/EricLBuehler/mistral.rs), picked per model. Requires the matching binary on your PATH.
      </Typography>
      <Divider sx={{ mb: 0.5 }} />
      {endpoints.length === 0 ? (
        <Typography variant="caption" color="text.secondary" sx={{ py: 1 }}>
          No local models added yet.
        </Typography>
      ) : (
        endpoints.map((e) => <EndpointRow key={e.id} endpoint={e} onEdit={() => setDialog({ endpoint: e, modelPath: null })} />)
      )}
      <EndpointDialog
        open={dialog !== null}
        onClose={() => setDialog(null)}
        endpoint={dialog?.endpoint ?? null}
        modelPath={dialog?.modelPath ?? null}
        defaultName={dialog?.modelPath ? fileName(dialog.modelPath).replace(/\.gguf$/i, '') : ''}
      />
    </Stack>
  )
}
