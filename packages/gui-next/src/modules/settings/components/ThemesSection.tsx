import { Stack, Typography, Divider, Box, ButtonBase, ButtonGroup, Button } from '../../../ui'
import { PALETTES } from '../../../theme'
import { useThemeStore, type ThemeMode } from '../store/themeStore'
import { PRESET_LABELS, applyPreset, resetLayout, type Preset } from '../../workspace/layout'
import { useSettingsUi } from '../store/uiStore'

/** Themes — palette cards + light/dark toggle. Palettes live in theme.ts
 * (the single source of visual truth) and are applied by rebuilding the MUI
 * theme in main.tsx; this section is only the picker. */
export function ThemesSection() {
  const mode = useThemeStore((s) => s.mode)
  const paletteId = useThemeStore((s) => s.paletteId)
  const setMode = useThemeStore((s) => s.setMode)
  const setPalette = useThemeStore((s) => s.setPalette)
  const closeSettings = useSettingsUi((s) => s.closeSettings)

  // Close Settings on the way out: a preset rearranges the whole window, and
  // the dialog covers exactly the area that changed — left open, a working
  // button looks dead because nothing visible moves until it's dismissed.
  const choose = (preset: Preset) => {
    applyPreset(preset)
    closeSettings()
  }

  return (
    <Stack sx={{ gap: 1.5 }}>
      <Typography variant="subtitle2" sx={{ fontWeight: 700 }}>
        Layout
      </Typography>
      <Typography variant="caption" color="text.secondary">
        Every panel — the chat included — can be dragged anywhere: onto another panel to share its tabs, onto an edge
        to split beside it. Drag the gaps between panels to resize. The arrangement is remembered. These are starting
        points; the Layout button in the title bar has them too.
      </Typography>
      <Stack direction="row" sx={{ gap: 1, flexWrap: 'wrap' }}>
        {(Object.keys(PRESET_LABELS) as Preset[]).map((preset) => (
          <Button key={preset} size="small" variant="outlined" onClick={() => choose(preset)} sx={{ textTransform: 'none' }}>
            {PRESET_LABELS[preset]}
          </Button>
        ))}
        <Button
          size="small"
          color="inherit"
          onClick={() => {
            resetLayout()
            closeSettings()
          }}
          sx={{ textTransform: 'none', color: 'text.secondary' }}
        >
          Reset to default
        </Button>
      </Stack>
      <Divider />

      <Typography variant="subtitle2" sx={{ fontWeight: 700 }}>
        Theme
      </Typography>
      <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'flex-end' }}>
        <ButtonGroup size="small">
          {(['dark', 'light'] as ThemeMode[]).map((m) => (
            <Button
              key={m}
              onClick={() => setMode(m)}
              sx={{ textTransform: 'capitalize', bgcolor: mode === m ? 'action.selected' : 'transparent', '&:hover': { bgcolor: mode === m ? 'action.selected' : 'action.hover' } }}
            >
              {m}
            </Button>
          ))}
        </ButtonGroup>
      </Stack>
      <Typography variant="caption" color="text.secondary">
        Each palette ships a light and dark variant; the toggle above flips between them.
      </Typography>
      <Divider />

      <Stack sx={{ gap: 1 }}>
        {PALETTES.map((p) => {
          const active = p.id === paletteId
          return (
            <ButtonBase
              key={p.id}
              onClick={() => setPalette(p.id)}
              sx={{
                display: 'flex',
                alignItems: 'center',
                gap: 2,
                p: 1.5,
                borderRadius: 2,
                border: 1,
                borderColor: active ? 'primary.main' : 'divider',
                bgcolor: active ? 'action.selected' : 'transparent',
                '&:hover': { bgcolor: active ? 'action.selected' : 'action.hover' },
              }}
            >
              <Box sx={{ display: 'flex', flexShrink: 0 }}>
                {p.swatches.map((c) => (
                  <Box
                    key={c}
                    sx={{
                      width: 22,
                      height: 22,
                      borderRadius: '50%',
                      bgcolor: c,
                      border: 1,
                      borderColor: 'rgba(128,128,128,0.4)',
                      mx: -0.4,
                    }}
                  />
                ))}
              </Box>
              <Stack sx={{ textAlign: 'left', minWidth: 0 }}>
                <Typography variant="body2" sx={{ fontWeight: 600 }}>
                  {p.symbol} {p.label}
                </Typography>
                <Typography variant="caption" color="text.secondary" noWrap>
                  {active ? 'Active' : 'Click to apply'}
                </Typography>
              </Stack>
            </ButtonBase>
          )
        })}
      </Stack>
    </Stack>
  )
}
