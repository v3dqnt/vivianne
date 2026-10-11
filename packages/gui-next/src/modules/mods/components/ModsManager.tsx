import { useEffect, useState } from 'react'
import {
  Stack,
  Typography,
  Button,
  Switch,
  Chip,
  Alert,
  Collapse,
  ButtonBase,
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  IconButton,
  Tooltip,
  Box,
} from '../../../ui'
import { AddLinear, FolderOpenLinear, TrashLinear, LayerLinear, MagicpenLinear } from '../../../icons'
import { call } from '../../../ipc'
import { useModsStore } from '../store/modsStore'
import { useAgentStore } from '../../ai/store/agentStore'
import { NewModDialog } from './NewModDialog'
import type { ModInfo, ModStatus } from '../lib/types'

const STATUS: Record<ModStatus, { label: string; color: 'default' | 'success' | 'warning' | 'error' }> = {
  on: { label: 'On', color: 'success' },
  off: { label: 'Off', color: 'default' },
  needsApproval: { label: 'New', color: 'warning' },
  permissionsChanged: { label: 'Asks for more', color: 'warning' },
  untrusted: { label: 'Project not trusted', color: 'default' },
  invalid: { label: 'Broken', color: 'error' },
}

function summary(m: ModInfo): string {
  const c = m.manifest?.contributes
  if (!c) return ''
  const parts = [
    c.panels.length && `${c.panels.length} panel${c.panels.length > 1 ? 's' : ''}`,
    c.sidebar.length && `${c.sidebar.length} sidebar section${c.sidebar.length > 1 ? 's' : ''}`,
    c.chat.length && `chat: ${c.chat.map((x) => '```' + x.fence).join(', ')}`,
    c.tools.length && `tool${c.tools.length > 1 ? 's' : ''}: ${c.tools.map((t) => t.name).join(', ')}`,
    c.themes.length && `${c.themes.length} theme${c.themes.length > 1 ? 's' : ''}`,
  ].filter(Boolean)
  return parts.join(' · ')
}

/** Approve a mod's permissions before it runs. */
function ConsentDialog({ mod, onClose }: { mod: ModInfo | null; onClose: () => void }) {
  const setEnabled = useModsStore((s) => s.setEnabled)
  const [busy, setBusy] = useState(false)
  if (!mod) return null
  const changed = mod.status === 'permissionsChanged'
  return (
    <Dialog open onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle>
        {changed ? 'Allow more for' : 'Turn on'} {mod.manifest?.name ?? mod.key}?
      </DialogTitle>
      <DialogContent>
        <Stack sx={{ gap: 1 }}>
          {mod.manifest?.description && (
            <Typography variant="body2" color="text.secondary">
              {mod.manifest.description}
            </Typography>
          )}
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            {mod.permissions.length ? 'It will be able to:' : 'It asks for no permissions.'}
          </Typography>
          {mod.permissions.map(([p, words]) => (
            <Typography key={p} variant="body2" sx={{ pl: 1 }} color={p === 'shell' || p === 'chat.send' || p === 'files.write' ? 'warning.main' : undefined}>
              • {words}
            </Typography>
          ))}
          <Typography variant="caption" color="text.secondary">
            It runs sandboxed and can do nothing beyond this. If it later asks for more, it stays off until you agree again.
            {mod.scope === 'project' ? ' This mod comes with the project folder.' : ''}
          </Typography>
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} sx={{ textTransform: 'none' }}>
          Not now
        </Button>
        <Button
          variant="contained"
          disabled={busy}
          onClick={async () => {
            setBusy(true)
            try {
              await setEnabled(mod.key, true)
              onClose()
            } finally {
              setBusy(false)
            }
          }}
          sx={{ textTransform: 'none' }}
        >
          Turn on
        </Button>
      </DialogActions>
    </Dialog>
  )
}

function ModRow({ mod, onConsent }: { mod: ModInfo; onConsent: (m: ModInfo) => void }) {
  const setEnabled = useModsStore((s) => s.setEnabled)
  const remove = useModsStore((s) => s.remove)
  const [confirmRemove, setConfirmRemove] = useState(false)
  const s = STATUS[mod.status]
  const canToggle = mod.status !== 'invalid' && mod.status !== 'untrusted'
  const on = mod.status === 'on'

  const toggle = () => {
    if (on) void setEnabled(mod.key, false)
    else if (mod.status === 'off') void setEnabled(mod.key, true)
    else onConsent(mod)
  }

  return (
    <Stack direction="row" sx={{ gap: 1.5, alignItems: 'flex-start', px: 1.5, py: 1.25, borderRadius: 2, border: 1, borderColor: 'divider' }}>
      <Box sx={{ pt: 0.25, color: 'text.secondary' }}>
        <LayerLinear size={16} color="currentColor" />
      </Box>
      <Stack sx={{ flex: 1, minWidth: 0, gap: 0.25 }}>
        <Stack direction="row" sx={{ gap: 0.75, alignItems: 'center', flexWrap: 'wrap' }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            {mod.manifest?.name ?? mod.key}
          </Typography>
          {mod.manifest?.version && (
            <Typography variant="caption" color="text.secondary">
              {mod.manifest.version}
            </Typography>
          )}
          <Chip size="small" label={s.label} color={s.color} variant="outlined" />
          {mod.scope === 'project' && <Chip size="small" label="This project" variant="outlined" />}
        </Stack>
        {mod.manifest?.description && (
          <Typography variant="caption" color="text.secondary">
            {mod.manifest.description}
          </Typography>
        )}
        <Typography variant="caption" color="text.secondary" sx={{ fontFamily: 'monospace', fontSize: '0.6875rem' }}>
          {summary(mod)}
        </Typography>
        {mod.error && (
          <Typography variant="caption" color="error">
            {mod.error}
          </Typography>
        )}
        {mod.status === 'permissionsChanged' && (
          <Typography variant="caption" color="warning.main">
            It now asks for permissions you haven't given it. Turn it on to review them.
          </Typography>
        )}
      </Stack>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.25, flexShrink: 0 }}>
        <Tooltip title="Open its folder">
          <IconButton size="small" onClick={() => void call('cmd_open_external', { path: mod.dir, target: 'explorer' })}>
            <FolderOpenLinear size={14} color="currentColor" />
          </IconButton>
        </Tooltip>
        {mod.scope === 'user' && (
          <Tooltip title={confirmRemove ? 'Click again to delete it and its data' : 'Delete'}>
            <IconButton
              size="small"
              color={confirmRemove ? 'error' : 'default'}
              onBlur={() => setConfirmRemove(false)}
              onClick={() => (confirmRemove ? void remove(mod.key) : setConfirmRemove(true))}
            >
              <TrashLinear size={14} color="currentColor" />
            </IconButton>
          </Tooltip>
        )}
        <Switch size="small" checked={on} disabled={!canToggle} onChange={toggle} />
      </Stack>
    </Stack>
  )
}

