import { useMemo, useState } from 'react'
import { Stack, Typography, Button, Dialog, DialogTitle, DialogContent, DialogActions, TextField, ButtonBase, Box, Alert } from '../../../ui'
import { PRESETS, SLOT_LABELS, idFromName, type Preset, type PresetSlot } from '../presets'
import { useModsStore } from '../store/modsStore'

const SLOTS: PresetSlot[] = ['panel', 'chat', 'sidebar', 'tool']

/** Describe what a permission allows, the same words the consent dialog uses. */
export const PERMISSION_WORDS: Record<string, string> = {
  storage: 'Keep its own data (up to 1 MB)',
  notify: 'Show notifications',
  'chat.read': 'Read the conversation you have open',
  'chat.compose': 'Put text in your message box',
  'chat.send': 'Send messages to Vivianne as you',
  'agent.status': 'See what your agents are doing',
  'files.read': 'Read files in your workspace',
  'files.write': 'Change files in your workspace',
  open: 'Open links and panels',
  shell: 'Give Vivianne tools that run commands on your computer',
}

/**
 * "New mod": pick a slot and a preset, name it, and the mod is written to
 * `~/.viv/mods/<id>/`. It starts off; turning it on asks for its
 * permissions like any other mod.
 */
export function NewModDialog({ open, onClose, onCreated }: { open: boolean; onClose: () => void; onCreated: (key: string) => void }) {
  const create = useModsStore((s) => s.create)
  const [slot, setSlot] = useState<PresetSlot>('panel')
  const [preset, setPreset] = useState<Preset>(PRESETS[0])
  const [name, setName] = useState('')
  const [fence, setFence] = useState('')
  const [url, setUrl] = useState('http://localhost:3000')
  const [toolName, setToolName] = useState('')
  const [toolDescription, setToolDescription] = useState('')
  const [command, setCommand] = useState('')
  const [params, setParams] = useState('{\n  "type": "object",\n  "properties": {}\n}')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const choices = useMemo(() => PRESETS.filter((p) => p.slot === slot), [slot])
  const id = idFromName(name)
  const fenceValue = fence || preset.fence || ''

  const pickSlot = (s: PresetSlot) => {
    setSlot(s)
    setPreset(PRESETS.find((p) => p.slot === s)!)
    setFence('')
    setError(null)
  }

  const submit = async () => {
    setError(null)
    if (id.length < 2) return setError('Give it a name (2+ letters or digits).')
    let parameters: Record<string, unknown> | undefined
    if (preset.needsTool) {
      if (!/^[A-Za-z_][A-Za-z0-9_]{1,63}$/.test(toolName)) return setError('The tool name is letters, digits and _ (like count_todos).')
      if (!toolDescription.trim() || !command.trim()) return setError('The tool needs a description and a command.')
      try {
        parameters = JSON.parse(params)
      } catch {
        return setError('Parameters must be valid JSON (a JSON Schema object).')
      }
    }
    if (preset.slot === 'chat' && !/^[a-z][a-z0-9-]{1,29}$/.test(fenceValue)) {
      return setError('The fence is 2-30 lowercase letters, digits or -, starting with a letter.')
    }
    setBusy(true)
    try {
      const files = preset.files({
        id,
        name: name.trim(),
        fence: fenceValue,
        url,
        tool: parameters ? { name: toolName, description: toolDescription.trim(), command, parameters } : undefined,
      })
      const key = await create(files)
      onCreated(key)
      setName('')
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle>New mod</DialogTitle>
      <DialogContent>
        <Stack sx={{ gap: 1.5, pt: 0.5 }}>
          <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap' }}>
            {SLOTS.map((s) => (
              <Button key={s} size="small" variant={slot === s ? 'contained' : 'outlined'} onClick={() => pickSlot(s)} sx={{ textTransform: 'none' }}>
                {SLOT_LABELS[s]}
              </Button>
            ))}
          </Stack>

          <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(180px, 1fr))', gap: 1 }}>
            {choices.map((p) => (
              <ButtonBase
                key={p.id}
                onClick={() => setPreset(p)}
                sx={{
                  display: 'block',
                  textAlign: 'left',
                  p: 1.25,
                  borderRadius: 2,
                  border: 1,
                  borderColor: preset.id === p.id ? 'primary.main' : 'divider',
                  bgcolor: preset.id === p.id ? 'action.selected' : 'transparent',
                }}
              >
                <Typography variant="body2" sx={{ fontWeight: 600 }}>
                  {p.label}
                </Typography>
                <Typography variant="caption" color="text.secondary">
                  {p.description}
                </Typography>
              </ButtonBase>
            ))}
          </Box>

          <TextField
            size="small"
            label="Name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={preset.label}
            helperText={id ? `Folder: ~/.viv/mods/${id}` : ' '}
            autoFocus
          />
          {preset.slot === 'chat' && (
            <TextField
              size="small"
              label="Fence"
              value={fenceValue}
              onChange={(e) => setFence(e.target.value)}
              helperText={`Vivianne writes a \`\`\`${fenceValue || 'fence'} block and this mod renders it.`}
            />
          )}
          {preset.needsUrl && (
            <TextField size="small" label="Local URL" value={url} onChange={(e) => setUrl(e.target.value)} helperText="http://localhost:<port> or http://127.0.0.1:<port>" />
          )}
          {preset.needsTool && (
            <>
              <TextField size="small" label="Tool name" value={toolName} onChange={(e) => setToolName(e.target.value)} placeholder="count_todos" />
              <TextField size="small" label="What it does (Vivianne reads this)" value={toolDescription} onChange={(e) => setToolDescription(e.target.value)} />
              <TextField
                size="small"
                label="Command (bash)"
                value={command}
                onChange={(e) => setCommand(e.target.value)}
                placeholder="grep -c TODO {{path}}"
                helperText="{{name}} is replaced by the argument of that name."
              />
              <TextField size="small" label="Parameters (JSON Schema)" value={params} onChange={(e) => setParams(e.target.value)} multiline minRows={4} />
            </>
          )}

          <Typography variant="caption" color="text.secondary">
            {preset.permissions.length === 0
              ? 'Needs no permissions.'
              : `Will ask to: ${preset.permissions.map((p) => PERMISSION_WORDS[p] ?? p).join('; ')}.`}{' '}
            It starts off. Turn it on to run it, and edit its files any time: open pages reload when you save.
          </Typography>
          {error && <Alert severity="error">{error}</Alert>}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} sx={{ textTransform: 'none' }}>
          Cancel
        </Button>
        <Button variant="contained" onClick={() => void submit()} disabled={busy} sx={{ textTransform: 'none' }}>
          Create
        </Button>
      </DialogActions>
    </Dialog>
  )
}
