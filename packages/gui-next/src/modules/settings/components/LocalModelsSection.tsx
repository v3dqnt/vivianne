import { useEffect, useState } from 'react'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import Divider from '@mui/material/Divider'
import Button from '@mui/material/Button'
import IconButton from '@mui/material/IconButton'
import CircularProgress from '@mui/material/CircularProgress'
import Dialog from '@mui/material/Dialog'
import DialogTitle from '@mui/material/DialogTitle'
import DialogContent from '@mui/material/DialogContent'
import DialogActions from '@mui/material/DialogActions'
import TextField from '@mui/material/TextField'
import Select from '@mui/material/Select'
import MenuItem from '@mui/material/MenuItem'
import InputLabel from '@mui/material/InputLabel'
import FormControl from '@mui/material/FormControl'
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

  useEffect(() => {
    if (!open) return
    setName(endpoint?.name ?? defaultName)
    setCtxSize(endpoint?.ctxSize ?? DEFAULT_CTX_SIZE)
    setNgl(endpoint?.ngl ?? DEFAULT_NGL)
    setEngine(endpoint?.engine ?? 'llamaCpp')
  }, [open, endpoint, defaultName])

  const running = endpoint ? isRunning(endpoint.status) : false

  const submit = async () => {
    setSaving(true)
    try {
      if (endpoint) {
        await updateEndpoint(endpoint.id, { name: name.trim() || endpoint.name, ctxSize, ngl, engine })
      } else if (modelPath) {
        await addEndpoint(modelPath, name.trim() || fileName(modelPath).replace(/\.gguf$/i, ''), ctxSize, ngl, engine)
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
            helperText="Vivianne's agent prompt alone is ~25k tokens — keep this ≥ 32768 for agent use. Larger = more VRAM/RAM for the KV cache."
          />
          <TextField
            label="GPU layers (ngl)"
            size="small"
            type="number"
            value={ngl}
            onChange={(e) => setNgl(Math.max(0, Number(e.target.value) || 0))}
            helperText="999 = offload the whole model to GPU. 0 = CPU only (frees VRAM, much slower)."
          />
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
