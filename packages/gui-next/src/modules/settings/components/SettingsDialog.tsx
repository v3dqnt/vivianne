import { lazy, Suspense } from 'react'
import { INSTANT_ENABLED } from '../../instant/enabled'
import { Dialog, Box, Divider, Stack, Typography, IconButton, ButtonBase, Tooltip, useTheme } from '../../../ui'
import {
  CloseCircleLinear,
  KeyLinear,
  LockLinear,
  CpuLinear,
  FlashLinear,
  Link2Linear,
  ArchiveBookLinear,
  MagicpenLinear,
  ClockLinear,
  ColorSwatchLinear,
  LayoutLinear,
  UserOctagonLinear,
  ArchiveLinear,
  MicrophoneLinear,
  SparkleLinear,
  GlobalLinear,
  ShieldTickLinear,
} from '../../../icons'
const ProvidersSection = lazy(() => import('./ProvidersSection').then((m) => ({ default: m.ProvidersSection })))
const LocalModelsSection = lazy(() => import('./LocalModelsSection').then((m) => ({ default: m.LocalModelsSection })))
const ModsSection = lazy(() => import('./ModsSection').then((m) => ({ default: m.ModsSection })))
const VaultSection = lazy(() => import('./VaultSection').then((m) => ({ default: m.VaultSection })))
const BrowserSection = lazy(() => import('./BrowserSection').then((m) => ({ default: m.BrowserSection })))
const ConnectionsSection = lazy(() => import('./ConnectionsSection').then((m) => ({ default: m.ConnectionsSection })))
const MemorySection = lazy(() => import('./MemorySection').then((m) => ({ default: m.MemorySection })))
const DiagnosticsSection = lazy(() => import('./DiagnosticsSection').then((m) => ({ default: m.DiagnosticsSection })))
const UsageSection = lazy(() => import('./UsageSection').then((m) => ({ default: m.UsageSection })))
const SkillsSection = lazy(() => import('./SkillsSection').then((m) => ({ default: m.SkillsSection })))
const AutomationsSection = lazy(() => import('./AutomationsSection').then((m) => ({ default: m.AutomationsSection })))
const ThemesSection = lazy(() => import('./ThemesSection').then((m) => ({ default: m.ThemesSection })))
const LayoutSection = lazy(() => import('./LayoutSection').then((m) => ({ default: m.LayoutSection })))
const ComponentsSection = lazy(() => import('./ComponentsSection').then((m) => ({ default: m.ComponentsSection })))
const UpdatesSection = lazy(() => import('./UpdatesSection').then((m) => ({ default: m.UpdatesSection })))
const InstantSection = lazy(() => import('./InstantSection').then((m) => ({ default: m.InstantSection })))
const ReliabilitySection = lazy(() => import('./ReliabilitySection').then((m) => ({ default: m.ReliabilitySection })))
const WebSearchSection = lazy(() => import('./WebSearchSection').then((m) => ({ default: m.WebSearchSection })))
const AccountSection = lazy(() => import('../../account/components/AccountSection').then((m) => ({ default: m.AccountSection })))
import { useSettingsUi, type SettingsSection } from '../store/uiStore'

interface SectionDef {
  id: SettingsSection
  label: string
  icon: typeof KeyLinear
}

/** Groups exist purely as visual breathing room in the sidebar — sections
 * within a group share no other coupling. */
const GROUPS: { label: string; items: SectionDef[] }[] = [
  {
    label: 'Account',
    items: [
      // Usage lives here rather than in its own tab: what it reports — tokens
      // spent, cost, which days were active — is an account fact, and having
      // to cross the sidebar to answer "what has this cost me" split one
      // question across two places.
      { id: 'account', label: 'Account & usage', icon: UserOctagonLinear },
    ],
  },
  {
    label: 'Model',
    items: [
      { id: 'providers', label: 'Providers', icon: KeyLinear },
      { id: 'local', label: 'Local models', icon: CpuLinear },
      { id: 'reliability', label: 'Reliability', icon: ShieldTickLinear },
    ],
  },
  {
    label: 'Agent',
    items: [
      { id: 'mods', label: 'Mods', icon: FlashLinear },
      { id: 'connections', label: 'Connections', icon: Link2Linear },
      { id: 'websearch', label: 'Web & browser', icon: GlobalLinear },
      { id: 'memory', label: 'Memory', icon: ArchiveBookLinear },
      { id: 'vault', label: 'Vault', icon: LockLinear },
      { id: 'skills', label: 'Skills', icon: MagicpenLinear },
      { id: 'automations', label: 'Workflows', icon: ClockLinear },
      ...(INSTANT_ENABLED ? [{ id: 'instant' as const, label: 'Viv Instant', icon: MicrophoneLinear }] : []),
    ],
  },
  {
    label: 'App',
    items: [
      { id: 'diagnostics', label: 'Diagnostics', icon: FlashLinear },
      { id: 'themes', label: 'Appearance', icon: ColorSwatchLinear },
      { id: 'layout', label: 'Layout & shortcuts', icon: LayoutLinear },
      { id: 'components', label: 'Components', icon: ArchiveLinear },
      { id: 'updates', label: 'Updates', icon: SparkleLinear },
    ],
  },
]

