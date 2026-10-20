import { get, type Expr } from './shared'
import C from '../../../shared/constants.json'

/** Speed colour ramp (km/h, lower bound of each band). Track with no usable maxspeed is drawn pale grey.
 *  The lower bounds come from shared/constants.json (the pipeline cuts the same bands); colours and labels live here. */
const SPEED_STYLE: { color: string; label: string }[] = [
  { color: '#4A0D2A', label: '400+ km/h' },
  { color: '#80102E', label: '350-399' },
  { color: '#B3123A', label: '300-349' },
  { color: '#D81B60', label: '250-299' },
  { color: '#E8590C', label: '200-249' },
  { color: '#E0A100', label: '160-199' },
  { color: '#6FA33B', label: '120-159' },
  { color: '#2F7FA8', label: '80-119' },
  { color: '#7A6FA8', label: 'under 80' },
]
export const SPEED_BANDS: { min: number; color: string; label: string }[] = C.speed_band_mins.map((min, i) => ({ min, ...SPEED_STYLE[i] }))
/** Speed-band switch value for track with no usable maxspeed */
export const SPEED_NONE = -1
/** Filter terms hiding the speed bands in `off` (band lower bounds, SPEED_NONE: no maxspeed); only speed mode has them */
export function notHiddenSpeed(off: number[]): Expr[] {
  if (off.length === 0) return []
  const terms = off.map((m) => {
    if (m === SPEED_NONE) return ['!', ['has', 'kmh']]
    const i = SPEED_BANDS.findIndex((b) => b.min === m)
    return i > 0 ? ['all', ['has', 'kmh'], ['>=', get('kmh'), m], ['<', get('kmh'), SPEED_BANDS[i - 1].min]] : ['all', ['has', 'kmh'], ['>=', get('kmh'), m]]
  })
  return [['!', ['any', ...terms]] as unknown as Expr]
}
/** the band a speed (or an overview band's lower bound) falls in */
export const speedBand = (kmh: number) => SPEED_BANDS.find((b) => kmh >= b.min) ?? SPEED_BANDS[SPEED_BANDS.length - 1]
export const SPEED_UNKNOWN = '#BDB9AE'
export const speedColor: Expr = ['case', ['has', 'kmh'],
  ['step', get('kmh'), SPEED_BANDS[SPEED_BANDS.length - 1].color, ...[...SPEED_BANDS].reverse().slice(1).flatMap((b) => [b.min, b.color])],
  SPEED_UNKNOWN] as Expr
