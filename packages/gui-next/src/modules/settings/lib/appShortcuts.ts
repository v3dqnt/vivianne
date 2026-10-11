import type { ShortcutAction } from '../../../lib/shortcuts'
import { useSidebarStore } from '../../sessions/store/sidebarStore'
import { useSettingsUi } from '../store/uiStore'
import { useDaysStore } from '../../sessions/store/daysStore'
import { useAgentStore } from '../../ai/store/agentStore'
import { useProjectsStore } from '../../sessions/store/projectsStore'
import { applyPreset, openPanel, toggleZone, useSavedLayouts, applySavedLayout, type Preset } from '../../workspace/layout'

const PRESET_ORDER: Preset[] = ['default', 'focus', 'coding', 'research']

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
    { id: 'panel.right', label: 'Show or hide the right panel', group: 'Panels', defaultKeys: 'Mod+Alt+B', run: () => toggleZone('right') },
    { id: 'panel.bottom', label: 'Show or hide the bottom panel', group: 'Panels', defaultKeys: 'Mod+J', run: () => toggleZone('bottom') },
    { id: 'panel.terminal', label: 'New terminal', group: 'Panels', defaultKeys: 'Mod+Shift+`', run: () => openPanel('terminal', { newInstance: true }) },
    { id: 'panel.files', label: 'Open All files', group: 'Panels', defaultKeys: 'Mod+Shift+E', run: () => openPanel('files') },
    { id: 'panel.preview', label: 'Open Preview', group: 'Panels', defaultKeys: null, run: () => openPanel('preview') },
  ]
  PRESET_ORDER.forEach((p, i) =>
    actions.push({
      id: `layout.${p}`,
      label: `Layout: ${p === 'default' ? 'chat with right panel' : p === 'focus' ? 'chat only' : p}`,
      group: 'Layouts',
      defaultKeys: `Mod+Alt+${i + 1}`,
      run: () => applyPreset(p),
    }),
  )
  for (const l of useSavedLayouts.getState().saved) {
    actions.push({ id: `layout.saved.${l.id}`, label: `Layout: ${l.name}`, group: 'Layouts', defaultKeys: null, run: () => void applySavedLayout(l.id) })
  }
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
