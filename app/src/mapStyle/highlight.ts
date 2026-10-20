import type { GeoJSONSource, Map as MLMap } from 'maplibre-gl'
import { TYPE_WIDTH } from '../palette'
import { TILES, regionOf } from '../regions'
import { loadLineHighlight, loadNameHighlight, pmtilesUrl, type Highlight } from '../data'
import { L, byType, hitLayers, type Expr, type RailCtx } from './shared'
import { typeColor } from './trackStyle'

const EMPTY: GeoJSON.FeatureCollection = { type: 'FeatureCollection', features: [] }
const HL_NONE = ['==', ['get', 'id'], ''] as Expr   // matches no track piece

/** the highlight layers: the hovered or selected line drawn wider in its own colour */
export function addHighlightLayers({ map, source, hlLayer }: RailCtx) {
  // highlight for the hovered or selected line: the same line drawn wider in its own colour, no outline
  const hlW: Expr = ['interpolate', ['linear'], ['zoom'],
    8, ['+', ['*', byType((t) => TYPE_WIDTH[t][0]), 1.8], 2], 12, ['+', ['*', byType((t) => TYPE_WIDTH[t][1]), 1.8], 2.5],
    15, ['+', ['*', byType((t) => TYPE_WIDTH[t][2]), 1.8], 3]] as Expr
  // The highlight layers come after the casing, so no twin is needed; high-speed sorts last within each.
  // z9 and closer: the track pieces whose `id` property ("way/123") is in the filter set by highlightLine. No feature state:
  // MapLibre decodes every loaded MLT tile on the main thread as soon as a source has any feature state.
  // Own source (same PMTiles, z9 and closer only): a filter change reloads every tile of the layer's source, and on the main
  // source that drops each tile's decoded copy, which MLT then decodes again whole on the next hover query.
  map.addSource(L(source, 'hls'), { type: 'vector', url: pmtilesUrl(source), encoding: 'mlt', minzoom: 9 })
  hlLayer({ id: L(source, 'hl-fs'), type: 'line', source: L(source, 'hls'), 'source-layer': 'track', minzoom: 9,
    layout: { 'line-cap': 'round', 'line-join': 'round' },
    filter: HL_NONE, paint: { 'line-color': typeColor, 'line-width': hlW } })
  // below z9 the merged overview has no per-piece ids: the line's simplified geometry from its side file
  map.addSource(L(source, 'hl'), { type: 'geojson', data: EMPTY })
  hlLayer({ id: L(source, 'hl'), type: 'line', source: L(source, 'hl'), maxzoom: 9,
    layout: { 'line-cap': 'round', 'line-join': 'round' }, paint: { 'line-color': typeColor, 'line-width': hlW } })
}

let hlKey = ''          // what was asked for last
let hlShown = ''        // what is on the map
let hlCurrent = false
const hlLoaded = new Map<string, Highlight | null>()   // highlight data already fetched, by key: shown with no wait

function clearHighlight(map: MLMap, setFilter = true) {
  if (hlCurrent && setFilter && map.getLayer(L(TILES, 'hl-fs'))) map.setFilter(L(TILES, 'hl-fs'), HL_NONE)
  if (hlCurrent) (map.getSource(L(TILES, 'hl')) as GeoJSONSource | undefined)?.setData(EMPTY)
  hlCurrent = false
}

/** Show one or several highlights at once (a station's lines, the services on a track): the union of their track pieces on the
 *  z9+ layer, one feature per line for the simplified geometry below z9. */
/** the colour expression of the geometry layer, with the feature properties it reads (`type`, `oc`) fixed to one line's values */
function pinLine(e: unknown, h: Highlight): unknown {
  if (!Array.isArray(e)) return e
  if (e[0] === 'get' && e[1] === 'type') return h.type
  if (e[0] === 'get' && e[1] === 'oc') return h.oc ?? ['get', '__none']
  if (e[0] === 'literal') return e
  return e.map((x) => pinLine(x, h))
}

function showHighlight(map: MLMap, hs: Highlight[]) {
  clearHighlight(map, false)
  hlCurrent = true
  const ways = hs.length === 1 ? hs[0].ways : [...new Set(hs.flatMap((h) => h.ways))].sort()
  map.setFilter(L(TILES, 'hl-fs'), ['in', ['get', 'id'], ['literal', ways]] as Expr)
  // One line: draw all of it in the line's own colour (what the zoomed-out geometry layer does), not in the colour of each track piece it runs on
  // (an S-Bahn over intercity track would change colour piece by piece). Several lines: each piece keeps its own colour.
  const base = map.getPaintProperty(L(TILES, 'hl'), 'line-color')
  map.setPaintProperty(L(TILES, 'hl-fs'), 'line-color', (hs.length === 1 ? pinLine(base, hs[0]) : base) as never)
  const src = map.getSource(L(TILES, 'hl')) as GeoJSONSource | undefined
  src?.setData({ type: 'FeatureCollection', features: hs.filter((h) => h.geom).map((h) => ({ type: 'Feature', properties: { type: h.type, ...(h.oc ? { oc: h.oc } : {}) }, geometry: h.geom! })) })
}

