import type { ExpressionSpecification, GeoJSONSource, LayerSpecification, Map as MLMap, StyleSpecification } from 'maplibre-gl'
import { STATION_ZOOM } from './config'
import { FILL, INK, TYPE_COLOR, TYPE_WIDTH, TYPE_ORDER, type RailType } from './palette'
import { regionOf, TILES } from './regions'
import { boundaryUrl, loadLineHighlight, loadNameHighlight, pmtilesUrl, type Highlight } from './data'

export type ColorMode = 'type' | 'operator' | 'speed'
export interface MapState { mode: ColorMode; construction: boolean; proposed: boolean; freight: boolean; hidden: string[]; hiddenSpeed: number[]; hsrOnly: boolean; lang: 'local' | 'en' }

const OFM = 'https://tiles.openfreemap.org/planet'
export const ATTRIBUTION = '© <a href="https://www.openstreetmap.org/copyright" target="_blank" rel="noopener">OpenStreetMap</a> contributors'

type Expr = ExpressionSpecification
const get = (k: string): Expr => ['get', k]
const ALL_TYPES = [...TYPE_ORDER, 'freight', 'yard'] as RailType[]

/** match on the track `type` property */
function byType(f: (t: RailType) => number | string, key = 'type', fallback: number | string = 0): Expr {
  const out: unknown[] = ['match', get(key)]
  for (const t of ALL_TYPES) out.push(t, f(t))
  out.push(fallback)
  return out as Expr
}

