/**
 * One look for everything in the transcript: messages, tool cards, step
 * groups and checkpoints share the same width, corners, padding, border and
 * type sizes, so the conversation reads as one column of the same material
 * instead of a mix of shapes.
 */

/** Every block in the transcript stops at the same width. Sizes here read
 * CSS variables Settings → Appearance sets (density, chat width). */
export const CHAT_MAX_WIDTH = 'var(--chat-max-width, min(82%, 720px))'
/** Corners of a top-level block (a message, a card). */
export const CHAT_RADIUS = '14px'
/** Corners of a card nested inside another (a step inside a group). */
export const CHAT_RADIUS_INNER = '10px'

/** Type sizes: message text, a card's title, and the small print. */
export const CHAT_FONT = {
  text: 'var(--chat-font-text, 0.875rem)',
  title: 'var(--chat-font-title, 0.8125rem)',
  meta: 'var(--chat-font-meta, 0.75rem)',
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
  minHeight: 'var(--chat-header-height, 38px)',
} as const

/** A card's title text. */
export const CHAT_TITLE_SX = { fontWeight: 600, fontSize: CHAT_FONT.title, flexShrink: 0 } as const

/** A card's secondary text (counts, targets, timings). */
export const CHAT_META_SX = { fontSize: CHAT_FONT.meta } as const

/**
 * A tool call as a line of text, not a card: the accent colour marks it as
 * something you can open, a soft tint shows on hover. No border, no box.
 */
export const TOOL_ROW_SX = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 0.75,
  maxWidth: '100%',
  minWidth: 0,
  px: '4px',
  mx: '-4px',
  py: '4px',
  borderRadius: '6px',
  color: 'text.secondary',
  fontSize: CHAT_FONT.title,
  fontWeight: 400,
  userSelect: 'none',
  transition: 'background-color 0.12s',
} as const

/** What a single tool row opens onto: indented under it, tied by a rail. */
export const TOOL_BODY_SX = { mt: '4px', minWidth: 0 } as const

/** A step row with nothing to show: empty, or holding only empty elements
 * (a reply that was all tool calls renders an empty wrapper). Media counts
 * as content even though its elements are empty. */
const BLANK = ':empty, :not(:has(> :not(:empty), img, svg, canvas, video, iframe))'

/** A group's steps when opened: one rounded list, a hairline between rows. */
export const TOOL_LIST_SX = {
  mt: '4px',
  border: 1,
  borderColor: 'divider',
  borderRadius: '8px',
  overflow: 'hidden',
  '& > *': { px: '12px', py: '4px', minHeight: 32, display: 'flex', flexDirection: 'column', justifyContent: 'center', boxSizing: 'border-box' },
  // A step that renders nothing takes no row, and a divider sits only
  // between two rows that show something: no blank bands, no stray line
  // above the first.
  [`& > :is(${BLANK})`]: { display: 'none' },
  [`& > :not(${BLANK}) ~ :not(${BLANK})`]: { borderTop: 1, borderColor: 'divider' },
} as const
