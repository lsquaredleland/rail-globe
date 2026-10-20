import type { LayerSpecification, Map as MLMap } from 'maplibre-gl'
import { THEME_COLORS } from '../palette'
import { TILES } from '../regions'
import { BOLD_STACK, L, REG_STACK, get, type Expr } from './shared'

/** Place names: only countries with data and the cities and towns that have a passenger station (`rail-extract places` writes places.json).
 *  Drawn above the rail so track does not cover them; colours come from `applyTheme`, the language from `applyState`. */
export const PLACE_LAYERS = ['place-country', 'place-city-1', 'place-city-2', 'place-city-3', 'place-town']
export const placeText = (lang: 'local' | 'en'): Expr => (lang === 'en' ? ['coalesce', get('en'), get('name')] : get('name')) as Expr
export function addPlaceLabels(map: MLMap, places: GeoJSON.FeatureCollection) {
  const t = THEME_COLORS.light
  map.addSource('places', { type: 'geojson', data: places })
  const sym = (id: string, filter: Expr, minzoom: number, maxzoom: number, layout: Record<string, unknown>, paint: Record<string, unknown> = {}) =>
    map.addLayer({
      id,
      type: 'symbol',
      source: 'places',
      minzoom,
      maxzoom,
      filter,
      layout: { 'text-field': placeText('local'), 'text-font': BOLD_STACK, 'symbol-sort-key': ['get', 'rank'], ...layout },
      paint: { 'text-color': t.label, 'text-halo-color': t.land, 'text-halo-width': 2, 'text-halo-blur': 0.4, ...paint },
    } as LayerSpecification)
  sym(
    'place-country',
    ['==', get('class'), 'country'] as Expr,
    1,
    8,
    {
      'text-size': ['interpolate', ['linear'], ['zoom'], 1, 11, 4, 16, 7, 22],
      'text-transform': 'uppercase',
      'text-letter-spacing': 0.14,
      'text-max-width': 8,
    },
    { 'text-opacity': ['interpolate', ['linear'], ['zoom'], 6, 1, 8, 0] }
  )
  // cities by rank (1 is the biggest): the big ones from far out, the rest as the map zooms in; the stations' own labels take over close in
  const city = (id: string, lo: number, hi: number, minzoom: number, size: number[]) =>
    sym(id, ['all', ['==', get('class'), 'city'], ['>=', get('rank'), lo], ['<=', get('rank'), hi]] as Expr, minzoom, 11, {
      'text-size': ['interpolate', ['linear'], ['zoom'], minzoom, size[0], 10, size[1]],
      'text-max-width': 7,
    })
  // MapLibre places the layers on top first, so they are added smallest first: where labels collide, the bigger place keeps its name
  sym('place-town', ['==', get('class'), 'town'] as Expr, 8.5, 11, {
    'text-size': ['interpolate', ['linear'], ['zoom'], 8.5, 10, 11, 12],
    'text-max-width': 7,
    'text-font': REG_STACK,
  })
  city('place-city-3', 8, 99, 6, [10.5, 14])
  city('place-city-2', 4, 7, 4, [11, 16])
  city('place-city-1', 0, 3, 2, [12, 19])
  // z9 and up: tier 1 and 2 station labels are copied above the place layers so they are placed first and a town or district
  // name cannot hide them (A10). Places stop at z11. The originals stop at z9 (tier 1) and never show (tier 2 starts at z9); below z9 nothing
  // changes. crossSourceCollisions stays on (docs/06).
  for (const n of ['lbl1', 'lbl2']) {
    const orig = map.getLayer(L(TILES, n))?.serialize()
    if (!orig) continue
    map.setLayerZoomRange(orig.id, orig.minzoom ?? 0, 9)
    map.addLayer({ ...orig, id: L(TILES, `${n}t`), minzoom: Math.max(orig.minzoom ?? 0, 9), maxzoom: 24 } as LayerSpecification)
  }
  // the logos go on top of the copies, so they are placed first and keep priority over the labels (adding the copies below the logos would put them below the places again)
  if (map.getLayer(L(TILES, 'logo'))) map.moveLayer(L(TILES, 'logo'))
}
/** The rows of places.json ([lon, lat, class, rank, name, name_en, n_stations]) as points. */
export function placesGeoJSON(rows: [number, number, string, number, string, string, number][]): GeoJSON.FeatureCollection {
  return {
    type: 'FeatureCollection',
    features: rows.map(([lon, lat, cls, rank, name, en, n]) => ({
      type: 'Feature',
      properties: { class: cls, rank, name, en, n },
      geometry: { type: 'Point', coordinates: [lon, lat] },
    })),
  }
}
