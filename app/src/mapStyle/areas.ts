import { FILL } from '../palette'
import { BOLD_STACK, L, REG_STACK, get, type RailCtx } from './shared'

/** airport outline and terminals (drawn as infrastructure), ground, yards, buildings and platforms: the infrastructure under the track */
export function addAreaLayers({ source, add, src }: RailCtx) {
  add({ id: L(source, 'airport-fill'), type: 'fill', ...src, 'source-layer': 'airports', minzoom: 8, filter: ['==', get('k'), 'aerodrome'],
    paint: { 'fill-color': '#E6E0D2', 'fill-opacity': 0.85 } })
  add({ id: L(source, 'airport-line'), type: 'line', ...src, 'source-layer': 'airports', minzoom: 8, filter: ['==', get('k'), 'aerodrome'],
    layout: { 'line-join': 'round' },
    paint: { 'line-color': '#9A927E', 'line-width': ['interpolate', ['linear'], ['zoom'], 8, 0.8, 12, 1.4, 15, 2.2] } })
  add({ id: L(source, 'airport-terminal'), type: 'fill', ...src, 'source-layer': 'airports', minzoom: 12, filter: ['==', get('k'), 'terminal'],
    paint: { 'fill-color': '#CBC1AD', 'fill-outline-color': '#A99F8A' } })

  add({ id: L(source, 'ground'), type: 'fill', ...src, 'source-layer': 'areas', minzoom: 12, filter: ['==', get('k'), 'land'],
    paint: { 'fill-color': FILL.ground, 'fill-opacity': 0.9 } })
  add({ id: L(source, 'yard-fill'), type: 'fill', ...src, 'source-layer': 'areas', minzoom: 12, filter: ['==', get('k'), 'yard'],
    paint: { 'fill-color': FILL.yard } })
  add({ id: L(source, 'ground-edge'), type: 'line', ...src, 'source-layer': 'areas', minzoom: 12,
    paint: { 'line-color': FILL.groundEdge, 'line-width': 0.8 } })
  add({ id: L(source, 'buildings'), type: 'fill', ...src, 'source-layer': 'buildings', minzoom: 13,
    paint: { 'fill-color': FILL.building, 'fill-outline-color': FILL.buildingEdge } })
  add({ id: L(source, 'platforms-fill'), type: 'fill', ...src, 'source-layer': 'platforms', minzoom: 14, filter: ['==', ['geometry-type'], 'Polygon'],
    paint: { 'fill-color': FILL.platform } })
  add({ id: L(source, 'platforms-line'), type: 'line', ...src, 'source-layer': 'platforms', minzoom: 14, filter: ['==', ['geometry-type'], 'LineString'],
    paint: { 'line-color': FILL.platform, 'line-width': 2 } })
}

/** outline label for small airfields, and the plane icon with the IATA code of commercial airports */
export function addAirportLabels({ source, add, src }: RailCtx) {
  // outline label for small airfields; commercial airports get one icon point below
  add({ id: L(source, 'airport-label'), type: 'symbol', ...src, 'source-layer': 'airports', minzoom: 8, filter: ['all', ['==', get('k'), 'aerodrome'], ['!=', get('com'), 1]],
    layout: { 'text-field': get('lb'), 'text-font': REG_STACK, 'text-size': ['interpolate', ['linear'], ['zoom'], 8, 10, 14, 12],
      'text-letter-spacing': 0.08, 'text-max-width': 8, 'text-padding': 4 },
    paint: { 'text-color': '#6B665B', 'text-halo-color': FILL.land, 'text-halo-width': 1.6 } })
  // commercial airports: one plane icon at the centre of the outline with the IATA code under it
  add({ id: L(source, 'airport-icon'), type: 'symbol', ...src, 'source-layer': 'airport_pts', minzoom: 8,
    layout: { 'icon-image': 'airport', 'icon-allow-overlap': true, 'icon-ignore-placement': true, 'icon-size': ['interpolate', ['linear'], ['zoom'], 8, 0.6, 12, 0.85, 15, 1.05],
      'text-field': get('lb'), 'text-font': BOLD_STACK, 'text-size': ['interpolate', ['linear'], ['zoom'], 8, 10, 14, 13],
      'text-anchor': 'top', 'text-offset': [0, 0.9], 'text-letter-spacing': 0.06, 'text-padding': 4, 'text-allow-overlap': true, 'text-ignore-placement': true,
      'symbol-sort-key': 0 },
    paint: { 'text-color': '#4A463D', 'text-halo-color': FILL.land, 'text-halo-width': 1.8 } })
}

/** polygon layers that answer hovers, most specific first */
export const areaLayers = (slug: string) => ['platforms-fill', 'platforms-line', 'airport-terminal', 'buildings', 'yard-fill', 'ground', 'airport-fill'].map((n) => L(slug, n))
