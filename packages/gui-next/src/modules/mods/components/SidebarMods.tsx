import { useState } from 'react'
import { Box, Stack, Typography, ButtonBase } from '../../../ui'
import { ArrowDownLinear } from '../../../icons'
import { loadPersisted, savePersisted } from '../../../lib/persist'
import { useModsStore, sidebarSections } from '../store/modsStore'
import { ModFrame } from '../host/ModFrame'

const COLLAPSED_KEY = 'viv.mods.sidebarCollapsed'

/** Sidebar sections mods add (`contributes.sidebar`), under the projects.
 * Each folds away; folded ones are unloaded. */
export function SidebarMods() {
  const mods = useModsStore((s) => s.mods)
  const sections = sidebarSections(mods)
  const [collapsed, setCollapsed] = useState<string[]>(() => loadPersisted<string[]>(COLLAPSED_KEY, []))
  if (sections.length === 0) return null

  const toggle = (id: string) => {
    const next = collapsed.includes(id) ? collapsed.filter((c) => c !== id) : [...collapsed, id]
    setCollapsed(next)
    savePersisted(COLLAPSED_KEY, next)
  }

  return (
    <Stack sx={{ gap: 0.5, px: 1, pb: 1 }}>
      {sections.map(({ mod, section }) => {
        const id = `${mod.key}/${section.id}`
        const open = !collapsed.includes(id)
        return (
          <Box key={id}>
            <ButtonBase
              onClick={() => toggle(id)}
              sx={{ width: '100%', justifyContent: 'flex-start', gap: 0.75, px: 1, py: 0.5, borderRadius: 1.5, color: 'text.secondary' }}
            >
              <Box component="span" sx={{ display: 'inline-flex', transform: open ? 'none' : 'rotate(-90deg)', transition: 'transform 120ms' }}>
                <ArrowDownLinear size={12} color="currentColor" />
              </Box>
              <Typography variant="caption" sx={{ fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.4, fontSize: '0.6875rem' }}>
                {section.title}
              </Typography>
            </ButtonBase>
            {open && (
              <ModFrame mod={mod} slot="sidebar" entry={section.entry} minHeight={section.height ?? 48} maxHeight={420} />
            )}
          </Box>
        )
      })}
    </Stack>
  )
}