export function SettingsDialog() {
  const theme = useTheme()
  const open = useSettingsUi((s) => s.open)
  const section = useSettingsUi((s) => s.section)
  const openSettings = useSettingsUi((s) => s.openSettings)
  const closeSettings = useSettingsUi((s) => s.closeSettings)
  const activeLabel = GROUPS.flatMap((g) => g.items).find((s) => s.id === section)?.label

  return (
    <Dialog
      open={open}
      onClose={closeSettings}
      maxWidth="md"
      fullWidth
      slotProps={{
        // Solid, not glass: Settings is read closely, and text over a blurred
        // window behind it is harder to read than text on a plain surface.
        paper: {
          sx: {
            height: 660,
            overflow: 'hidden',
            backgroundImage: 'none',
            bgcolor: 'background.paper',
            border: 0,
            borderColor: 'divider',
            borderRadius: `${theme.radius['3xl']}px`,
            boxShadow: `${theme.elevation.ring}, ${theme.elevation.xl}`,
          },
        },
      }}
    >
      <Stack direction="row" sx={{ height: '100%', minHeight: 0 }}>
        <Stack
          sx={{
            width: 220,
            flexShrink: 0,
            borderRight: 1,
            borderColor: 'divider',
            bgcolor: 'background.chrome',
            p: 2,
            pt: 2.5,
            gap: 2.25,
            overflowY: 'auto',
          }}
        >
          <Typography variant="subtitle1" sx={{ fontWeight: 700, px: 1 }}>
            Settings
          </Typography>
          {GROUPS.map((group) => (
            <Stack key={group.label} sx={{ gap: 0.375 }}>
              <Typography
                variant="caption"
                sx={{ px: 1.5, mb: 0.5, fontWeight: 600, letterSpacing: 0.6, textTransform: 'uppercase', fontSize: '0.6875rem', color: 'text.secondary' }}
              >
                {group.label}
              </Typography>
              {group.items.map((s) => (
                <ButtonBase
                  key={s.id}
                  onClick={() => openSettings(s.id)}
                  sx={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 1.25,
                    px: 1.5,
                    py: 0.875,
                    borderRadius: 1.5,
                    justifyContent: 'flex-start',
                    fontSize: '0.8125rem',
                    fontWeight: 500,
                    bgcolor: section === s.id ? 'action.selected' : 'transparent',
                    color: section === s.id ? 'text.primary' : 'text.secondary',
                    transition: 'background-color 0.15s, color 0.15s, transform 0.08s',
                    '&:hover': { bgcolor: section === s.id ? 'action.selected' : 'action.hover', color: 'text.primary' },
                    '&:active': { transform: 'scale(0.98)' },
                  }}
                >
                  <s.icon size={15} color="currentColor" />
                  {s.label}
                </ButtonBase>
              ))}
            </Stack>
          ))}
        </Stack>

        <Stack sx={{ flex: 1, minWidth: 0, height: '100%', minHeight: 0 }}>
          <Stack
            direction="row"
            sx={{ alignItems: 'center', justifyContent: 'space-between', px: 3.5, py: 1.75, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}
          >
            <Typography variant="subtitle2" sx={{ fontWeight: 600, color: 'text.secondary' }}>
              {activeLabel}
            </Typography>
            <Tooltip title="Close">
              <IconButton
                size="small"
                onClick={closeSettings}
                sx={{
                  transition: 'background-color 0.15s, transform 0.08s',
                  '&:hover': { bgcolor: 'action.hover' },
                  '&:active': { transform: 'scale(0.92)' },
                }}
              >
                <CloseCircleLinear size={18} color="currentColor" />
              </IconButton>
            </Tooltip>
          </Stack>

          <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: 3.5, py: 3 }}>
            {/* Providers is a master-detail layout (list + form) that wants
                the full width; every other section is a single settings
                column, which reads better constrained. */}
            <Box sx={section === 'providers' ? { height: '100%' } : { maxWidth: 640 }}>
              {/* Each section loads the first time it's opened. */}
              <Suspense fallback={null}>
              {(section === 'account' || section === 'usage') && (
                <Stack sx={{ gap: 4 }}>
                  <AccountSection />
                  <Box>
                    <Typography variant="subtitle2" sx={{ fontWeight: 600, mb: 0.25 }}>
                      Usage
                    </Typography>
                    <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 2 }}>
                      Tokens and cost across every provider, from this machine's local history.
                    </Typography>
                    <UsageSection />
                  </Box>
                </Stack>
              )}
              {section === 'providers' && <ProvidersSection />}
              {section === 'reliability' && <ReliabilitySection />}
              {section === 'local' && <LocalModelsSection />}
              {section === 'mods' && <ModsSection />}
              {/* Manual MCP servers live under Connections → Advanced; 'mcp' still opens it. */}
              {(section === 'connections' || section === 'mcp') && <ConnectionsSection />}
              {section === 'memory' && <MemorySection />}
              {section === 'vault' && <VaultSection />}
              {section === 'diagnostics' && <DiagnosticsSection />}
              {section === 'skills' && <SkillsSection />}
              {section === 'automations' && <AutomationsSection />}
              {section === 'instant' && <InstantSection />}
              {/* One tab for the web: how the agent searches it, and the
                  user's own browser it can work in. 'browser' still opens it. */}
              {(section === 'websearch' || section === 'browser') && (
                <Stack sx={{ gap: 4 }}>
                  <WebSearchSection />
                  <Divider />
                  <BrowserSection />
                </Stack>
              )}
              {section === 'themes' && <ThemesSection />}
              {section === 'layout' && <LayoutSection />}
              {section === 'components' && <ComponentsSection />}
              {section === 'updates' && <UpdatesSection />}
              </Suspense>
            </Box>
          </Box>
        </Stack>
      </Stack>
    </Dialog>
  )
}
