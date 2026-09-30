/**
 * Categorical palette for in-chat charts.
 *
 * Values are the dataviz skill's validated reference instance, re-validated
 * against THIS app's real chart surfaces rather than assumed to carry over:
 *
 *   dark  (paper #141414) — all five checks PASS
 *   light (paper #ffffff) — PASS, with a contrast WARN on aqua/yellow/magenta
 *
 * That light-mode WARN is why `ChartBlock` always renders a legend and offers a
 * table view: the skill treats sub-3:1 contrast as an obligation to provide
 * relief (visible labels or a table), not as something to dismiss.
 *
 * Slot ORDER is the colour-blindness safety mechanism, not decoration — the
 * order was chosen so every *adjacent* pair clears the CVD floor. Assign slots
 * in order and never cycle: a 9th series folds into "Other" instead.
 */
export const CHART_SERIES = {
  light: ['#2a78d6', '#eb6834', '#1baf7a', '#eda100', '#e87ba4', '#008300', '#4a3aa7', '#e34948'],
  dark: ['#3987e5', '#d95926', '#199e70', '#c98500', '#d55181', '#008300', '#9085e9', '#e66767'],
} as const

/** Slots that clear the ALL-PAIRS floors, not just adjacent ones.
 *
 * Scatter puts every series against every other simultaneously, so it needs
 * the stricter gate. Only the first three slots pass it in both modes; past
 * three the fourth puts yellow next to orange, which fails. Scatter therefore
 * caps at 3 series and folds the rest. */
export const ALL_PAIRS_SAFE_SLOTS = 3

/** Highest number of series that also get direct labels. Beyond this the
 * labels collide and the legend carries identity on its own. */
export const DIRECT_LABEL_MAX = 4

export function seriesColors(mode: 'light' | 'dark'): readonly string[] {
  return CHART_SERIES[mode === 'dark' ? 'dark' : 'light']
}
