import { useCallback, useRef } from 'react'
import { Box, useTheme } from '../../../../ui'
import type { SciColors } from './dslCommon'
import { flattenSvg } from './symbols'

/** The colours a figure needs, from the app theme. */
export function useSciColors(): SciColors {
  const theme = useTheme()
  const dark = theme.palette.mode === 'dark'
  return {
    ink: theme.palette.text.primary,
    muted: theme.palette.text.secondary,
    line: theme.palette.divider,
    paper: theme.palette.background.paper,
    dark,
    accents: dark ? ['#e5b84a', '#e66767', '#3987e5', '#199e70'] : ['#eda100', '#e34948', '#2a78d6', '#1baf7a'],
  }
}

/**
 * Shows an SVG string the figure code made, and hands the owner a way to read
 * it back as a standalone file. The string is generated here from the user's
 * own script by our renderers, never taken from the network.
 */
export function useSvgFigure(ink: string) {
  const ref = useRef<HTMLDivElement | null>(null)
  const getSvg = useCallback((): string | null => {
    const svg = ref.current?.querySelector('svg')
    return svg ? flattenSvg(svg as unknown as SVGSVGElement, ink) : null
  }, [ink])
  return { ref, getSvg }
}

export function SvgFigure({ svg, large, holder }: { svg: string; large: boolean; holder?: React.Ref<HTMLDivElement> }) {
  return (
    <Box
      ref={holder}
      sx={{ p: 1, display: 'flex', justifyContent: 'center', '& svg': { display: 'block', maxWidth: '100%', height: 'auto', ...(large ? { width: '100%' } : {}) } }}
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  )
}

/**
 * Copies the styles the page's CSS gave an element (and its children) onto a
 * clone as attributes, so the clone still looks right once it is a file with
 * no stylesheet. Used for figures whose libraries style through classes.
 */
export function inlineComputedStyles(from: Element, to: Element) {
  const props = ['fill', 'fill-opacity', 'stroke', 'stroke-width', 'stroke-opacity', 'stroke-dasharray', 'opacity', 'font-size', 'font-family', 'font-weight', 'text-anchor']
  const cs = getComputedStyle(from)
  const style = props.map((p) => `${p}:${cs.getPropertyValue(p)}`).join(';')
  if (to instanceof SVGElement) to.setAttribute('style', `${to.getAttribute('style') ?? ''};${style}`)
  const a = from.children
  const b = to.children
  for (let i = 0; i < a.length && i < b.length; i++) inlineComputedStyles(a[i], b[i])
}
