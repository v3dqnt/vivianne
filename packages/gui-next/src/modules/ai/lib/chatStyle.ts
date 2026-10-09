/**
 * One look for everything in the transcript: messages, tool cards, step
 * groups and checkpoints share the same width, corners, padding, border and
 * type sizes, so the conversation reads as one column of the same material
 * instead of a mix of shapes.
 */

/** Every block in the transcript stops at the same width. */
export const CHAT_MAX_WIDTH = 'min(82%, 720px)'
/** Corners of a top-level block (a message, a card). */
export const CHAT_RADIUS = '14px'
/** Corners of a card nested inside another (a step inside a group). */
export const CHAT_RADIUS_INNER = '10px'

/** Type sizes: message text, a card's title, and the small print. */
export const CHAT_FONT = {
  text: '0.875rem',
  title: '0.8125rem',
  meta: '0.75rem',
} as const

/** The box every card sits in. */
export const CHAT_SURFACE_SX = {
  border: 1,
  borderColor: 'divider',
  borderRadius: CHAT_RADIUS,
  bgcolor: 'background.paper',
  overflow: 'hidden',
} as const

/** A card's header row: one height and padding for all of them. */
export const CHAT_HEADER_SX = {
  display: 'flex',
  alignItems: 'center',
  gap: 1,
  px: 1.5,
  py: 0.75,
  minHeight: 38,
} as const

/** A card's title text. */
export const CHAT_TITLE_SX = { fontWeight: 600, fontSize: CHAT_FONT.title, flexShrink: 0 } as const

/** A card's secondary text (counts, targets, timings). */
export const CHAT_META_SX = { fontSize: CHAT_FONT.meta } as const