const RULES: [string, string][] = [
  ['Sandboxed', "Each mod page runs in an isolated frame: it can't reach Vivianne's internals, your other data, or the internet, except origins you allowed."],
  ['One door', 'Mods talk to Vivianne only through the vivianne SDK, and every call is checked against the permissions you approved.'],
  ['Your say', 'Nothing runs until you approve its permissions. A mod that later asks for more stays off until you agree again. Project mods stay off until you trust that project.'],
  ['Limits', '10 MB per mod, 1 MB of saved data, 20 contributions, 100 calls a second. A mod that crashes only takes down its own panel.'],
  ['Follows your theme', "Pages get the app's colours and fonts as --viv-* CSS variables, updated live."],
]

/** Settings → Mods: the mods installed, consent, project trust, the rules,
 * and "New mod". */
export function ModsManager() {
  const mods = useModsStore((s) => s.mods)
  const project = useModsStore((s) => s.project)
  const refresh = useModsStore((s) => s.refresh)
  const trustProject = useModsStore((s) => s.trustProject)
  const setDraft = useAgentStore((s) => s.setDraft)
  const [consent, setConsent] = useState<ModInfo | null>(null)
  const [creating, setCreating] = useState(false)
  const [rulesOpen, setRulesOpen] = useState(false)

  useEffect(() => {
    void refresh()
  }, [refresh])

  const untrusted = mods.filter((m) => m.status === 'untrusted')

  const openFolder = async () => {
    const dirs = await call<{ user: string }>('cmd_mods_dirs')
    if (dirs?.user) await call('cmd_open_external', { path: dirs.user, target: 'explorer' })
  }

  return (
    <Stack sx={{ gap: 1 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
        <Typography variant="subtitle2" sx={{ fontWeight: 600, flex: 1 }}>
          Mods
        </Typography>
        <Button size="small" startIcon={<FolderOpenLinear size={14} color="currentColor" />} onClick={() => void openFolder()} sx={{ textTransform: 'none' }}>
          Folder
        </Button>
        <Button
          size="small"
          startIcon={<MagicpenLinear size={14} color="currentColor" />}
          onClick={() => setDraft('Build me a Vivianne mod that ')}
          sx={{ textTransform: 'none' }}
        >
          Ask Vivianne
        </Button>
        <Button size="small" variant="contained" startIcon={<AddLinear size={14} color="currentColor" />} onClick={() => setCreating(true)} sx={{ textTransform: 'none' }}>
          New mod
        </Button>
      </Stack>
      <Typography variant="caption" color="text.secondary">
        Add your own panels, sidebar sections, chat components (renderers for a ```fence Vivianne writes) and tools. A mod is a
        folder with a mod.json and web files.{' '}
        <ButtonBase onClick={() => setRulesOpen((v) => !v)} sx={{ color: 'primary.main', fontSize: 'inherit', verticalAlign: 'baseline' }}>
          {rulesOpen ? 'Hide the rules' : 'The rules'}
        </ButtonBase>
      </Typography>
      <Collapse in={rulesOpen}>
        <Stack sx={{ gap: 0.5, px: 1.5, py: 1, borderRadius: 2, bgcolor: 'action.hover' }}>
          {RULES.map(([title, text]) => (
            <Typography key={title} variant="caption">
              <b>{title}.</b> {text}
            </Typography>
          ))}
        </Stack>
      </Collapse>

      {untrusted.length > 0 && project && (
        <Alert
          severity="warning"
          action={
            <Button size="small" color="inherit" onClick={() => void trustProject(project, true)} sx={{ textTransform: 'none' }}>
              Trust
            </Button>
          }
        >
          This project comes with {untrusted.length} mod{untrusted.length > 1 ? 's' : ''}. They stay off until you trust this project's
          mods. Only do that for projects you know.
        </Alert>
      )}

      {mods.length === 0 ? (
        <Typography variant="caption" color="text.secondary" sx={{ py: 1 }}>
          No mods yet. Start from a preset with New mod, or ask Vivianne to build one.
        </Typography>
      ) : (
        <Stack sx={{ gap: 0.75 }}>
          {mods.map((m) => (
            <ModRow key={m.key} mod={m} onConsent={setConsent} />
          ))}
        </Stack>
      )}

      <ConsentDialog mod={consent} onClose={() => setConsent(null)} />
      <NewModDialog
        open={creating}
        onClose={() => setCreating(false)}
        onCreated={(key) => {
          setCreating(false)
          const made = useModsStore.getState().mods.find((m) => m.key === key)
          if (made) setConsent(made)
        }}
      />
    </Stack>
  )
}
