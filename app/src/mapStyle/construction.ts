import { TYPE_WIDTH, type RailType } from '../palette'
import { byType, get, type Expr, type RailCtx } from './shared'
import { LOW_ZOOM_SHARE, rank, typeColor } from './trackStyle'
import C from '../../../shared/constants.json'

// Under-construction and proposed track: dashes cut as real geometry in the pipeline (tile layer `con_dash`, tiles.rs), not MapLibre's line-dasharray,
// which restarts at every tile edge and stretches between whole zooms. Parallel tracks are one line, so the pattern runs along a stretch.
// Construction at full opacity, proposed fainter. Hover, click and highlight stay on the per-way `track` features (`hit-con`, `hit-prop`).
const { first: CON_BAND_FIRST, step: CON_BAND_STEP, last: CON_BAND_LAST } = C.con_band // shared/constants.json: the bands tiles.rs cuts con_dash for
/** One layer per band (a zoom range of CON_BAND_STEP), because a zoom-dependent opacity on a data-driven property is evaluated at whole zooms
 *  and blended between them, so it cannot show one band exactly. Layer `<name>-b<k>`, filter b == k. */
export const CON_BANDS = Array.from({ length: Math.round((CON_BAND_LAST - CON_BAND_FIRST) / CON_BAND_STEP) + 1 }, (_, k) => k)
const conBandRange = (k: number): { minzoom: number; maxzoom: number } => ({
  minzoom: CON_BAND_FIRST + k * CON_BAND_STEP,
  maxzoom: k === CON_BANDS.length - 1 ? 24 : CON_BAND_FIRST + (k + 1) * CON_BAND_STEP,
})
const CON_DASH_OPACITY = 1
const PROP_DASH_OPACITY = 0.6
/** [zoom, minimum px, TYPE_WIDTH index, share of the width] of the construction width ramp; width = share * max(minimum, type width * 1.2) */
const CON_STOPS: [number, number, number, number][] = [
  [4, 2.6, 0, LOW_ZOOM_SHARE],
  [7, 2.6, 0, LOW_ZOOM_SHARE],
  [10, 1.8, 0, 1],
  [12, 2.2, 1, 1],
  [15, 3, 2, 1],
]
const conStopWidth = (t: RailType, k: number) => CON_STOPS[k][3] * Math.max(CON_STOPS[k][1], TYPE_WIDTH[t][CON_STOPS[k][2]] * 1.2)
export const conWidth = (mult?: Expr): Expr =>
  [
    'interpolate',
    ['linear'],
    ['zoom'],
    ...CON_STOPS.flatMap((_, k) => {
      const w = byType((t) => conStopWidth(t, k), 'type', conStopWidth('other', k)) as Expr
      return [CON_STOPS[k][0], mult ? ['*', w, mult] : w]
    }),
  ] as Expr
/** under-construction colour: the colour rule of open track (`other` is drawn dark grey) */
export const conColor: Expr = ['match', get('type'), 'other', '#45423C', typeColor]

/** under construction and proposed: dashes from `con_dash` (one layer per zoom band) */
export function addConstructionLayers({ src, trackLayer }: RailCtx) {
  // An explicit sort key wins over hsLast; `rank` already puts high_speed last.
  // One layer per kind, so a switch only flips visibility: changing a filter makes MapLibre re-evaluate every track piece in the loaded tiles.
  // Yard track is left out of the tiles; freight follows the Freight switch (applyRegion). Proposed starts hidden (its switch is off by default).
  for (const [name, opacity] of [
    ['construction', CON_DASH_OPACITY],
    ['proposed', PROP_DASH_OPACITY],
  ] as const) {
    const kind: Expr = name === 'proposed' ? ['has', 'prop'] : ['!', ['has', 'prop']]
    const layout = { 'line-cap': 'butt', 'line-join': 'round', 'line-sort-key': rank, visibility: name === 'proposed' ? 'none' : 'visible' }
    const conPaint = { 'line-color': conColor, 'line-width': conWidth(), 'line-opacity': opacity }
    for (const k of CON_BANDS) {
      trackLayer(`${name}-b${k}`, {
        type: 'line',
        ...src,
        'source-layer': 'con_dash',
        ...conBandRange(k),
        filter: ['all', kind, ['==', get('b'), k]],
        layout,
        paint: conPaint,
      })
    }
  }
}
