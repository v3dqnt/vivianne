/**
 * One look for everything in the transcript: messages, tool cards, step
 * groups and checkpoints share the same width, corners, padding, border and
 * type sizes, so the conversation reads as one column of the same material
 * instead of a mix of shapes.
 */

import type { Theme } from '../../../ui'

/** The reading column the transcript and the composer share. Prose, tool
 * rows, checkpoints and summaries all start at its left edge and end at its
 * right edge, so the transcript has one left margin and one right margin
 * instead of a different width per block type. */
export const CHAT_COLUMN = 880

/** Your own messages are the one thing that stops short of the column: they
 * sit in a block on the right, narrow enough to read as a quotation rather
 * than a page. Sizes here read CSS variables Settings → Appearance sets
 * (density, chat width). */
export const CHAT_MAX_WIDTH = 'var(--chat-max-width, min(72%, 640px))'

/** The inset prose and tool rows share. It gives a row room for its hover
 * tint and its chevron without moving its text off the common left edge.
 * Every gap in the transcript is a multiple of 4px, starting here. */
export const CHAT_GUTTER = '8px'
/** Corners of a top-level block (a message, a card). */
export const CHAT_RADIUS = '12px'
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
 *
 * The row fills the reading column rather than hugging its own text. That is
 * what makes a stack of them symmetric: every label starts on the same left
 * edge as the prose, and every chevron ends on the same right edge, so the
 * group reads as a list instead of a ragged block. The gutter is padding, not
 * a negative margin, so a hover tint can't push the text sideways.
 */
export const TOOL_ROW_SX = {
  display: 'flex',
  alignItems: 'center',
  gap: '8px',
  width: '100%',
  maxWidth: '100%',
  minWidth: 0,
  boxSizing: 'border-box',
  px: CHAT_GUTTER,
  py: '4px',
  borderRadius: '6px',
  color: 'text.secondary',
  fontSize: CHAT_FONT.title,
  fontWeight: 400,
  lineHeight: 1.5,
  userSelect: 'none',
  transition: 'color 0.12s, background-color 0.12s',
} as const

/** What a single tool row opens onto: inset by the shared gutter on both
 * sides, so its left edge lines up with the row's text and its right edge
 * with the chevron, and held off the next row's hairline below. */
export const TOOL_BODY_SX = { mt: '4px', mx: CHAT_GUTTER, mb: '8px', minWidth: 0 } as const

/** The tinted well every tool body draws in: output, a checklist, a note.
 * One tint, one radius, one padding, so the bodies read as one material. */
export const TOOL_WELL_SX = { borderRadius: '8px', bgcolor: 'action.hover', px: '12px', py: '8px', minWidth: 0 } as const

/** A step row with nothing to show: empty, or holding only empty elements
 * (a reply that was all tool calls renders an empty wrapper). Media counts
 * as content even though its elements are empty. */
const BLANK = ':empty, :not(:has(> :not(:empty), img, svg, canvas, video, iframe))'

/** A group's steps when opened: one rounded list, a hairline between rows.
 * The hairline is an inset shadow, not a border, so it costs no layout: a
 * step's text holds exactly the same left edge as the group's heading, and
 * opening a group adds rows rather than an indent. */
export const TOOL_LIST_SX = {
  mt: '4px',
  borderRadius: '8px',
  overflow: 'hidden',
  boxShadow: (t: Theme) => `inset 0 0 0 1px ${t.palette.divider}`,
  '& > *': { minHeight: 32, display: 'flex', flexDirection: 'column', justifyContent: 'center', boxSizing: 'border-box' },
  // A step that renders nothing takes no row, and a divider sits only
  // between two rows that show something: no blank bands, no stray line
  // above the first.
  [`& > :is(${BLANK})`]: { display: 'none' },
  [`& > :not(${BLANK}) ~ :not(${BLANK})`]: { borderTop: 1, borderColor: 'divider' },
} as const
