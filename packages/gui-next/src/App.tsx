import { useEffect } from 'react'
import { ModsRuntime } from './modules/mods/components/ModsRuntime'
import { Stack } from './ui'
import { TitleBar } from './components/TitleBar'
import { ProjectRail } from './modules/sessions/components/ProjectRail'
import { Sidebar } from './modules/sessions/components/Sidebar'
import { SettingsDialog } from './modules/settings/components/SettingsDialog'
import { OptionalComponentsModal } from './modules/settings/components/OptionalComponentsModal'
import { UpdateSurfaces } from './modules/updates/components/UpdateSurfaces'
import { LayoutErrorBoundary } from './components/LayoutErrorBoundary'
import { Workspace } from './modules/workspace/Workspace'
import { useSidebarStore } from './modules/sessions/store/sidebarStore'
import { installShortcuts, registerShortcuts } from './lib/shortcuts'
import { appShortcuts } from './modules/settings/lib/appShortcuts'
import { useLayoutPrefs } from './modules/settings/store/layoutPrefsStore'
import { useSavedLayouts } from './modules/workspace/layout'

function App() {
  const sidebarCollapsed = useSidebarStore((s) => s.collapsed)
  const sidebarSide = useLayoutPrefs((s) => s.sidebarSide)
  const showRail = useLayoutPrefs((s) => s.showRail)
  const savedLayouts = useSavedLayouts((s) => s.saved)

  // Every app shortcut, rebindable in Settings → Layout & shortcuts. Saved
  // layouts are actions too, so the list is rebuilt when they change.
  useEffect(() => {
    registerShortcuts(appShortcuts())
  }, [savedLayouts])
  useEffect(() => installShortcuts(), [])

  const sidebar = !sidebarCollapsed && <Sidebar />

  return (
    <Stack sx={{ height: '100%' }}>
      <TitleBar />

      {/* `bgcolor` is the chrome colour the title bar, rail and sidebar share;
          the workspace's panels sit on it as separate rounded surfaces. */}
      <Stack direction="row" sx={{ flex: 1, minHeight: 0, bgcolor: 'background.chrome' }}>
        {/* Fixed chrome: projects and their days are navigation, not content,
            so they stay put while everything to their right is dockable. */}
        {showRail && <ProjectRail />}
        {sidebarSide === 'left' && sidebar}

        {/* Chat, editor, files, preview, terminals — all panels, arranged
            however the user drags them. See `modules/workspace`. */}
        <LayoutErrorBoundary>
          <Workspace />
        </LayoutErrorBoundary>
        {sidebarSide === 'right' && sidebar}
      </Stack>

      <SettingsDialog />
      <ModsRuntime />
      <OptionalComponentsModal />
      <UpdateSurfaces />
    </Stack>
  )
}

export default App
