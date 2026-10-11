import { Stack, Typography } from '../../../ui'
import { useModsStore, findModPanel } from '../store/modsStore'
import { ModFrame } from '../host/ModFrame'

/** A dock panel a mod adds (`contributes.panels`). The tab's `panelName`
 * is `<mod key>/<panel id>`. */
export function ModPanel({ name }: { name: string | null }) {
  const mods = useModsStore((s) => s.mods)
  const loaded = useModsStore((s) => s.loaded)
  const found = findModPanel(mods, name)

  if (!found) {
    if (!loaded) return null
    return (
      <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center', gap: 0.5, p: 3, textAlign: 'center' }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Panel not available
        </Typography>
        <Typography variant="caption" color="text.secondary">
          The mod behind this panel is off or was removed. Settings → Mods.
        </Typography>
      </Stack>
    )
  }
  return (
    <Stack sx={{ flex: 1, minHeight: 0 }}>
      <ModFrame
        key={found.name}
        mod={found.mod}
        slot="panel"
        entry={found.panel.entry ?? undefined}
        url={found.panel.url ?? undefined}
        fill
      />
    </Stack>
  )
}
