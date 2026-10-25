import { ButtonBase, Tooltip, alpha, useTheme } from '../../ui'
import { EDITION, useQuickstart } from './edition'

/** The edition's mark in the title bar; click to reopen the quickstart. */
export function EditionBadge() {
  const theme = useTheme()
  const show = useQuickstart((s) => s.show)
  return (
    <Tooltip title="Set up Nebius and Nemotron">
      <ButtonBase
        onClick={show}
        sx={{
          ml: 1,
          px: 0.875,
          height: 18,
          borderRadius: '5px',
          pointerEvents: 'auto',
          fontSize: '0.625rem',
          fontWeight: 700,
          letterSpacing: 0.3,
          textTransform: 'uppercase',
          color: 'primary.main',
          bgcolor: alpha(theme.palette.primary.main, 0.14),
          '&:hover': { bgcolor: alpha(theme.palette.primary.main, 0.24) },
        }}
      >
        {EDITION.short}
      </ButtonBase>
    </Tooltip>
  )
}
