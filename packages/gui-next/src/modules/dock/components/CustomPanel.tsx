import { useEffect } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import { usePlaybookStore } from '../../settings/store/playbookStore'
import { isPanel } from '../../settings/lib/playbooks'

/** Renders one panel-type playbook (`~/.viv/playbooks/<name>.json` with a
 * `panel` field — see `coding_agent::playbooks`) as an iframe. Which panel
 * a tab shows is the tab's own `panelName` (the playbook's `name`), looked
 * up fresh on every render rather than baked into a route — so `set_panel`
 * and the dock's "add a view" menu can open any installed panel without the
 * dock itself needing to know they exist. */
export function CustomPanel({ id }: { id: string | null }) {
  const playbooks = usePlaybookStore((s) => s.playbooks)
  const refresh = usePlaybookStore((s) => s.refresh)

  useEffect(() => {
    void refresh()
  }, [refresh])

  const panel = playbooks.find((p) => p.name === id && isPanel(p))

  if (!id) {
    return (
      <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center', p: 3 }}>
        <Typography variant="body2" color="text.secondary">
          No panel selected.
        </Typography>
      </Stack>
    )
  }

  if (!panel || !panel.panel) {
    return (
      <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center', gap: 0.5, p: 3, textAlign: 'center' }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Panel not found
        </Typography>
        <Typography variant="caption" color="text.secondary">
          "{id}" isn't installed (or was removed) — check Settings → Mods → Playbooks.
        </Typography>
      </Stack>
    )
  }

  return (
    <Box
      component="iframe"
      src={panel.panel.url}
      title={panel.name}
      sx={{ flex: 1, minHeight: 0, width: '100%', border: 0, bgcolor: 'background.paper' }}
      // Same-origin-ish local dev servers only (enforced backend-side too,
      // and by the app's CSP frame-src) — no need for extra sandboxing
      // beyond what an iframe already provides against a page you don't
      // control reaching back out into the host app.
      sandbox="allow-scripts allow-forms allow-same-origin allow-popups"
    />
  )
}