function apply(map: MLMap, key: string, slug: string | null, hs: Highlight[]) {
  if (hs.length && slug) showHighlight(map, hs)
  else clearHighlight(map)
  hlShown = key
}

/** Highlight a line, several lines (all of one region `slug`) or, with no line, a named railway: filters the lines' track pieces on the highlight
 *  layer and shows their simplified geometry below z9. Data already loaded is shown at once; otherwise it is fetched (the pointer may have moved on by then: hlKey). */
export function highlightLine(map: MLMap, slug: string | null, lineIds: number | number[] | null, trackName: string | null = null) {
  const ids = lineIds == null ? [] : Array.isArray(lineIds) ? lineIds : [lineIds]
  const key = slug && (ids.length || trackName != null) ? `${slug}|${ids.join(',')}|${trackName ?? ''}` : ''
  if (key === hlKey) return
  hlKey = key
  if (key === hlShown) return
  if (!key) { apply(map, '', null, []); return }
  const parts = ids.length ? ids.map((id) => `${slug}|${id}`) : [`${slug}|n|${trackName}`]
  const ready = () => parts.map((k) => hlLoaded.get(k)).filter((h): h is Highlight => !!h)
  if (parts.every((k) => hlLoaded.has(k))) { apply(map, key, slug, ready()); return }
  void (async () => {
    await Promise.all(parts.filter((k) => !hlLoaded.has(k)).map(async (k) => {
      const h = await (ids.length ? loadLineHighlight(slug!, Number(k.split('|')[1])) : loadNameHighlight(slug!, trackName!)).catch(() => null)
      hlLoaded.set(k, h)
    }))
    if (hlKey === key) apply(map, key, slug, ready())   // else something else was asked for while this loaded
  })()
}

/** While the map is idle, load the highlight data of the lines on screen so a hover finds it ready (a file is a few KB; a long
 *  line's is up to ~100 KB). Cheap enough: one hit-layer query, at most PREFETCH_MAX files, a few at a time. */
const PREFETCH_MAX = 48
let prefetchGen = 0
/** Resolves to true when every queued file was processed, false when the map moved first or a newer call took over (the caller may then ask again). */
export function prefetchHighlights(map: MLMap): Promise<boolean> {
  if (map.getZoom() < 6) return Promise.resolve(true)
  const layers = hitLayers(TILES).filter((l) => map.getLayer(l))
  if (!layers.length) return Promise.resolve(true)
  const gen = ++prefetchGen
  const todo = new Map<string, () => Promise<unknown>>()
  // A view returns thousands of track pieces. regionOf is a point-in-outline test (China's outline has tens of thousands of vertices):
  // it was 1.8 s of a 4 s idle at Chengdu z10, throttle 4, all of it freezing the main thread just when the pointer starts to move. So it
  // runs once per ~5 km cell, and a piece already queued (same region, same line id or railway name; ids are numbered per region) is skipped first.
  const cellRegion = new Map<string, string>()
  for (const f of map.queryRenderedFeatures({ layers })) {
    if (todo.size >= PREFETCH_MAX) break
    const ln = Number(String(f.properties.ln ?? '').split(',')[0])
    const isLine = Number.isFinite(ln) && ln > 0
    if (!isLine && !f.properties.n) continue
    let c = (f.geometry as GeoJSON.LineString).coordinates as unknown
    while (Array.isArray(c) && Array.isArray(c[0])) c = c[0]
    const cell = Array.isArray(c) ? `${Math.round(Number(c[0]) * 20)},${Math.round(Number(c[1]) * 20)}` : ''
    let slug = cellRegion.get(cell)
    if (slug === undefined) { slug = regionOf(f); if (cell) cellRegion.set(cell, slug) }
    const key = isLine ? `${slug}|${ln}` : `${slug}|n|${f.properties.n}`
    if (todo.has(key)) continue
    const s = slug
    todo.set(key, isLine ? () => loadLineHighlight(s, ln) : () => loadNameHighlight(s, String(f.properties.n)))
  }
  const jobs = [...todo.values()]
  let complete = true
  // two at a time (a shard is up to ~470 KB and r2.dev serves HTTP/1.1: six connections shared with the tiles); the rest is dropped as soon
  // as the map moves again (or a newer prefetch starts), so a pan's tile requests do not queue behind a prefetch nobody is waiting for
  const run = async () => {
    while (jobs.length) {
      if (map.isMoving() || gen !== prefetchGen) { complete = false; return }
      await jobs.shift()!().catch(() => null)
    }
  }
  return Promise.all([run(), run()]).then(() => complete)
}

export function setFocus(map: MLMap, slug: string | null) {
  if (map.getLayer(L(TILES, 'veil'))) map.setPaintProperty(L(TILES, 'veil'), 'background-opacity', slug ? FOCUS_VEIL : 0)
}
const FOCUS_VEIL = 0.72
