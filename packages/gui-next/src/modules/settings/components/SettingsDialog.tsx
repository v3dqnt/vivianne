import Dialog from '@mui/material/Dialog'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import ButtonBase from '@mui/material/ButtonBase'
import Tooltip from '@mui/material/Tooltip'
import { useTheme } from '@mui/material/styles'
import { surfaceCard } from '../../../theme'
import {
  CloseCircleLinear,
  KeyLinear,
  CpuLinear,
  FlashLinear,
  Link2Linear,
  ArchiveBookLinear,
  ChartLinear,
  MagicpenLinear,
  ClockLinear,
  ColorSwatchLinear,
  UserOctagonLinear,
  ArchiveLinear,
  MicrophoneLinear,
  SparkleLinear,
} from '../../../icons'
import { ProvidersSection } from './ProvidersSection'
import { LocalModelsSection } from './LocalModelsSection'
import { ModsSection } from './ModsSection'
import { SubagentsSection } from './SubagentsSection'
import { McpSection } from './McpSection'
import { MemorySection } from './MemorySection'
import { UsageSection } from './UsageSection'
import { SkillsSection } from './SkillsSection'
import { AutomationsSection } from './AutomationsSection'
import { ThemesSection } from './ThemesSection'
import { VoiceSection } from './VoiceSection'
import { ComponentsSection } from './ComponentsSection'
import { UpdatesSection } from './UpdatesSection'
import { AccountSection } from '../../account/components/AccountSection'
import { MultiplayerSection } from '../../multiplayer/components/MultiplayerSection'
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
      { id: 'account', label: 'Account', icon: UserOctagonLinear },
      { id: 'multiplayer', label: 'Multiplayer', icon: Link2Linear },
    ],
  },
  {
    label: 'Model',
    items: [
      { id: 'providers', label: 'Providers', icon: KeyLinear },
      { id: 'local', label: 'Local models', icon: CpuLinear },
    ],
  },
  {
    label: 'Agent',
    items: [
      { id: 'mods', label: 'Mods', icon: FlashLinear },
      { id: 'subagents', label: 'Subagents', icon: UserOctagonLinear },
      { id: 'mcp', label: 'MCP servers', icon: Link2Linear },
      { id: 'memory', label: 'Memory', icon: ArchiveBookLinear },
      { id: 'skills', label: 'Skills', icon: MagicpenLinear },
      { id: 'automations', label: 'Automations', icon: ClockLinear },
      { id: 'voice', label: 'Voice', icon: MicrophoneLinear },
    ],
  },
  {
    label: 'App',
    items: [
      { id: 'usage', label: 'Usage', icon: ChartLinear },
      { id: 'themes', label: 'Theme', icon: ColorSwatchLinear },
      { id: 'components', label: 'Components', icon: ArchiveLinear },
      { id: 'updates', label: 'Updates', icon: SparkleLinear },
    ],
  },
]

export function SettingsDialog() {
  const theme = useTheme()
  const SURFACE = surfaceCard(theme.palette.mode)
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
        paper: { sx: { height: 660, overflow: 'hidden', backgroundImage: 'none', ...SURFACE } },
        backdrop: { sx: { backdropFilter: 'blur(3px)' } },
      }}
    >
      <Stack direction="row" sx={{ height: '100%', minHeight: 0 }}>
        <Stack
          sx={{
            width: 220,
            flexShrink: 0,
            borderRight: 1,
            borderColor: 'divider',
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
                sx={{ px: 1.5, mb: 0.5, fontWeight: 600, letterSpacing: 0.6, textTransform: 'uppercase', fontSize: '0.625rem', color: 'text.secondary' }}
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
              {section === 'account' && <AccountSection />}
              {section === 'multiplayer' && <MultiplayerSection />}
              {section === 'providers' && <ProvidersSection />}
              {section === 'local' && <LocalModelsSection />}
              {section === 'mods' && <ModsSection />}
              {section === 'subagents' && <SubagentsSection />}
              {section === 'mcp' && <McpSection />}
              {section === 'memory' && <MemorySection />}
              {section === 'usage' && <UsageSection />}
              {section === 'skills' && <SkillsSection />}
              {section === 'automations' && <AutomationsSection />}
              {section === 'voice' && <VoiceSection />}
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
