import type { LayerSpecification, Map as MLMap } from 'maplibre-gl'
import { FILL, TYPE_COLOR, TYPE_WIDTH } from '../palette'
import { pmtilesUrl } from '../data'
import { ATTRIBUTION } from './base'
import { L, byType, get, roundJoin, type Expr, type RailCtx } from './shared'
import { LOW_ZOOM_SHARE, TWINS, rank, typeColor, widthExpr } from './trackStyle'
import { addConstructionLayers } from './construction'
import { addHighlightLayers } from './highlight'
import { addAirportLabels, addAreaLayers } from './areas'
import { addLineLabels } from './labels'
import { addCityDots, addStationLayers } from './stations'

/** All rail layers of the tile source `source` (TILES), stacked in drawing order; ids are `<source>:<name>`. */
export function addRailSource(map: MLMap, source: string) {
  // our PMTiles are MLT (written by rail-extract); MapLibre does not detect the format, so say so here
  map.addSource(source, { type: 'vector', url: pmtilesUrl(source), encoding: 'mlt', attribution: ATTRIBUTION })
  const add = (l: Record<string, unknown>): void => { map.addLayer(l as unknown as LayerSpecification) }
  const src = { source } as const

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
    }
    add({ ...spec, id, filter: f ? ['all', ['!=', get(typeKey), 'high_speed'], f] : ['!=', get(typeKey), 'high_speed'] } as unknown as LayerSpecification)
    top.push({ ...spec, id: `${id}-hs`, filter: f ? ['all', ['==', get(typeKey), 'high_speed'], f] : ['==', get(typeKey), 'high_speed'] } as unknown as LayerSpecification)
  }
  // highlight layers are added last (after a veil, below), whatever order they are declared in
  const late: LayerSpecification[] = []
  const hlLayer = (spec: Record<string, unknown>) => {
    late.push({ ...spec, layout: { 'line-sort-key': hsLast('type'), ...(spec.layout as Record<string, unknown>) } } as unknown as LayerSpecification)
  }
  const c: RailCtx = { map, source, add, src, top, late, trackLayer, hlLayer }

  addAreaLayers(c)
  addTrackLayers(c)
  addConstructionLayers(c)
  addHighlightLayers(c)
  addStrandLayers(c)

  top.forEach((l) => add(l)) // high-speed twins and casing: above every other rail layer
  // Focus veil: while a line is selected the other lines fade. A background layer at constant opacity does it with a repaint only:
  // changing a data-driven paint property (the obvious way) makes MapLibre re-parse and re-upload every loaded tile.
  add({ id: L(source, 'veil'), type: 'background', paint: { 'background-color': FILL.land, 'background-opacity': 0, 'background-opacity-transition': { duration: 120, delay: 0 } } })
  late.forEach((l) => add(l))   // the highlight, above the veil

  addHitLayers(c)
  addLineLabels(c)

  // yard track gets its own hit layer: it only draws from z13
  add({ id: L(source, 'hit-yard'), type: 'line', ...src, 'source-layer': 'track', minzoom: 13, filter: ['==', get('type'), 'yard'],
    layout: roundJoin, paint: { 'line-color': '#000000', 'line-opacity': 0, 'line-width': 10 } })

  addAirportLabels(c)
  addCityDots(c)
  addStationLayers(c)
}

/** yard and freight track, the high-speed casing and the open track itself */
function addTrackLayers({ source, add, src, top, trackLayer }: RailCtx) {
  add({ id: L(source, 'yard'), type: 'line', ...src, 'source-layer': 'track', minzoom: 13, filter: ['==', get('type'), 'yard'],
    layout: roundJoin, paint: { 'line-color': TYPE_COLOR.yard, 'line-width': widthExpr() } })
  add({ id: L(source, 'freight'), type: 'line', ...src, 'source-layer': 'track', minzoom: 8,
    filter: ['all', ['==', get('type'), 'freight'], ['!', ['has', 'con']]],
    layout: { ...roundJoin, visibility: 'none' }, paint: { 'line-color': TYPE_COLOR.freight, 'line-width': widthExpr() } })
  top.push({ id: L(source, 'casing'), type: 'line', ...src, 'source-layer': 'track', filter: ['all', ['==', get('type'), 'high_speed'], ['!', ['has', 'con']]],
    layout: roundJoin,
    paint: { 'line-color': '#FFFFFF', 'line-opacity': 0.9, 'line-gap-width': 0,
      'line-width': ['interpolate', ['linear'], ['zoom'], 7, 6 * LOW_ZOOM_SHARE, 10, 6, 12, 7.5, 15, 9] } } as LayerSpecification)
  trackLayer('track', { type: 'line', ...src, 'source-layer': 'track', minzoom: 0,
    filter: ['all', ['!', ['has', 'con']], ['!=', get('type'), 'yard'], ['!=', get('type'), 'freight']],
    layout: { ...roundJoin, 'line-sort-key': rank },
    paint: { 'line-color': typeColor, 'line-width': widthExpr(), 'line-opacity': 1 } })
}

/** operator-colour strands (side by side from z12) */
function addStrandLayers({ src, trackLayer }: RailCtx) {
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
}

function addHitLayers({ source, add, src }: RailCtx) {
  // invisible, wider hit target for track tooltips
  // at every zoom: below z9 the track tiles hold the merged overview, which keeps the line ids and railway names a click needs
  // open track, construction and proposed are separate layers so their switches only flip visibility (see the construction layers)
  for (const [name, kind] of [['hit', ['!', ['has', 'con']]], ['hit-con', ['all', ['has', 'con'], ['!', ['has', 'prop']]]], ['hit-prop', ['has', 'prop']]] as const) {
    add({ id: L(source, name), type: 'line', ...src, 'source-layer': 'track', filter: ['all', ['!=', get('type'), 'yard'], kind],
      layout: roundJoin, paint: { 'line-color': '#000000', 'line-opacity': 0, 'line-width': 12 } })
  }
}
