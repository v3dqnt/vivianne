import { useEffect, useMemo } from 'react'
import { Box, GlobalStyles, alpha, useTheme } from '../../ui'
import { DockviewReact, themeDark, themeLight, type DockviewTheme } from 'dockview-react'
import 'dockview-react/dist/styles/dockview.css'
import { SURFACE_RADIUS } from '../../theme'
import { COMPONENTS, GroupActions, PanelTab, ZoneEmptyState } from './panels'
import { attach, detach } from './layout'

/**
 * The main window's panel area: every panel, the chat included, in one
 * dockable grid.
 *
 * Drag a tab onto another group to join it, onto a group's edge to split
 * beside it, or onto the window's edge to dock along it. Drag the gaps to
 * resize. The arrangement is saved as it changes and restored on launch.
 */
export function Workspace() {
  const theme = useTheme()
  const dark = theme.palette.mode === 'dark'

  // Built on dockview's own light/dark theme for its structural defaults,
  // with every colour then overridden from the app theme below — so panels
  // follow the palette and light/dark switches like the rest of the app,
  // rather than looking like a VS Code window dropped into it.
  const dockTheme = useMemo<DockviewTheme>(() => {
    const base = dark ? themeDark : themeLight
    // The gap between panes: narrow, like Terax's 2px+2px gutters plus room
    // for the splitter's hit area.
    return { ...base, name: 'viv', className: `${base.className} viv-dock`, gap: 6 }
  }, [dark])

  useEffect(() => () => detach(), [])

  const p = theme.palette
  const t = p.tokens
  const fast = `${theme.transitions.duration.fast}ms`

  return (
    // Terax's shell: panes float on the frame, with horizontal gutters only
    // (8px at the window edge, a narrow gap between panes). Panes meet the
    // title bar flush, so the header's own padding isn't doubled.
    <Box sx={{ flex: 1, minWidth: 0, height: '100%', bgcolor: 'background.chrome', pr: 1, pb: 1, pl: 0.5 }}>
      <GlobalStyles
        styles={{
          // Doubled class for specificity: dockview's own theme class sets
          // the same variables, and these must win regardless of which
          // stylesheet happens to load later.
          '.viv-dock.viv-dock': {
            fontFamily: theme.typography.fontFamily,
            '--dv-group-view-background-color': 'transparent',
            '--dv-tabs-and-actions-container-background-color': 'transparent',
            '--dv-tabs-and-actions-container-height': '36px',
            '--dv-tabs-and-actions-container-font-size': '12px',
            '--dv-activegroup-visiblepanel-tab-background-color': alpha(t.foreground, 0.07),
            '--dv-activegroup-hiddenpanel-tab-background-color': 'transparent',
            '--dv-inactivegroup-visiblepanel-tab-background-color': alpha(t.foreground, 0.04),
            '--dv-inactivegroup-hiddenpanel-tab-background-color': 'transparent',
            '--dv-activegroup-visiblepanel-tab-color': t.foreground,
            '--dv-activegroup-hiddenpanel-tab-color': alpha(t.foreground, 0.6),
            '--dv-inactivegroup-visiblepanel-tab-color': alpha(t.foreground, 0.75),
            '--dv-inactivegroup-hiddenpanel-tab-color': alpha(t.foreground, 0.5),
            '--dv-tab-divider-color': 'transparent',
            '--dv-separator-border': 'transparent',
            '--dv-paneview-header-border-color': p.divider,
            '--dv-border-radius': `${SURFACE_RADIUS}px`,
            '--dv-tab-border-radius': `${theme.radius.md}px`,
            // Splitters are invisible until hovered, then a hairline.
            '--dv-sash-color': 'transparent',
            '--dv-active-sash-color': t.border,
            '--dv-active-sash-transition-delay': '0.1s',
            '--dv-drag-over-background-color': alpha(t.foreground, 0.06),
            '--dv-drag-over-border-color': t.ring,
            '--dv-drag-over-border': `1px solid ${alpha(t.ring, 0.6)}`,
            '--dv-edge-dock-indicator-color': t.ring,
            '--dv-icon-hover-background-color': p.action.hover,
            '--dv-context-menu-background-color': p.background.paper,
            '--dv-context-menu-color': p.text.primary,
            '--dv-floating-border': `1px solid ${p.divider}`,
            '--dv-floating-box-shadow': theme.elevation.xl,
            '--dv-floating-titlebar-background-color': t.background,
            '--dv-scrollbar-background-color': 'transparent',
          },
          // Each group is a pane: solid, a 9% hairline, no drop shadow.
          '.viv-dock .dv-groupview': {
            borderRadius: `${SURFACE_RADIUS}px`,
            overflow: 'hidden',
            backgroundColor: t.background,
            border: `1px solid ${t.border}`,
            boxShadow: theme.elevation.pane,
          },
          // Tabs: 28px, rounded-md, the active one a soft ink tint with a
          // hairline ring and a small shadow. No underlines, no dividers.
          '.viv-dock .dv-tabs-and-actions-container': {
            padding: '4px 6px 0 6px',
            gap: '2px',
            alignItems: 'center',
          },
          '.viv-dock .dv-tab': {
            padding: 0,
            height: 28,
            borderRadius: `${theme.radius.md}px`,
            marginRight: '2px',
            fontWeight: 500,
            transition: `background-color ${fast}, color ${fast}`,
          },
          '.viv-dock .dv-tab:not(.dv-active-tab):hover': { color: t.foreground, backgroundColor: alpha(t.foreground, 0.04) },
          '.viv-dock .dv-tab.dv-active-tab': { boxShadow: `inset 0 0 0 1px ${alpha(t.foreground, 0.05)}, ${theme.elevation.sm}` },
        }}
      />
      <DockviewReact
        className={dockTheme.className}
        theme={dockTheme}
        components={COMPONENTS}
        defaultTabComponent={PanelTab}
        rightHeaderActionsComponent={GroupActions}
        watermarkComponent={ZoneEmptyState}
        onReady={(e) => attach(e.api)}
      />
    </Box>
  )
}
