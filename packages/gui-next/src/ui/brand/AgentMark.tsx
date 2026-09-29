import { memo, useId } from 'react'
import { useTheme } from '../ThemeProvider'
import { darken, lighten } from '../color'
import { CUT, EYE, GLINT, HELMET, type MarkPath } from './agentMarkPaths'

/** The mark's own box, cropped from the 1024 trace to the helmet. */
const VIEWBOX = '208 209 608 608'

function paths(list: readonly MarkPath[], fill: string) {
  return list.map(([d, x, y], i) => <path key={i} d={d} fill={fill} transform={`translate(${x},${y})`} />)
}

/**
 * The agent mark — the helmet with its eye — in an agent's colour.
 *
 * Every agent gets the same silhouette, so they read as one family; the eye
 * carries the agent's colour at full strength and the helmet a tint of it,
 * so each is recognisable at a glance. The helmet follows the theme's text
 * colour (a black helmet would vanish on the dark theme), and the gaps in
 * it are real holes (a mask), so the mark sits on any surface.
 */
function AgentMarkImpl({ color, size = 24, title }: { color: string; size?: number; title?: string }) {
  const theme = useTheme()
  const id = useId().replace(/:/g, '')
  const dark = theme.palette.mode === 'dark'
  // The eye has to stand out from a helmet of its own hue, as blue does
  // against black in the original: on the light helmet (dark theme) it goes
  // deeper, on the dark helmet (light theme) it stays bright.
  const helmet = `color-mix(in srgb, ${theme.palette.text.primary} ${dark ? 62 : 70}%, ${color})`
  const eye = dark ? darken(color, 0.25) : color
  const glint = lighten(color, 0.75)
  return (
    <svg
      width={size}
      height={size}
      viewBox={VIEWBOX}
      role={title ? 'img' : undefined}
      aria-hidden={title ? undefined : true}
      style={{ display: 'block', flexShrink: 0 }}
    >
      {title && <title>{title}</title>}
      <defs>
        <mask id={`m${id}`} maskUnits="userSpaceOnUse" x="0" y="0" width="1024" height="1024">
          <rect width="1024" height="1024" fill="black" />
          {paths(HELMET, 'white')}
          {paths(CUT, 'black')}
        </mask>
      </defs>
      <rect width="1024" height="1024" fill={helmet} mask={`url(#m${id})`} />
      {paths(EYE, eye)}
      {paths(GLINT, glint)}
    </svg>
  )
}

export const AgentMark = memo(AgentMarkImpl)
