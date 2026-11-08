import { Box } from '../../ui'
import { CommandSquareLinear, DocumentTextLinear, FolderLinear, LayerLinear } from '../../icons'
import { SegmentedTabs } from '../../components/SegmentedTabs'
import { useRail, type RailTab } from './railStore'

const TABS: { id: RailTab; label: string; Icon: typeof LayerLinear; needsFolder: boolean }[] = [
  { id: 'overview', label: 'Overview', Icon: LayerLinear, needsFolder: false },
  { id: 'files', label: 'Files', Icon: FolderLinear, needsFolder: true },
  { id: 'preview', label: 'Preview', Icon: DocumentTextLinear, needsFolder: true },
  { id: 'terminal', label: 'Terminal', Icon: CommandSquareLinear, needsFolder: true },
]

/**
 * The rail's tabs, in the app's own segmented control (the same one Home uses). In the compact overview card they are icons only; once
 * the rail widens for a working tab there is room for their names. Preview appears once a file has been opened.
 */
export function RailTabs({ hasFolder }: { hasFolder: boolean }) {
  const tab = useRail((s) => s.tab)
  const hasPreview = useRail((s) => s.previewPath !== null)
  const setTab = useRail((s) => s.setTab)
  const shown = TABS.filter((t) => (!t.needsFolder || hasFolder) && (t.id !== 'preview' || hasPreview))
  if (shown.length < 2) return null
  const tabs = shown.map(({ id, label, Icon }) => ({ id, label, icon: <Icon size={14} color="currentColor" /> }))
  return (
    // The card asks its own width: icons only while it is narrow, names once it has widened.
    <Box sx={{ mb: '12px', flexShrink: 0, '& .rail-wide': { display: 'none' }, '@container (min-width: 360px)': { '& .rail-wide': { display: 'block' }, '& .rail-narrow': { display: 'none' } } }}>
      <Box className="rail-narrow">
        <SegmentedTabs fill compact value={tab} onChange={setTab} tabs={tabs} />
      </Box>
      <Box className="rail-wide">
        <SegmentedTabs fill value={tab} onChange={setTab} tabs={tabs} />
      </Box>
    </Box>
  )
}
