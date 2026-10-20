import type { Map as MLMap } from 'maplibre-gl'
import { INK, FILL, THEME_COLORS, TYPE_COLOR } from '../palette'
import { STATION_ZOOM } from '../config'
import { TILES } from '../regions'
import { BOLD_STACK, L, REG_STACK, get, type Expr, type RailCtx } from './shared'

/** cities with a metro, tram or monorail system: one dot each, from far out (z2) until the stations themselves show (z8) */
export function addCityDots({ source, add, src }: RailCtx) {
  const dotSize: Expr = ['step', get('n'), 0.85, 40, 1, 150, 1.3] as Expr
  add({
    id: L(source, 'cities'),
    type: 'circle',
    ...src,
    'source-layer': 'cities',
    maxzoom: 8,
    paint: {
      'circle-radius': ['interpolate', ['linear'], ['zoom'], 2, ['*', 1.8, dotSize], 4, ['*', 2.8, dotSize], 7, ['*', 4.4, dotSize]],
      'circle-color': ['case', ['>', get('m'), 0], TYPE_COLOR.metro, ['>', get('l'), 0], TYPE_COLOR.light_rail, TYPE_COLOR.monorail],
      'circle-stroke-width': 1.2,
      'circle-stroke-color': '#FFFFFF',
      'circle-opacity': 0.95,
    },
    layout: { 'circle-sort-key': get('n') } as never,
  })
}

/** the station dots, their labels, the operator logos and the dots and labels of stations still being built */
export function addStationLayers({ source, add, src }: RailCtx) {
  const circle = (name: string, tier: number, minzoom: number, r: [number, number, number], stroke: number, ring: Expr | string): void =>
    add({
      id: L(source, name),
      type: 'circle',
      ...src,
      'source-layer': 'stations',
      minzoom,
      filter: ['all', ['==', get('tier'), tier], ['!', ['has', 'con']]],
      layout: { 'circle-sort-key': tier === 1 ? 3 : tier === 2 ? 2 : 1 } as never,
      paint: {
        'circle-radius': ['interpolate', ['linear'], ['zoom'], 8, r[0], 12, r[1], 15, r[2]],
        'circle-color': THEME_COLORS.light.stationFill,
        'circle-stroke-width': stroke,
        'circle-stroke-color': ring,
        'circle-pitch-alignment': 'map',
      },
    })
  circle('st3', 3, STATION_ZOOM[3].dot, [2.2, 2.8, 3.5], 1.3, '#3A3936')
  circle('st2', 2, STATION_ZOOM[2].dot, [4, 5, 6], 2.5, INK)
  circle('st1', 1, STATION_ZOOM[1].dot, [4.5, 6.5, 7], 3, ['case', ['==', get('hsr'), 1], '#B3123A', INK])

  // stations still being built: hollow ring (red when high-speed), only shown while Construction is on; size follows the inferred tier
  const conCircle = (tier: 1 | 2 | 3, r: [number, number, number], stroke: number) =>
    add({
      id: L(source, `stcon${tier}`),
      type: 'circle',
      ...src,
      'source-layer': 'stations',
      minzoom: STATION_ZOOM[tier].dot,
      filter: ['all', ['has', 'con'], ['==', get('tier'), tier]],
      paint: {
        'circle-radius': ['interpolate', ['linear'], ['zoom'], 8, r[0], 12, r[1], 15, r[2]],
        'circle-color': THEME_COLORS.light.stationFill,
        'circle-stroke-width': stroke,
        'circle-stroke-color': ['case', ['==', get('hsr'), 1], '#B3123A', '#6B665B'],
        'circle-stroke-opacity': 0.95,
      },
    })
  conCircle(3, [2.6, 3.6, 4.6], 1.4)
  conCircle(2, [4, 5, 6], 2)
  conCircle(1, [4.5, 6.5, 7], 2.6)

  const label = (name: string, tier: number, minzoom: number, size: [number, number], bold: boolean) =>
    add({
      id: L(source, name),
      type: 'symbol',
      ...src,
      'source-layer': 'complexes',
      minzoom,
      filter: ['==', get('tier'), tier],
      layout: {
        'text-field': get('n'),
        'text-font': bold ? BOLD_STACK : REG_STACK,
        'text-size': ['interpolate', ['linear'], ['zoom'], minzoom, size[0], 15, size[1]],
        'text-max-width': 7,
        'text-variable-anchor': ['top', 'bottom', 'right', 'left'],
        'text-radial-offset': 1.0,
        'text-padding': 2,
        'text-optional': true,
      },
      paint: { 'text-color': INK, 'text-halo-color': FILL.land, 'text-halo-width': 1.8 },
    })
  label('lbl1', 1, STATION_ZOOM[1].label, [13, 16], true)
  label('lbl2', 2, STATION_ZOOM[2].label, [12, 13], true)
  label('lbl3', 3, STATION_ZOOM[3].label, [10.5, 11.5], false)
  // operator logo on the dots of the bigger stations (tier 1 and 2), always covering the dot (centred at every zoom, so the dot never shows beside it); the image is fetched when the layer first asks for it (addBadgeImages)
  add({
    id: L(source, 'logo'),
    type: 'symbol',
    ...src,
    'source-layer': 'stations',
    minzoom: 9,
    filter: ['all', ['has', 'lg'], ['<=', get('tier'), 2], ['!', ['has', 'con']]],
    layout: {
      'icon-image': ['concat', 'logo|', get('lg')],
      'icon-anchor': 'center',
      'symbol-sort-key': get('tier'),
      'icon-size': ['interpolate', ['linear'], ['zoom'], 9, 0.6, 14, 0.9],
      'icon-padding': 3,
    },
  })
  const conLabel = (tier: 1 | 2 | 3, size: [number, number]) =>
    add({
      id: L(source, `lblcon${tier}`),
      type: 'symbol',
      ...src,
      'source-layer': 'complexes',
      minzoom: STATION_ZOOM[tier].label,
      filter: ['all', ['has', 'con'], ['==', get('tier'), tier]],
      layout: {
        'text-field': get('n'),
        'text-font': tier < 3 ? BOLD_STACK : REG_STACK,
        'text-size': ['interpolate', ['linear'], ['zoom'], 11, size[0], 15, size[1]],
        'text-max-width': 7,
        'text-anchor': 'top',
        'text-offset': [0, 0.8],
        'text-padding': 3,
      },
      paint: { 'text-color': '#7A756A', 'text-halo-color': FILL.land, 'text-halo-width': 1.5 },
    })
  conLabel(1, [13, 16])
  conLabel(2, [12, 13])
  conLabel(3, [10, 11.5])
}

