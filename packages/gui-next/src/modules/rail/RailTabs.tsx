import { Box, ButtonBase, Stack, alpha, useTheme } from '../../ui'
import { CommandSquareLinear, DocumentTextLinear, FolderLinear, LayerLinear } from '../../icons'
import { useRail, type RailTab } from './railStore'

const TABS: { id: RailTab; label: string; Icon: typeof LayerLinear; needsFolder: boolean }[] = [
  { id: 'overview', label: 'Overview', Icon: LayerLinear, needsFolder: false },
  { id: 'files', label: 'Files', Icon: FolderLinear, needsFolder: true },
  { id: 'preview', label: 'Preview', Icon: DocumentTextLinear, needsFolder: true },
  { id: 'terminal', label: 'Terminal', Icon: CommandSquareLinear, needsFolder: true },
]

/**
 * The rail's tabs: one quiet segmented row at its top. The active tab sits on a raised chip; the others are icons until there is room
 * for their names (the rail asks its own width). Preview only appears once a file has been opened.
 */
export function RailTabs({ hasFolder }: { hasFolder: boolean }) {
  const theme = useTheme()
  const tab = useRail((s) => s.tab)
  const hasPreview = useRail((s) => s.previewPath !== null)
  const setTab = useRail((s) => s.setTab)
  const shown = TABS.filter((t) => (!t.needsFolder || hasFolder) && (t.id !== 'preview' || hasPreview))
  if (shown.length < 2) return null
  return (
    <Stack
      direction="row"
      role="tablist"
      sx={{ gap: '2px', p: '3px', borderRadius: '8px', bgcolor: alpha(theme.palette.text.primary, 0.04), flexShrink: 0, mb: '12px' }}
    >
      {shown.map(({ id, label, Icon }) => {
        const active = id === tab
        return (
            <ButtonBase
              key={id}
              title={label}
              role="tab"
              aria-selected={active}
              aria-label={label}
              onClick={() => setTab(active && id !== 'overview' ? 'overview' : id)}
              sx={{
                flex: '1 1 0',
                minWidth: 30,
                height: 28,
                px: '8px',
                gap: '6px',
                borderRadius: '6px',
                fontSize: '0.75rem',
                fontWeight: 500,
                color: active ? (theme.palette.mode === 'dark' ? 'text.primary' : 'background.paper') : 'text.secondary',
                bgcolor: active ? alpha(theme.palette.text.primary, theme.palette.mode === 'dark' ? 0.12 : 0.9) : 'transparent',
                boxShadow: active ? `0 1px 2px ${alpha('#000', theme.palette.mode === 'dark' ? 0.4 : 0.08)}` : 'none',
                transition: 'background-color 160ms, color 160ms',
                '&:hover': { color: 'text.primary' },
                '& .lbl': { display: 'none' },
                // Names once the rail has the room (it widens for the working tabs).
                '@container (min-width: 360px)': { '& .lbl': { display: 'inline' } },
              }}
            >
              <Box sx={{ display: 'flex', flexShrink: 0 }}>
                <Icon size={14} color="currentColor" />
              </Box>
              <Box component="span" className="lbl" sx={{ whiteSpace: 'nowrap' }}>
                {label}
              </Box>
            </ButtonBase>
        )
      })}
    </Stack>
  )
}
