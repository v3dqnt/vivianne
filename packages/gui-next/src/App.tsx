import { useEffect } from 'react'
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

function App() {
  const sidebarCollapsed = useSidebarStore((s) => s.collapsed)
  const toggleSidebar = useSidebarStore((s) => s.toggle)

  // Ctrl/Cmd+B folds the sidebar, as in editors.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'b') {
        e.preventDefault()
        toggleSidebar()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [toggleSidebar])

  return (
    <Stack sx={{ height: '100%' }}>
      <TitleBar />

      {/* `bgcolor` is the chrome colour the title bar, rail and sidebar share;
          the workspace's panels sit on it as separate rounded surfaces. */}
      <Stack direction="row" sx={{ flex: 1, minHeight: 0, bgcolor: 'background.chrome' }}>
        {/* Fixed chrome: projects and their days are navigation, not content,
            so they stay put while everything to their right is dockable. */}
        <ProjectRail />
        {!sidebarCollapsed && <Sidebar />}

        {/* Chat, editor, files, preview, terminals — all panels, arranged
            however the user drags them. See `modules/workspace`. */}
        <LayoutErrorBoundary>
          <Workspace />
        </LayoutErrorBoundary>
      </Stack>

      <SettingsDialog />
      <OptionalComponentsModal />
      <UpdateSurfaces />
    </Stack>
  )
}

export default App