/** layers that also have a high-speed twin (`<name>-hs`), see addRailSource */
const TWINS = new Set(['track', 'strand', 'strand-casing'])
const rank = byType((t) => (ALL_TYPES.length - ALL_TYPES.indexOf(t)))
/** Speed colour ramp (km/h, lower bound of each band). Track with no usable maxspeed is drawn pale grey. */
export const SPEED_BANDS: { min: number; color: string; label: string }[] = [
  { min: 400, color: '#4A0D2A', label: '400+ km/h' },
  { min: 350, color: '#80102E', label: '350-399' },
  { min: 300, color: '#B3123A', label: '300-349' },
  { min: 250, color: '#D81B60', label: '250-299' },
  { min: 200, color: '#E8590C', label: '200-249' },
  { min: 160, color: '#E0A100', label: '160-199' },
  { min: 120, color: '#6FA33B', label: '120-159' },
  { min: 80, color: '#2F7FA8', label: '80-119' },
  { min: 0, color: '#7A6FA8', label: 'under 80' },
]
/** Speed-band switch value for track with no usable maxspeed */
export const SPEED_NONE = -1
/** Filter terms hiding the speed bands in `off` (band lower bounds, SPEED_NONE: no maxspeed); only speed mode has them */
function notHiddenSpeed(off: number[]): Expr[] {
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
const speedColor: Expr = ['case', ['has', 'kmh'],
  ['step', get('kmh'), SPEED_BANDS[SPEED_BANDS.length - 1].color, ...[...SPEED_BANDS].reverse().slice(1).flatMap((b) => [b.min, b.color])],
  SPEED_UNKNOWN] as Expr
/** Grey-out mode has one definition of "active": high-speed track, and the stations that high-speed rail serves.
 *  Track (open or under construction), track names and stations all test these two expressions, so they cannot drift apart. */
const activeTrack: Expr = ['==', get('type'), 'high_speed'] as Expr
const activeStation = (ty: Expr): Expr => ['any', ['==', get('hsr'), 1], ['in', ',high_speed,', ty]] as unknown as Expr
export const INACTIVE = '#D4D1C7'
const INACTIVE_WIDTH = 0.6
const typeColor = byType((t) => TYPE_COLOR[t], 'type', TYPE_COLOR.other)

/** width that follows z10 / z12 / z15 stops, with an optional multiplier expression */
function widthExpr(mult?: Expr): Expr {
  const stop = (i: number): Expr => {
    const w = byType((t) => TYPE_WIDTH[t][i]) as Expr
    return mult ? (['*', w, mult] as Expr) : w
  }
  return ['interpolate', ['linear'], ['zoom'], 10, stop(0), 12, stop(1), 15, stop(2)] as Expr
}

/** label text: the English field when the language is English and the tile has one, else the local field */
const nameExpr = (lang: 'local' | 'en', local: string, en: string): Expr =>
  (lang === 'en' ? ['case', ['all', ['has', en], ['!=', get(en), '']], get(en), get(local)] : get(local)) as Expr

const L = (slug: string, name: string) => `${slug}:${name}`

export function baseStyle(): StyleSpecification {
  const sansR = ['Noto Sans Regular']
  const place = (cls: string[], minzoom: number, size: number[]): LayerSpecification => ({
    id: `place-${cls.join('-')}`, type: 'symbol', source: 'ofm', 'source-layer': 'place', minzoom,
    filter: ['in', ['get', 'class'], ['literal', cls]],
    layout: {
      'text-field': ['coalesce', ['get', 'name'], ['get', 'name:latin']], 'text-font': sansR,
      'text-size': ['interpolate', ['linear'], ['zoom'], minzoom, size[0], 14, size[1]], 'text-max-width': 8,
    },
    paint: { 'text-color': FILL.place, 'text-halo-color': FILL.land, 'text-halo-width': 1.6 },
  })
  return {
    version: 8,
    glyphs: 'https://tiles.openfreemap.org/fonts/{fontstack}/{range}.pbf',
    sources: { ofm: { type: 'vector', url: OFM, attribution: '' } },
    layers: [
      { id: 'background', type: 'background', paint: { 'background-color': FILL.land } },
      { id: 'park', type: 'fill', source: 'ofm', 'source-layer': 'park', paint: { 'fill-color': FILL.park } },
      { id: 'water', type: 'fill', source: 'ofm', 'source-layer': 'water', paint: { 'fill-color': FILL.water } },
      {
        id: 'waterway', type: 'line', source: 'ofm', 'source-layer': 'waterway', minzoom: 9,
        paint: { 'line-color': FILL.water, 'line-width': ['interpolate', ['linear'], ['zoom'], 9, 0.4, 14, 1.6] },
      },
      { id: 'aeroway-apron', type: 'fill', source: 'ofm', 'source-layer': 'aeroway', minzoom: 10,
        filter: ['all', ['==', ['geometry-type'], 'Polygon'], ['in', ['get', 'class'], ['literal', ['apron', 'runway', 'taxiway', 'helipad']]]],
        paint: { 'fill-color': '#D9D3C4', 'fill-opacity': 0.9 } },
      { id: 'aeroway-taxiway', type: 'line', source: 'ofm', 'source-layer': 'aeroway', minzoom: 10,
        filter: ['all', ['==', ['geometry-type'], 'LineString'], ['==', ['get', 'class'], 'taxiway']],
        paint: { 'line-color': '#BDB6A4', 'line-width': ['interpolate', ['linear'], ['zoom'], 10, 0.5, 14, 2.4, 17, 8] } },
      { id: 'aeroway-runway', type: 'line', source: 'ofm', 'source-layer': 'aeroway', minzoom: 10,
        filter: ['all', ['==', ['geometry-type'], 'LineString'], ['==', ['get', 'class'], 'runway']],
        layout: { 'line-cap': 'butt' },
        paint: { 'line-color': '#A49C88', 'line-width': ['interpolate', ['linear'], ['zoom'], 10, 1.6, 12, 3, 14, 7, 17, 30] } },
      {
        id: 'road-major', type: 'line', source: 'ofm', 'source-layer': 'transportation', minzoom: 7,
        filter: ['all', ['in', ['get', 'class'], ['literal', ['motorway', 'trunk', 'primary']]], ['!=', ['get', 'brunnel'], 'tunnel']],
        layout: { 'line-cap': 'round', 'line-join': 'round' },
        paint: { 'line-color': FILL.road, 'line-width': ['interpolate', ['linear'], ['zoom'], 7, 0.5, 12, 1.4, 16, 3.5] },
      },
      {
        id: 'road-secondary', type: 'line', source: 'ofm', 'source-layer': 'transportation', minzoom: 11,
        filter: ['all', ['==', ['get', 'class'], 'secondary'], ['!=', ['get', 'brunnel'], 'tunnel']],
        layout: { 'line-cap': 'round', 'line-join': 'round' },
        paint: { 'line-color': FILL.road, 'line-width': ['interpolate', ['linear'], ['zoom'], 11, 0.4, 16, 2.4] },
      },
      {
        id: 'admin', type: 'line', source: 'ofm', 'source-layer': 'boundary', minzoom: 3,
        filter: ['all', ['<=', ['get', 'admin_level'], 4], ['!=', ['get', 'maritime'], 1]],
        layout: { 'line-join': 'round' },
        paint: { 'line-color': FILL.boundary, 'line-width': 0.8, 'line-dasharray': [6, 5] },
      },
      place(['city'], 4, [11, 15]),
      place(['town'], 9, [11, 13]),
      place(['suburb', 'quarter', 'neighbourhood'], 12, [10.5, 12]),
    ],
  }
}

/** A region's faint outline (every region, also those drawn from a bigger region's tiles). Added before the rail layers. */
export function addBoundary(map: MLMap, slug: string) {
  map.addSource(L(slug, 'boundary'), { type: 'geojson', data: boundaryUrl(slug) })
  map.addLayer({ id: L(slug, 'boundary'), type: 'line', source: L(slug, 'boundary'), layout: { 'line-join': 'round' },
    paint: { 'line-color': '#B9B5AD', 'line-width': 1.1, 'line-dasharray': [6, 5], 'line-opacity': 0.8 } })
}

/** All rail layers of the tile source `source` (TILES), stacked in drawing order; ids are `<source>:<name>`. */
export function addRailSource(map: MLMap, source: string) {
  // a track piece's OSM id ("way/123") is its feature id, so a highlighted line can be flagged with feature state
  map.addSource(source, { type: 'vector', url: pmtilesUrl(source), attribution: ATTRIBUTION, promoteId: { track: 'id' } })
  const add = (l: Record<string, unknown>): void => { map.addLayer(l as unknown as LayerSpecification) }
  const src = { source } as const

  // airport outline and terminals, drawn as infrastructure
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

  const roundJoin = { 'line-cap': 'round', 'line-join': 'round' } as const
  // High-speed rail is drawn on top of other rail. Inside a layer a line-sort-key does it (high-speed sorts last). Only the layers
  // in TWINS also get a "-hs" twin added after all the others (below only the station symbols): the track, because the white
  // high-speed casing must sit between it and the other rail, and the strands, which the casing and the high-speed track
  // would otherwise cover. The base layer then excludes high-speed, the twin holds only high-speed.
  const top: LayerSpecification[] = []
  const hsLast = (typeKey: string): Expr => ['case', ['==', get(typeKey), 'high_speed'], 1, 0] as Expr
  const trackLayer = (name: string, spec: Record<string, unknown>, typeKey = 'type') => {
    const f = spec.filter as Expr | undefined
    const id = L(source, name)
    if (!TWINS.has(name)) {
      const layout = spec.layout as Record<string, unknown>
      add({ ...spec, id, layout: { 'line-sort-key': hsLast(typeKey), ...layout } })
      return
/** The outline of the area in view (the "Outline area in view" switch): a faint line with a soft inner shade, under the rail. Fed by `setOutline`. */
export function addOutline(map: MLMap) {
  map.addSource('area-outline', { type: 'geojson', data: { type: 'FeatureCollection', features: [] } })
  map.addLayer({ id: 'area-outline-shade', type: 'line', source: 'area-outline', layout: { 'line-join': 'round' },
    paint: { 'line-color': '#1E1E1C', 'line-width': 12, 'line-blur': 10, 'line-offset': 5, 'line-opacity': 0.05 } })
  map.addLayer({ id: 'area-outline', type: 'line', source: 'area-outline', layout: { 'line-join': 'round' },
    paint: { 'line-color': '#1E1E1C', 'line-width': 1.3, 'line-opacity': 0.32 } })
}
export function setOutline(map: MLMap, geometry: GeoJSON.Geometry | null) {
  ;(map.getSource('area-outline') as GeoJSONSource | undefined)?.setData({ type: 'FeatureCollection', features: geometry ? [{ type: 'Feature', properties: {}, geometry }] : [] })
}

    }
    add({ ...spec, id, filter: f ? ['all', ['!=', get(typeKey), 'high_speed'], f] : ['!=', get(typeKey), 'high_speed'] } as unknown as LayerSpecification)
    top.push({ ...spec, id: `${id}-hs`, filter: f ? ['all', ['==', get(typeKey), 'high_speed'], f] : ['==', get(typeKey), 'high_speed'] } as unknown as LayerSpecification)
  }
  // highlight layers are added last (after a veil, below), whatever order they are declared in
  const late: LayerSpecification[] = []
  const hlLayer = (spec: Record<string, unknown>) => {
    late.push({ ...spec, layout: { 'line-sort-key': hsLast('type'), ...(spec.layout as Record<string, unknown>) } } as unknown as LayerSpecification)
  }
  add({ id: L(source, 'yard'), type: 'line', ...src, 'source-layer': 'track', minzoom: 13, filter: ['==', get('type'), 'yard'],
    layout: roundJoin, paint: { 'line-color': TYPE_COLOR.yard, 'line-width': widthExpr() } })
  add({ id: L(source, 'freight'), type: 'line', ...src, 'source-layer': 'track', minzoom: 8,
    filter: ['all', ['==', get('type'), 'freight'], ['!', ['has', 'con']]],
    layout: { ...roundJoin, visibility: 'none' }, paint: { 'line-color': TYPE_COLOR.freight, 'line-width': widthExpr() } })
  top.push({ id: L(source, 'casing'), type: 'line', ...src, 'source-layer': 'track', filter: ['all', ['==', get('type'), 'high_speed'], ['!', ['has', 'con']]],
    layout: roundJoin,
    paint: { 'line-color': '#FFFFFF', 'line-opacity': 0.9, 'line-gap-width': 0,
      'line-width': ['interpolate', ['linear'], ['zoom'], 10, 6, 12, 7.5, 15, 9] } } as LayerSpecification)
  trackLayer('track', { type: 'line', ...src, 'source-layer': 'track', minzoom: 0,
    filter: ['all', ['!', ['has', 'con']], ['!=', get('type'), 'yard'], ['!=', get('type'), 'freight']],
    layout: { ...roundJoin, 'line-sort-key': rank },
    paint: { 'line-color': typeColor, 'line-width': widthExpr(), 'line-opacity': 1 } })

  // under construction: drawn above the track on a white casing so the dashes read, never thinner than 2.6 px zoomed out (1.8 from z10) and with long dashes so simplified overview geometry keeps them
  const conW = (extra: number): Expr => ['interpolate', ['linear'], ['zoom'],
    4, ['+', ['max', 2.6, ['*', byType((t) => TYPE_WIDTH[t][0]), 1.2]], extra],
    10, ['+', ['max', 1.8, ['*', byType((t) => TYPE_WIDTH[t][0]), 1.2]], extra], 12, ['+', ['max', 2.2, ['*', byType((t) => TYPE_WIDTH[t][1]), 1.2]], extra],
    15, ['+', ['max', 3, ['*', byType((t) => TYPE_WIDTH[t][2]), 1.2]], extra]] as Expr
  trackLayer('construction-casing', { type: 'line', ...src, 'source-layer': 'track', filter: ['has', 'con'],
    // an explicit sort key wins over hsLast; `rank` already puts high_speed last
    layout: { 'line-cap': 'butt', 'line-join': 'round', 'line-sort-key': rank }, paint: { 'line-color': '#FFFFFF', 'line-opacity': 0.85, 'line-width': conW(2) } })
  trackLayer('construction', { type: 'line', ...src, 'source-layer': 'track', filter: ['has', 'con'],
    layout: { 'line-cap': 'butt', 'line-join': 'round', 'line-sort-key': rank },
    paint: { 'line-color': ['match', get('type'), 'other', '#45423C', typeColor], 'line-width': conW(0), 'line-dasharray': [3, 2] } })

  // highlight for the hovered or selected line: the same line drawn wider in its own colour, no outline
  const hlW: Expr = ['interpolate', ['linear'], ['zoom'],
    8, ['+', ['*', byType((t) => TYPE_WIDTH[t][0]), 1.8], 2], 12, ['+', ['*', byType((t) => TYPE_WIDTH[t][1]), 1.8], 2.5],
    15, ['+', ['*', byType((t) => TYPE_WIDTH[t][2]), 1.8], 3]] as Expr
  // The highlight layers come after the casing, so no twin is needed; high-speed sorts last within each.
  // z8 and closer: the track pieces flagged with feature state by highlightLine (no tile reload, no copying geometry)
  hlLayer({ id: L(source, 'hl-fs'), type: 'line', ...src, 'source-layer': 'track', minzoom: 8,
    layout: { 'line-cap': 'round', 'line-join': 'round' },
    paint: { 'line-color': typeColor, 'line-width': hlW, 'line-opacity': ['case', ['boolean', ['feature-state', 'hl'], false], 1, 0] } })
  // below z8 the merged overview has no per-piece ids: the line's simplified geometry from its side file
  map.addSource(L(source, 'hl'), { type: 'geojson', data: EMPTY })
  hlLayer({ id: L(source, 'hl'), type: 'line', source: L(source, 'hl'), maxzoom: 8,
    layout: { 'line-cap': 'round', 'line-join': 'round' }, paint: { 'line-color': typeColor, 'line-width': hlW } })

  // operator-colour strands (side by side from z12)
  // strands: spacing and width shrink once more than 7 lines share a piece of track, so terminal throats stay readable
  const U: Expr = ['-', get('i'), ['/', ['-', get('n'), 1], 2]]
  const K: Expr = ['min', 1, ['/', 7, get('n')]]
  // one thickness per rail type in every colour mode: a strand is as wide as the track it replaces (TYPE_WIDTH), shrunk by K on crowded track
  const strandW = (extra = 0): Expr => ['interpolate', ['linear'], ['zoom'],
    12, ['+', ['*', byType((t) => TYPE_WIDTH[t][1], 't', TYPE_WIDTH.other[1]), K], extra], 15, ['+', ['*', byType((t) => TYPE_WIDTH[t][2], 't', TYPE_WIDTH.other[2]), K], extra]] as Expr
  const offset: Expr = ['interpolate', ['linear'], ['zoom'], 12, ['*', ['*', U, K], 3.5], 15, ['*', ['*', U, K], 5.5]] as Expr
  trackLayer('strand-casing', { type: 'line', ...src, 'source-layer': 'lines', minzoom: 12,
    layout: { ...roundJoin, visibility: 'none' },
    paint: { 'line-color': '#1E1E1C', 'line-opacity': 0.28, 'line-width': strandW(1), 'line-offset': offset } }, 't')
  trackLayer('strand', { type: 'line', ...src, 'source-layer': 'lines', minzoom: 12,
    layout: { ...roundJoin, visibility: 'none' },
    paint: { 'line-color': ['coalesce', get('c'), byType((t) => TYPE_COLOR[t], 't', TYPE_COLOR.other)],
      'line-opacity': ['case', ['has', 'c'], 1, 0.6], 'line-width': strandW(), 'line-offset': offset } }, 't')

  top.forEach((l) => add(l)) // high-speed twins and casing: above every other rail layer
  // Focus veil: while a line is selected the other lines fade. A background layer at constant opacity does it with a repaint only:
  // changing a data-driven paint property (the obvious way) makes MapLibre re-parse and re-upload every loaded tile.
  add({ id: L(source, 'veil'), type: 'background', paint: { 'background-color': FILL.land, 'background-opacity': 0, 'background-opacity-transition': { duration: 120, delay: 0 } } })
  late.forEach((l) => add(l))   // the highlight, above the veil

  // invisible, wider hit target for track tooltips
  // at every zoom: below z8 the track tiles hold the merged overview, which keeps the line ids and railway names a click needs
  add({ id: L(source, 'hit'), type: 'line', ...src, 'source-layer': 'track', filter: ['!=', get('type'), 'yard'],
    layout: roundJoin, paint: { 'line-color': '#000000', 'line-opacity': 0, 'line-width': 12 } })

  // line names along the track: one label per line (from its strands), or the track's own name where no line is mapped
  const lblPaint = { 'text-color': '#33312C', 'text-halo-color': FILL.land, 'text-halo-width': 2, 'text-halo-blur': 0.3 }
  add({ id: L(source, 'lbl-lines'), type: 'symbol', ...src, 'source-layer': 'lines', minzoom: 11.5,
    layout: { 'symbol-placement': 'line', 'text-field': get('lb'), 'text-font': ['Noto Sans Regular'],
      'text-size': ['interpolate', ['linear'], ['zoom'], 11.5, 10, 15, 12], 'symbol-spacing': 320, 'text-max-angle': 30,
      'text-padding': 3, 'symbol-sort-key': get('i'), 'text-letter-spacing': 0.02 },
    paint: lblPaint })
  add({ id: L(source, 'lbl-track'), type: 'symbol', ...src, 'source-layer': 'track', minzoom: 11.5,
    filter: ['all', ['has', 'n'], ['!', ['has', 'nl']], ['!', ['has', 'con']], ['!=', get('type'), 'yard'], ['!=', get('type'), 'freight']],
    layout: { 'symbol-placement': 'line', 'text-field': get('n'), 'text-font': ['Noto Sans Regular'],
      'text-size': ['interpolate', ['linear'], ['zoom'], 11.5, 10, 15, 12], 'symbol-spacing': 360, 'text-max-angle': 30,
      'text-padding': 3, 'text-letter-spacing': 0.02 },
    paint: lblPaint })

  // yard track gets its own hit layer: it only draws from z13
  add({ id: L(source, 'hit-yard'), type: 'line', ...src, 'source-layer': 'track', minzoom: 13, filter: ['==', get('type'), 'yard'],
    layout: roundJoin, paint: { 'line-color': '#000000', 'line-opacity': 0, 'line-width': 10 } })

  // outline label for small airfields; commercial airports get one icon point below
  add({ id: L(source, 'airport-label'), type: 'symbol', ...src, 'source-layer': 'airports', minzoom: 8, filter: ['all', ['==', get('k'), 'aerodrome'], ['!=', get('com'), 1]],
    layout: { 'text-field': get('lb'), 'text-font': ['Noto Sans Regular'], 'text-size': ['interpolate', ['linear'], ['zoom'], 8, 10, 14, 12],
      'text-letter-spacing': 0.08, 'text-max-width': 8, 'text-padding': 4 },
    paint: { 'text-color': '#6B665B', 'text-halo-color': FILL.land, 'text-halo-width': 1.6 } })
  // commercial airports: one plane icon at the centre of the outline with the IATA code under it
  add({ id: L(source, 'airport-icon'), type: 'symbol', ...src, 'source-layer': 'airport_pts', minzoom: 8,
    layout: { 'icon-image': 'airport', 'icon-allow-overlap': true, 'icon-ignore-placement': true, 'icon-size': ['interpolate', ['linear'], ['zoom'], 8, 0.6, 12, 0.85, 15, 1.05],
      'text-field': get('lb'), 'text-font': ['Noto Sans Bold'], 'text-size': ['interpolate', ['linear'], ['zoom'], 8, 10, 14, 13],
      'text-anchor': 'top', 'text-offset': [0, 0.9], 'text-letter-spacing': 0.06, 'text-padding': 4, 'text-allow-overlap': true, 'text-ignore-placement': true,
      'symbol-sort-key': 0 },
    paint: { 'text-color': '#4A463D', 'text-halo-color': FILL.land, 'text-halo-width': 1.8 } })

  // cities with a metro or tram system: one dot each, from far out (z2) until the stations themselves show (z8)
  const dotSize: Expr = ['step', get('n'), 0.85, 10, 1, 30, 1.3] as Expr
  add({ id: L(source, 'cities'), type: 'circle', ...src, 'source-layer': 'cities', maxzoom: 8,
    paint: { 'circle-radius': ['interpolate', ['linear'], ['zoom'], 2, ['*', 1.8, dotSize], 4, ['*', 2.8, dotSize], 7, ['*', 4.4, dotSize]],
      'circle-color': ['case', ['>', get('m'), 0], TYPE_COLOR.metro, TYPE_COLOR.light_rail], 'circle-stroke-width': 1.2, 'circle-stroke-color': '#FFFFFF',
      'circle-opacity': 0.95 },
    layout: { 'circle-sort-key': get('n') } as never })

  // stations
  const circle = (name: string, tier: number, minzoom: number, r: [number, number, number], stroke: number, ring: Expr | string): void =>
    add({ id: L(source, name), type: 'circle', ...src, 'source-layer': 'stations', minzoom, filter: ['all', ['==', get('tier'), tier], ['!', ['has', 'con']]],
      layout: { 'circle-sort-key': tier === 1 ? 3 : tier === 2 ? 2 : 1 } as never,
      paint: { 'circle-radius': ['interpolate', ['linear'], ['zoom'], 8, r[0], 12, r[1], 15, r[2]],
        'circle-color': '#FFFFFF', 'circle-stroke-width': stroke, 'circle-stroke-color': ring, 'circle-pitch-alignment': 'map' } })
  circle('st3', 3, STATION_ZOOM[3].dot, [2.2, 2.8, 3.5], 1.3, '#3A3936')
  circle('st2', 2, STATION_ZOOM[2].dot, [4, 5, 6], 2.5, INK)
  circle('st1', 1, STATION_ZOOM[1].dot, [4.5, 6.5, 7], 3, ['case', ['==', get('hsr'), 1], '#B3123A', INK])

  // stations still being built: hollow ring (red when high-speed), only shown while Construction is on; size follows the inferred tier
  const conCircle = (tier: 1 | 2 | 3, r: [number, number, number], stroke: number) =>
    add({ id: L(source, `stcon${tier}`), type: 'circle', ...src, 'source-layer': 'stations', minzoom: STATION_ZOOM[tier].dot,
      filter: ['all', ['has', 'con'], ['==', get('tier'), tier]],
      paint: { 'circle-radius': ['interpolate', ['linear'], ['zoom'], 8, r[0], 12, r[1], 15, r[2]], 'circle-color': '#FFFFFF',
        'circle-stroke-width': stroke, 'circle-stroke-color': ['case', ['==', get('hsr'), 1], '#B3123A', '#6B665B'], 'circle-stroke-opacity': 0.95 } })
  conCircle(3, [2.6, 3.6, 4.6], 1.4)
  conCircle(2, [4, 5, 6], 2)
  conCircle(1, [4.5, 6.5, 7], 2.6)

  const label = (name: string, tier: number, minzoom: number, size: [number, number], bold: boolean) =>
    add({ id: L(source, name), type: 'symbol', ...src, 'source-layer': 'complexes', minzoom, filter: ['==', get('tier'), tier],
      layout: { 'text-field': get('n'), 'text-font': [bold ? 'Noto Sans Bold' : 'Noto Sans Regular'],
        'text-size': ['interpolate', ['linear'], ['zoom'], minzoom, size[0], 15, size[1]], 'text-max-width': 7,
        'text-variable-anchor': ['top', 'bottom', 'right', 'left'], 'text-radial-offset': 1.0,
        'text-padding': 2, 'text-optional': true },
      paint: { 'text-color': INK, 'text-halo-color': FILL.land, 'text-halo-width': 1.8 } })
  label('lbl1', 1, STATION_ZOOM[1].label, [13, 16], true)
  label('lbl2', 2, STATION_ZOOM[2].label, [12, 13], true)
  label('lbl3', 3, STATION_ZOOM[3].label, [10.5, 11.5], false)
  const conLabel = (tier: 1 | 2 | 3, size: [number, number]) =>
    add({ id: L(source, `lblcon${tier}`), type: 'symbol', ...src, 'source-layer': 'complexes', minzoom: STATION_ZOOM[tier].label,
      filter: ['all', ['has', 'con'], ['==', get('tier'), tier]],
      layout: { 'text-field': get('n'), 'text-font': [tier < 3 ? 'Noto Sans Bold' : 'Noto Sans Regular'],
        'text-size': ['interpolate', ['linear'], ['zoom'], 11, size[0], 15, size[1]], 'text-max-width': 7, 'text-anchor': 'top', 'text-offset': [0, 0.8], 'text-padding': 3 },
      paint: { 'text-color': '#7A756A', 'text-halo-color': FILL.land, 'text-halo-width': 1.5 } })
  conLabel(1, [13, 16]); conLabel(2, [12, 13]); conLabel(3, [10, 11.5])
}

export function stationLayers(slug: string) { return ['stcon1', 'stcon2', 'stcon3', 'st1', 'st2', 'st3'].map((n) => L(slug, n)) }
export const strandLayer = (slug: string) => L(slug, 'strand')
export const hitLayer = (slug: string) => L(slug, 'hit')
export const yardHitLayer = (slug: string) => L(slug, 'hit-yard')
/** polygon layers that answer hovers, most specific first */
export const areaLayers = (slug: string) => ['platforms-fill', 'platforms-line', 'airport-terminal', 'buildings', 'yard-fill', 'ground', 'airport-fill'].map((n) => L(slug, n))

const vis = (on: boolean) => (on ? 'visible' : 'none')

/** Push toggles / colour mode into the rail layers. Cheap enough to call on every state change. */
export function applyState(map: MLMap, s: MapState) {
  if (map.getLayer(L(TILES, 'track'))) applyRegion(map, TILES, s)
}

/** Fade every line but the highlighted one (a line or railway is selected); null: no focus. One source, so the veil covers
 *  all regions on screen (it used to cover only the selected region's source and those below it). */
export function setFocus(map: MLMap, slug: string | null) {
  if (map.getLayer(L(TILES, 'veil'))) map.setPaintProperty(L(TILES, 'veil'), 'background-opacity', slug ? FOCUS_VEIL : 0)
}
const FOCUS_VEIL = 0.72

function applyRegion(map: MLMap, slug: string, s: MapState) {
  const operator = s.mode === 'operator'
  {
    map.setLayoutProperty(L(slug, 'freight'), 'visibility', vis(s.freight))

    for (const n of ['lbl1', 'lbl2', 'lbl3', 'lblcon1', 'lblcon2', 'lblcon3']) map.setLayoutProperty(L(slug, n), 'text-field', nameExpr(s.lang, 'n', 'ne'))
    map.setLayoutProperty(L(slug, 'lbl-track'), 'text-field', nameExpr(s.lang, 'n', 'ne'))
    map.setLayoutProperty(L(slug, 'lbl-lines'), 'text-field', nameExpr(s.lang, 'lb', 'lbe'))
    // paint, layout and filter changes reach a layer and its high-speed twin (if it has one) together
    const names = (n: string) => (TWINS.has(n) ? [n, `${n}-hs`] : [n])
    const setPaint = (n: string, prop: string, v: unknown) => names(n).forEach((x) => map.setPaintProperty(L(slug, x), prop as never, v as never))
    const setVis = (n: string, v: string) => names(n).forEach((x) => map.setLayoutProperty(L(slug, x), 'visibility', v as never))
    const setFilt = (n: string, f: Expr | null, typeKey = 'type') => {
      if (!TWINS.has(n)) { map.setFilter(L(slug, n), f); return }
      names(n).forEach((x, i) => {
        const hs: Expr = [i === 0 ? '!=' : '==', get(typeKey), 'high_speed'] as Expr
        map.setFilter(L(slug, x), (f ? ['all', hs, f] : hs) as Expr)
      })
    }
    // proposed track rides on the construction layers (flag `prop`), drawn fainter; each switch shows its own kind
    setVis('construction', vis(s.construction || s.proposed)); setVis('construction-casing', vis(s.construction || s.proposed))
    const conKinds: Expr = ['any', ...(s.construction ? [['all', ['has', 'con'], ['!', ['has', 'prop']]]] : []), ...(s.proposed ? [['has', 'prop']] : [])] as unknown as Expr
    setVis('strand', vis(operator)); setVis('strand-casing', vis(operator))
    const has = ['has', 'oc'] as Expr
    const tun: Expr = ['step', ['zoom'], 1, 13, ['case', ['has', 'tun'], 0.45, 1]]
    // each colour mode sets its own colour and width; "HSR only" then greys and thins everything that is not high-speed, in any mode
    const greyOut = s.hsrOnly
    const grey = (c: Expr, active: Expr = activeTrack): Expr => (greyOut ? (['case', active, c, INACTIVE] as Expr) : c)
    const thin = (m?: Expr): Expr | undefined => (greyOut ? (['*', m ?? 1, ['case', activeTrack, 1, INACTIVE_WIDTH]] as Expr) : m)
    const strandActive: Expr = ['==', get('t'), 'high_speed'] as Expr
    let color: Expr, width: Expr, opacity: Expr = tun
    if (operator) {
      // below z12 each track takes one colour; from z12 tracks with mapped lines give way to the strands
      color = ['coalesce', get('oc'), typeColor] as Expr
      width = widthExpr(thin())
      opacity = ['step', ['zoom'], ['case', has, 1, 0.6], 12, ['case', ['has', 'nl'], 0, 0.6]] as Expr
    } else if (s.mode === 'speed') {
      color = speedColor
      width = widthExpr(thin())
    } else {
      color = typeColor
      width = widthExpr(thin())
    }
    setPaint('construction', 'line-opacity', ['case', ['has', 'prop'], 0.5, 1])
    setPaint('construction-casing', 'line-opacity', ['case', ['has', 'prop'], 0.45, 0.85])
    setPaint('track', 'line-color', grey(color))
    setPaint('track', 'line-width', width)
    setPaint('track', 'line-opacity', opacity)
    setPaint('hl', 'line-color', grey(color))
    setPaint('hl-fs', 'line-color', grey(color))
    // under-construction track follows the same colour rule as open track
    setPaint('construction', 'line-color', grey(s.mode === 'speed' ? speedColor : (['match', get('type'), 'other', '#45423C', typeColor] as Expr)))
    setPaint('strand', 'line-color', grey(['coalesce', get('c'), byType((t) => TYPE_COLOR[t], 't', TYPE_COLOR.other)] as Expr, strandActive))
    setPaint('strand-casing', 'line-opacity', greyOut ? (['case', strandActive, 0.28, 0] as Expr) : 0.28)
    const hiddenT = s.hidden.length > 0
    const notHiddenT = (key: string): Expr[] => (hiddenT ? [['!', ['in', get(key), ['literal', s.hidden]]] as Expr] : [])
    const noSpeed = notHiddenSpeed(s.hiddenSpeed)
    setFilt('track', ['all', ['!', ['has', 'con']], ['!=', get('type'), 'yard'], ['!=', get('type'), 'freight'], ...notHiddenT('type'), ...noSpeed] as Expr)
    setFilt('construction', ['all', conKinds, ...notHiddenT('type'), ...noSpeed] as Expr)
    setFilt('construction-casing', ['all', conKinds, ...notHiddenT('type'), ...noSpeed] as Expr)
    map.setLayoutProperty(L(slug, 'casing'), 'visibility', vis(!s.hidden.includes('high_speed')))
    map.setFilter(L(slug, 'casing'), ['all', ['==', get('type'), 'high_speed'], ['!', ['has', 'con']], ...noSpeed] as Expr)
    // city dots: a city shows while one of its systems (metro, tram) is not hidden; "HSR only" has no metro or tram to show
    const cityKinds: Expr[] = [...(s.hidden.includes('metro') ? [] : [['>', get('m'), 0]]), ...(s.hidden.includes('light_rail') ? [] : [['>', get('l'), 0]])] as Expr[]
    map.setFilter(L(slug, 'cities'), cityKinds.length ? (['any', ...cityKinds] as Expr) : (['==', 1, 0] as Expr))
    map.setLayoutProperty(L(slug, 'cities'), 'visibility', vis(!greyOut))
    // a station disappears when every rail type that serves it is hidden (stations with no known type stay)
    const ty: Expr = ['coalesce', get('ty'), ''] as Expr
    const served: Expr[] = hiddenT
      ? [['any', ['==', ty, ''], ...TYPE_ORDER.filter((t) => !s.hidden.includes(t)).map((t) => ['in', `,${t},`, ty])] as unknown as Expr]
      : []
    // greyed-out mode: only stations that high-speed rail serves stay on the map
    if (greyOut) served.push(activeStation(ty))
    ;([['st1', 1], ['st2', 2], ['st3', 3], ['lbl1', 1], ['lbl2', 2], ['lbl3', 3]] as const).forEach(([n, tier]) =>
      map.setFilter(L(slug, n), ['all', ['==', get('tier'), tier], ['!', ['has', 'con']], ...served] as Expr))
    ;([['stcon1', 1], ['stcon2', 2], ['stcon3', 3], ['lblcon1', 1], ['lblcon2', 2], ['lblcon3', 3]] as const).forEach(([n, tier]) =>
      map.setFilter(L(slug, n), ['all', ['has', 'con'], ['==', get('tier'), tier], ...served] as Expr))
    for (const n of ['stcon1', 'stcon2', 'stcon3', 'lblcon1', 'lblcon2', 'lblcon3']) map.setLayoutProperty(L(slug, n), 'visibility', vis(s.construction))
    for (const n of ['strand', 'strand-casing']) setFilt(n, hiddenT ? (['all', ...notHiddenT('t')] as Expr) : null, 't')
    map.setFilter(L(slug, 'lbl-lines'), hiddenT || greyOut ? (['all', ...(greyOut ? [strandActive] : []), ...notHiddenT('t')] as Expr) : null)
    map.setFilter(L(slug, 'lbl-track'), ['all', ...(greyOut ? [activeTrack] : []), ['has', 'n'], ['!', ['has', 'nl']], ['!', ['has', 'con']], ['!=', get('type'), 'yard'], ['!=', get('type'), 'freight'], ...notHiddenT('type'), ...noSpeed] as Expr)
    // construction track is not sorted into speed bands, so hiding a band leaves it hoverable
    const hide: Expr[] = [...notHiddenT('type'), ...noSpeed.map((e) => ['any', ['has', 'con'], e] as unknown as Expr)]
    if (!s.freight) hide.push(['!=', get('type'), 'freight'] as Expr)
    if (!s.construction) hide.push(['any', ['!', ['has', 'con']], ['has', 'prop']] as Expr)
    if (!s.proposed) hide.push(['!', ['has', 'prop']] as Expr)
    if (greyOut) hide.push(activeTrack)  // greyed-out track cannot be hovered or selected
    map.setFilter(L(slug, 'hit'), ['all', ['!=', get('type'), 'yard'], ...hide] as Expr)
  }
}

const EMPTY: GeoJSON.FeatureCollection = { type: 'FeatureCollection', features: [] }
let hlKey = ''          // what was asked for last
let hlShown = ''        // what is on the map
let hlCurrent: Highlight | null = null
const hlLoaded = new Map<string, Highlight | null>()   // highlight data already fetched, by key: shown with no wait

function clearHighlight(map: MLMap) {
  // remove the flags we set, one id at a time: removing a whole source layer makes MapLibre re-send every id that was ever
  // flagged on each later update, so a long session of hovering got slower and slower
  if (hlCurrent && map.getSource(TILES)) for (const id of hlCurrent.ways) map.removeFeatureState({ source: TILES, sourceLayer: 'track', id }, 'hl')
  if (hlCurrent) (map.getSource(L(TILES, 'hl')) as GeoJSONSource | undefined)?.setData(EMPTY)
  hlCurrent = null
}

function showHighlight(map: MLMap, h: Highlight) {
  clearHighlight(map)
  hlCurrent = h
  for (const id of h.ways) map.setFeatureState({ source: TILES, sourceLayer: 'track', id }, { hl: true })
  const src = map.getSource(L(TILES, 'hl')) as GeoJSONSource | undefined
  if (src && h.geom) src.setData({ type: 'Feature', properties: { type: h.type, ...(h.oc ? { oc: h.oc } : {}) }, geometry: h.geom })
}

function apply(map: MLMap, key: string, slug: string | null, h: Highlight | null) {
  if (h && slug) showHighlight(map, h)
  else clearHighlight(map)
  hlShown = key
}

/** Highlight a line (or, with no line, a named railway) of region `slug`: flags the line's track pieces with feature state in
 *  the tile source and shows its simplified geometry below z8. No tile is decoded or reloaded. A line whose data is already
 *  loaded is shown at once; otherwise its data is fetched once the pointer rests. */
export function highlightLine(map: MLMap, slug: string | null, lineId: number | null, trackName: string | null = null) {
  const key = slug && (lineId != null || trackName != null) ? `${slug}|${lineId ?? ''}|${trackName ?? ''}` : ''
  if (key === hlKey) return
  hlKey = key
  if (key === hlShown) return
  if (!key) { apply(map, '', null, null); return }
  if (hlLoaded.has(key)) { apply(map, key, slug, hlLoaded.get(key)!); return }
  // no delay: a hover starts loading at once (a line's file is a few KB and cached after the first time); a pointer that has
  // moved on by the time it arrives is ignored (hlKey)
  void (async () => {
    const h = await (lineId != null ? loadLineHighlight(slug!, lineId) : loadNameHighlight(slug!, trackName!)).catch(() => null)
    hlLoaded.set(key, h)
    if (hlKey === key) apply(map, key, slug, h)   // else something else was asked for while this loaded
  })()
}

/** While the map is idle, load the highlight data of the lines on screen so a hover finds it ready (a file is a few KB; a long
 *  line's is up to ~100 KB). Cheap enough: one hit-layer query, at most PREFETCH_MAX files, a few at a time. */
const PREFETCH_MAX = 48
export function prefetchHighlights(map: MLMap) {
  if (map.getZoom() < 6) return
  const layers = [hitLayer(TILES)].filter((l) => map.getLayer(l))
  if (!layers.length) return
  const todo = new Map<string, () => Promise<unknown>>()
  for (const f of map.queryRenderedFeatures({ layers })) {
    if (todo.size >= PREFETCH_MAX) break
    const slug = regionOf(f)
    const ln = Number(String(f.properties.ln ?? '').split(',')[0])
    if (Number.isFinite(ln) && ln > 0) todo.set(`${slug}|${ln}`, () => loadLineHighlight(slug, ln))
    else if (f.properties.n) todo.set(`${slug}|n|${f.properties.n}`, () => loadNameHighlight(slug, String(f.properties.n)))
  }
  const jobs = [...todo.values()]
  const run = async () => { for (let j = jobs.shift(); j; j = jobs.shift()) await j().catch(() => null) }
  for (let i = 0; i < 3; i++) void run()   // three at a time
}

/** Material "flight" glyph, 24x24 */
export const PLANE_PATH = 'M21 16v-2l-8-5V3.5c0-.83-.67-1.5-1.5-1.5S10 2.67 10 3.5V9l-8 5v2l8-2.5V19l-2 1.5V22l3.5-1 3.5 1v-1.5L13 19v-5.5l8 2.5z'

/** Airport station marker: a plane on a white disc; red ring when high-speed rail serves the station. */
export function addAirportIcon(map: MLMap) {
  const make = (name: string, ring: string) => {
    if (map.hasImage(name)) return
    const px = 2
    const size = 26 * px
    const c = document.createElement('canvas')
    c.width = c.height = size
    const g = c.getContext('2d')!
    g.scale(px, px)
    g.beginPath(); g.arc(13, 13, 11.5, 0, Math.PI * 2)
    g.fillStyle = '#FFFFFF'; g.fill()
    g.lineWidth = 2.2; g.strokeStyle = ring; g.stroke()
    g.translate(13 - 8.4, 13 - 8.4); g.scale(0.7, 0.7)
    g.fillStyle = '#1E1E1C'; g.fill(new Path2D(PLANE_PATH))
    map.addImage(name, g.getImageData(0, 0, size, size), { pixelRatio: px })
  }
  make('airport', '#1E1E1C')
  }
