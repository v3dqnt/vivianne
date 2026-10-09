import { CHART_SERIES } from '../../lib/chartPalette'
import type { PlotSpec } from './plotDsl'

/** Colour of each series: its own, or the chart palette in order. */
export function seriesColorOf(spec: PlotSpec, i: number, dark: boolean): string {
  const palette = CHART_SERIES[dark ? 'dark' : 'light']
  return spec.series[i].color ?? palette[i % palette.length]
}
