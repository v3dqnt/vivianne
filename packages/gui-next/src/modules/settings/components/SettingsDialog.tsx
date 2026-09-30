import { Dialog, Box, Stack, Typography, IconButton, ButtonBase, Tooltip, useTheme } from '../../../ui'
import {
  CloseCircleLinear,
  KeyLinear,
  CpuLinear,
  FlashLinear,
  Link2Linear,
  ArchiveBookLinear,
  MagicpenLinear,
  ClockLinear,
  ColorSwatchLinear,
  UserOctagonLinear,
  ArchiveLinear,
  MicrophoneLinear,
  SparkleLinear,
  GlobalLinear,
  ShieldTickLinear,
} from '../../../icons'
import { ProvidersSection } from './ProvidersSection'
import { LocalModelsSection } from './LocalModelsSection'
import { ModsSection } from './ModsSection'
import { SubagentsSection } from './SubagentsSection'
import { McpSection } from './McpSection'
import { MemorySection } from './MemorySection'
import { DiagnosticsSection } from './DiagnosticsSection'
import { UsageSection } from './UsageSection'
import { SkillsSection } from './SkillsSection'
import { AutomationsSection } from './AutomationsSection'
import { ThemesSection } from './ThemesSection'
import { ComponentsSection } from './ComponentsSection'
import { UpdatesSection } from './UpdatesSection'
import { InstantSection } from './InstantSection'
import { ReliabilitySection } from './ReliabilitySection'
import { WebSearchSection } from './WebSearchSection'
import { AccountSection } from '../../account/components/AccountSection'
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
      { id: 'subagents', label: 'Subagents', icon: UserOctagonLinear },
      { id: 'mcp', label: 'MCP servers', icon: Link2Linear },
      { id: 'websearch', label: 'Web search', icon: GlobalLinear },
      { id: 'memory', label: 'Memory', icon: ArchiveBookLinear },
      { id: 'skills', label: 'Skills', icon: MagicpenLinear },
      { id: 'automations', label: 'Automations', icon: ClockLinear },
      { id: 'instant', label: 'Viv Instant', icon: MicrophoneLinear },
    ],
  },
  {
    label: 'App',
    items: [
      { id: 'diagnostics', label: 'Diagnostics', icon: FlashLinear },
      { id: 'themes', label: 'Theme & layout', icon: ColorSwatchLinear },
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
              {section === 'subagents' && <SubagentsSection />}
              {section === 'mcp' && <McpSection />}
              {section === 'memory' && <MemorySection />}
              {section === 'diagnostics' && <DiagnosticsSection />}
              {section === 'skills' && <SkillsSection />}
              {section === 'automations' && <AutomationsSection />}
              {section === 'instant' && <InstantSection />}
              {section === 'websearch' && <WebSearchSection />}
              {section === 'themes' && <ThemesSection />}
              {section === 'components' && <ComponentsSection />}
              {section === 'updates' && <UpdatesSection />}
            </Box>
          </Box>
        </Stack>
      </Stack>
    </Dialog>
  )
}