export function stationLayers(slug: string) {
  return ['stcon1', 'stcon2', 'stcon3', 'st1', 'st2', 'st3'].map((n) => L(slug, n))
}

/** While lines are focused (a line, the services on a track, a station's lines), fade every station that is not on them. `ids` are station ids
 *  (the `id` property of the station tiles); labels carry the complex's id, which differs for a multi-station complex (Zürich HB), so the
 *  `cx` of the station dots that are on screen and listed is added to the set (refreshStationFocus, on every idle). null: no focus. */
const STATION_FADE = 0.22
let stationFocus: Set<string> | null = null
let stationFocusKey = ''
const FOCUS_DOT_LAYERS = ['st1', 'st2', 'st3', 'stcon1', 'stcon2', 'stcon3']
const FOCUS_LABEL_LAYERS = ['lbl1', 'lbl2', 'lbl1t', 'lbl2t', 'lbl3', 'lblcon1', 'lblcon2', 'lblcon3']
function paintStationFocus(map: MLMap, ids: Set<string> | null) {
  const on = (key: string): Expr => ['in', ['get', key], ['literal', [...ids!].sort()]] as Expr
  const fade = (key: string, ...vals: [number, number]): Expr => ['case', on(key), vals[0], vals[1]] as Expr
  const put = (id: string, prop: string, v: unknown) => map.getLayer(L(TILES, id)) && map.setPaintProperty(L(TILES, id), prop as never, v as never)
  for (const n of FOCUS_DOT_LAYERS) {
    const con = n.startsWith('stcon')
    put(n, 'circle-opacity', ids ? fade('id', 1, STATION_FADE) : 1)
    put(n, 'circle-stroke-opacity', ids ? fade('id', con ? 0.95 : 1, STATION_FADE) : con ? 0.95 : 1)
  }
  for (const n of FOCUS_LABEL_LAYERS) {
    put(n, 'text-opacity', ids ? fade('id', 1, STATION_FADE) : 1)
  }
  put('logo', 'icon-opacity', ids ? fade('id', 1, STATION_FADE) : 1)
}
export function setStationFocus(map: MLMap, ids: string[] | null) {
  stationFocus = ids ? new Set(ids) : null
  stationFocusKey = ''
  refreshStationFocus(map)
}
export function refreshStationFocus(map: MLMap) {
  if (!stationFocus) {
    if (stationFocusKey !== 'off') {
      paintStationFocus(map, null)
      stationFocusKey = 'off'
    }
    return
  }
  const layers = [...FOCUS_DOT_LAYERS, 'logo'].map((n) => L(TILES, n)).filter((l) => map.getLayer(l))
  const set = new Set(stationFocus)
  for (const f of map.queryRenderedFeatures({ layers })) if (stationFocus.has(String(f.properties.id)) && f.properties.cx) set.add(String(f.properties.cx))
  const key = `${stationFocus.size}|${set.size}`
  if (key === stationFocusKey) return
  stationFocusKey = key
  paintStationFocus(map, set)
}
