/** Every control under the composer is this one button: same height, same
 * type, no border, muted until hovered or set. */
export const TOOLBAR_PILL = {
  display: 'flex',
  alignItems: 'center',
  gap: '6px',
  height: 28,
  px: '8px',
  minWidth: 0,
  borderRadius: '8px',
  border: 0,
  fontSize: '12px',
  fontWeight: 500,
  lineHeight: 1,
  textTransform: 'none',
  color: 'text.secondary',
  bgcolor: 'transparent',
  '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
  '& .MuiTypography-root, & .ui-Typography': { fontSize: 'inherit', fontWeight: 'inherit', lineHeight: 'inherit' },
} as const
