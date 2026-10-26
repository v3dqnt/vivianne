import { useEffect } from 'react'
import { ModsRuntime } from './modules/mods/components/ModsRuntime'
import { Box, Stack } from './ui'
import { TitleBar } from './components/TitleBar'
import { ProjectRail } from './modules/sessions/components/ProjectRail'
import { Sidebar } from './modules/sessions/components/Sidebar'
import { SettingsDialog } from './modules/settings/components/SettingsDialog'
import { OptionalComponentsModal } from './modules/settings/components/OptionalComponentsModal'
import { UpdateSurfaces } from './modules/updates/components/UpdateSurfaces'
import { LayoutErrorBoundary } from './components/LayoutErrorBoundary'
import { Workspace } from './modules/workspace/Workspace'
import { GitView, useAppView } from './modules/git/components/GitView'
import { startGitStatus } from './modules/git/store/gitStatusStore'
import { useSidebarStore } from './modules/sessions/store/sidebarStore'
import { installShortcuts, registerShortcuts } from './lib/shortcuts'
import { appShortcuts } from './modules/settings/lib/appShortcuts'
import { startWorkflowRunner } from './modules/automation/workflowRunner'
import { startAutoReadAloud } from './modules/ai/lib/readAloud'
import { WorkflowConfirm } from './modules/automation/WorkflowConfirm'
import { Quickstart } from './modules/hackathon/Quickstart'
import { useLayoutPrefs } from './modules/settings/store/layoutPrefsStore'
import { useSavedLayouts } from './modules/workspace/layout'

function App() {
  const sidebarCollapsed = useSidebarStore((s) => s.collapsed)
  const sidebarSide = useLayoutPrefs((s) => s.sidebarSide)
  const showRail = useLayoutPrefs((s) => s.showRail)
  const savedLayouts = useSavedLayouts((s) => s.saved)
  const view = useAppView((s) => s.view)

  // Every app shortcut, rebindable in Settings → Layout & shortcuts. Saved
  // layouts are actions too, so the list is rebuilt when they change.
  useEffect(() => {
    registerShortcuts(appShortcuts())
  }, [savedLayouts])
  useEffect(() => installShortcuts(), [])
  // Scheduled and folder-triggered workflows run their steps in a thread of their own.
  useEffect(() => startWorkflowRunner(), [])
  useEffect(() => startAutoReadAloud(), [])
  // The repository's state, for the sidebar, the Git tab and the agent.
  useEffect(() => startGitStatus(), [])

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
          {/* Kept mounted under the Git view, so the chat's panels and their
              state are as they were when you come back. */}
          <Box sx={{ display: view === 'chat' ? 'contents' : 'none' }}>
            <Workspace />
          </Box>
          {view === 'git' && <GitView />}
        </LayoutErrorBoundary>
        {sidebarSide === 'right' && sidebar}
      </Stack>

      <SettingsDialog />
      <ModsRuntime />
      <WorkflowConfirm />
      <Quickstart />
      <OptionalComponentsModal />
      <UpdateSurfaces />
    </Stack>
  )
}

export default App
