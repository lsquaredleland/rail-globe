import type { LayerSpecification, Map as MLMap } from 'maplibre-gl'
import { THEME_COLORS } from '../palette'
import { TILES } from '../regions'
import { BOLD_STACK, L, REG_STACK, get, type Expr } from './shared'

/** Place names: only countries with data and the cities and towns that have a passenger station (`rail-extract places` writes places.json).
 *  Drawn above the rail so track does not cover them; colours come from `applyTheme`, the language from `applyState`. */
export const PLACE_LAYERS = ['place-country', 'place-city-1', 'place-city-2', 'place-city-3', 'place-town']
export const placeText = (lang: 'local' | 'en'): Expr => (lang === 'en' ? ['coalesce', get('en'), get('name')] : get('name')) as Expr
let placeFeatures: GeoJSON.Feature[] = []
export function addPlaceLabels(map: MLMap, places: GeoJSON.FeatureCollection) {
  placeFeatures = places.features
  buildGrid()
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

/** A clicked China station's trains (cn12306): place names within 8 km of a stop stay lit, and a city's own name within 60 km of one of its stops; the rest fade
 *  to 25%. One expression per layer, set once; the layers' own opacity is put back by restorePlaces. */
const PLACE_FADE = 0.25
const placeOpacity = new Map<string, unknown>()
let placeGrid: Map<string, number[]> | null = null
let gridFor: GeoJSON.Feature[] | null = null
const cellOf = (lon: number, lat: number) => `${Math.floor(lon / 0.1)},${Math.floor(lat / 0.1)}`
const km = (a: [number, number], b: [number, number]) => Math.hypot((a[0] - b[0]) * Math.cos((b[1] * Math.PI) / 180) * 111, (a[1] - b[1]) * 111)
const placeAt = (i: number) => (placeFeatures[i].geometry as GeoJSON.Point).coordinates as [number, number]
function buildGrid() {
  gridFor = placeFeatures
  const grid = new Map<string, number[]>()
  placeFeatures.forEach((_, i) => {
    const k = cellOf(...placeAt(i))
    grid.set(k, [...(grid.get(k) ?? []), i])
  })
  placeGrid = grid
  return grid
}
export function fadePlaces(map: MLMap, stops: [number, number][], cities: string[]) {
  const grid = !placeGrid || gridFor !== placeFeatures ? buildGrid() : placeGrid
  const cells = new Map<string, [number, number][]>()
  for (const p of stops) cells.set(cellOf(...p), [...(cells.get(cellOf(...p)) ?? []), p])
  const lit = new Set<number>()
  for (const [k, pts] of cells) {
    const [cx, cy] = k.split(',').map(Number)
    for (let x = cx - 1; x <= cx + 1; x++)
      for (let y = cy - 1; y <= cy + 1; y++) for (const i of grid.get(`${x},${y}`) ?? []) if (!lit.has(i) && pts.some((p) => km(placeAt(i), p) <= 8)) lit.add(i)
  }
  const names = new Set(cities)
  placeFeatures.forEach((f, i) => {
    if (f.properties?.class === 'city' && names.has(String(f.properties.name).replace(/市$/, '')) && !lit.has(i) && stops.some((p) => km(placeAt(i), p) <= 60))
      lit.add(i)
  })
  const keys = [...lit].map((i) => `${placeFeatures[i].properties?.name}|${placeFeatures[i].properties?.rank}|${placeFeatures[i].properties?.n}`)
  const key = ['concat', get('name'), '|', ['to-string', get('rank')], '|', ['to-string', get('n')]]
  const on = ['in', key, ['literal', keys]]
  for (const id of PLACE_LAYERS) {
    if (!map.getLayer(id)) continue
    if (!placeOpacity.has(id)) placeOpacity.set(id, map.getPaintProperty(id, 'text-opacity'))
    const v = id === 'place-country' ? ['interpolate', ['linear'], ['zoom'], 6, ['case', on, 1, PLACE_FADE], 8, 0] : ['case', on, 1, PLACE_FADE]
    map.setPaintProperty(id, 'text-opacity', v as never, { validate: false })
  }
}
export function restorePlaces(map: MLMap) {
  for (const [id, v] of placeOpacity) if (map.getLayer(id)) map.setPaintProperty(id, 'text-opacity', v as never)
  placeOpacity.clear()
}
