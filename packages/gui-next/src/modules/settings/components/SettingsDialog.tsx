import { useState } from 'react'
import Dialog from '@mui/material/Dialog'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import ButtonBase from '@mui/material/ButtonBase'
import { CloseCircleLinear, KeyLinear } from 'mx-icons'
import { ProvidersSection } from './ProvidersSection'

type Section = 'providers'

// More sections (General, Appearance, Memory, MCP) land here as they get a
// real backend to back them — no placeholder tabs for settings that don't
// do anything yet.
const SECTIONS: { id: Section; label: string; icon: typeof KeyLinear }[] = [
  { id: 'providers', label: 'Providers', icon: KeyLinear },
]

export function SettingsDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [section, setSection] = useState<Section>('providers')

  return (
    <Dialog
      open={open}
      onClose={onClose}
      maxWidth="md"
      fullWidth
      slotProps={{ paper: { sx: { height: 560, borderRadius: 4 } } }}
    >
      <Stack direction="row" sx={{ height: '100%', minHeight: 0 }}>
        <Stack
          sx={{
            width: 200,
            flexShrink: 0,
            borderRight: 1,
            borderColor: 'divider',
            p: 1.5,
            gap: 0.5,
          }}
        >
          <Typography variant="subtitle2" sx={{ fontWeight: 600, px: 1, mb: 1 }}>
            Settings
          </Typography>
          {SECTIONS.map((s) => (
            <ButtonBase
              key={s.id}
              onClick={() => setSection(s.id)}
              sx={{
                display: 'flex',
                alignItems: 'center',
                gap: 1,
                px: 1.5,
                py: 1,
                borderRadius: 2,
                justifyContent: 'flex-start',
                fontSize: '0.8125rem',
                bgcolor: section === s.id ? 'action.selected' : 'transparent',
                color: section === s.id ? 'text.primary' : 'text.secondary',
                '&:hover': { bgcolor: 'action.hover' },
              }}
            >
              <s.icon size={16} color="currentColor" />
              {s.label}
            </ButtonBase>
          ))}
        </Stack>

        <Box sx={{ flex: 1, minWidth: 0, position: 'relative', overflowY: 'auto', p: 3 }}>
          <IconButton size="small" onClick={onClose} sx={{ position: 'absolute', top: 12, right: 12 }}>
            <CloseCircleLinear size={18} color="currentColor" />
          </IconButton>
          {section === 'providers' && <ProvidersSection />}
        </Box>
      </Stack>
    </Dialog>
  )
}
