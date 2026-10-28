import type { ShortcutAction } from '../../../lib/shortcuts'
import { useSidebarStore } from '../../sessions/store/sidebarStore'
import { useSettingsUi } from '../store/uiStore'
import { useDaysStore } from '../../sessions/store/daysStore'
import { useAgentStore } from '../../ai/store/agentStore'
import { useProjectsStore } from '../../sessions/store/projectsStore'


/** Every app action a key can run. Defaults avoid what the composer and
 * editors already use (Ctrl+Enter, Ctrl+Z, Ctrl+C/V/X/A…). */
export function appShortcuts(): ShortcutAction[] {
  const actions: ShortcutAction[] = [
    { id: 'sidebar.toggle', label: 'Show or hide the sidebar', group: 'App', defaultKeys: 'Mod+B', run: () => useSidebarStore.getState().toggle() },
    { id: 'settings.open', label: 'Open Settings', group: 'App', defaultKeys: 'Mod+,', run: () => useSettingsUi.getState().openSettings() },
    { id: 'chat.new', label: 'New chat', group: 'Chat', defaultKeys: 'Mod+Shift+N', run: () => useDaysStore.getState().newThread() },
    {
      id: 'chat.focus',
      label: 'Focus the message box',
      group: 'Chat',
      defaultKeys: 'Mod+L',
      run: () => void window.dispatchEvent(new Event('viv:focus-composer')),
    },
    { id: 'chat.stop', label: 'Stop the agent', group: 'Chat', defaultKeys: 'Mod+Shift+.', run: () => useAgentStore.getState().abort() },
  ]
  for (let n = 1; n <= 9; n++) {
    actions.push({
      id: `project.${n}`,
      label: `Switch to project ${n}`,
      group: 'Projects',
      defaultKeys: `Mod+${n}`,
      run: () => {
        const s = useProjectsStore.getState()
        const p = s.projects[n - 1]
        if (p) s.setActiveProject(p.id)
      },
    })
  }
  return actions
}
