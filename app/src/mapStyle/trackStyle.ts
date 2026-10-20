import { TYPE_COLOR, TYPE_WIDTH } from '../palette'
import { ALL_TYPES, byType, get, type Expr } from './shared'

/** layers that also have a high-speed twin (`<name>-hs`), see addRailSource */
export const TWINS = new Set(['track', 'strand', 'strand-casing'])
export const rank = byType((t) => (ALL_TYPES.length - ALL_TYPES.indexOf(t)))
/** Grey-out mode has one definition of "active": high-speed track, and the stations that high-speed rail serves.
 *  Track (open or under construction), track names and stations all test these two expressions, so they cannot drift apart. */
export const activeTrack: Expr = ['==', get('type'), 'high_speed'] as Expr
export const activeStation = (ty: Expr): Expr => ['any', ['==', get('hsr'), 1], ['in', ',high_speed,', ty]] as unknown as Expr
export const INACTIVE = '#D4D1C7'
/** the same on the dark map: track that is not active sinks into the ground instead of standing out as pale lines */
export const INACTIVE_DARK = '#383D45'
export const INACTIVE_WIDTH = 0.6
export const typeColor = byType((t) => TYPE_COLOR[t], 'type', TYPE_COLOR.other)

/** Rail is drawn at LOW_ZOOM_SHARE of its width at z7 and below, and grows to full width at z10 (the thick lines crowd the zoomed-out map) */
export const LOW_ZOOM_SHARE = 0.5

/** width that follows z10 / z12 / z15 stops (half of the z10 width at z7 and below), with an optional multiplier expression */
export function widthExpr(mult?: Expr): Expr {
  const stop = (i: number): Expr => {
    const w = byType((t) => TYPE_WIDTH[t][i]) as Expr
    return mult ? (['*', w, mult] as Expr) : w
  }
  const low = ['*', stop(0), LOW_ZOOM_SHARE] as Expr
  return ['interpolate', ['linear'], ['zoom'], 7, low, 10, stop(0), 12, stop(1), 15, stop(2)] as Expr
}
