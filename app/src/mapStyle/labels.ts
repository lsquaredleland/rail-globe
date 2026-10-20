import { FILL } from '../palette'
import { BOLD_STACK, L, REG_STACK, get, type RailCtx } from './shared'

/** line names along the track, line number badges, and track names */
export function addLineLabels({ source, add, src }: RailCtx) {
  // line names along the track: one label per line (from its strands), or the track's own name where no line is mapped
  const lblPaint = { 'text-color': '#33312C', 'text-halo-color': FILL.land, 'text-halo-width': 2, 'text-halo-blur': 0.3 }
  add({
    id: L(source, 'lbl-lines'),
    type: 'symbol',
    ...src,
    'source-layer': 'lines',
    minzoom: 11.5,
    layout: {
      'symbol-placement': 'line',
      'text-field': get('lb'),
      'text-font': REG_STACK,
      'text-size': ['interpolate', ['linear'], ['zoom'], 11.5, 10, 15, 12],
      'symbol-spacing': 320,
      'text-max-angle': 30,
      'text-padding': 3,
      'symbol-sort-key': get('i'),
      'text-letter-spacing': 0.02,
    },
    paint: lblPaint,
  })
  // line number badges (operator colour mode only): the line's ref in a badge of its colour, upright, repeated along the strand.
  // The badge image comes from the styleimagemissing handler (addBadgeImages), one per colour and text shade.
  add({
    id: L(source, 'badge'),
    type: 'symbol',
    ...src,
    'source-layer': 'lines',
    minzoom: 12.5,
    filter: ['has', 'rf'],
    layout: {
      visibility: 'none',
      'symbol-placement': 'line',
      'symbol-spacing': 300,
      'symbol-sort-key': get('i'),
      'icon-image': ['concat', 'badge|', get('c'), '|', ['case', ['has', 'rl'], 'd', 'l']],
      'icon-text-fit': 'both',
      'icon-text-fit-padding': [1, 3, 1, 3],
      'icon-rotation-alignment': 'viewport',
      'icon-pitch-alignment': 'viewport',
      'icon-padding': 4,
      'text-field': get('rf'),
      'text-font': BOLD_STACK,
      'text-size': ['interpolate', ['linear'], ['zoom'], 12.5, 10, 16, 13],
      'text-rotation-alignment': 'viewport',
      'text-pitch-alignment': 'viewport',
    },
    paint: { 'text-color': ['case', ['has', 'rl'], '#1E1E1C', '#FFFFFF'] },
  })
  add({
    id: L(source, 'lbl-track'),
    type: 'symbol',
    ...src,
    'source-layer': 'track',
    minzoom: 11.5,
    filter: ['all', ['has', 'n'], ['!', ['has', 'nl']], ['!', ['has', 'con']], ['!=', get('type'), 'yard'], ['!=', get('type'), 'freight']],
    layout: {
      'symbol-placement': 'line',
      'text-field': get('n'),
      'text-font': REG_STACK,
      'text-size': ['interpolate', ['linear'], ['zoom'], 11.5, 10, 15, 12],
      'symbol-spacing': 360,
      'text-max-angle': 30,
      'text-padding': 3,
      'text-letter-spacing': 0.02,
    },
    paint: lblPaint,
  })
}
