import type { GeoJSONSource, Map as MLMap } from 'maplibre-gl'
import { bandWays, stretchFeatures, wayCells, type CnFocus } from '../cnServices'
import { loadCnCells, loadCnStretches, loadCnWays } from '../data'
import { TILES } from '../regions'
import { L, type Expr, type RailCtx } from './shared'
import { fadePlaces, restorePlaces } from './places'
import { typeColor } from './trackStyle'

/** A clicked China station's stretches (cn12306), in five widths by how many of its trains use them, coloured by rail type. From z9 the track pieces of the
 *  highlight source are filtered by way id (the same source as the line highlight, so nothing is filtered on the main source); below z9 a GeoJSON source
 *  holds the stretches' simplified geometry. Both are set once per click. */
const BAND_W = [1.8, 2.8, 4, 5.5, 7.5]
// The geometry layer fades out over z9 to GEO_MAX while the way-filtered track pieces (from z9) are already there: no blank frame while the way cells or tiles load.
const GEO_MAX = 9.6
const geoFade = ['interpolate', ['linear'], ['zoom'], 9, 1, GEO_MAX, 0] as Expr
const NONE = ['==', ['get', 'id'], ''] as Expr
const EMPTY: GeoJSON.FeatureCollection = { type: 'FeatureCollection', features: [] }
const src = (source: string) => L(source, 'cn12306')
const hlsId = (k: number) => L(TILES, `cn-hls-${k}`)

export function addCnServiceLayers({ map, source, hlLayer }: RailCtx) {
  map.addSource(src(source), { type: 'geojson', data: EMPTY })
  const width = (k: number): Expr => ['interpolate', ['linear'], ['zoom'], 4, BAND_W[k] * 0.85, 9, BAND_W[k], 13, BAND_W[k] * 1.5] as Expr
  const layout = { 'line-cap': 'round', 'line-join': 'round' }
  BAND_W.forEach((_, k) => {
    hlLayer({
      id: L(source, `cn-geo-${k}`),
      type: 'line',
      source: src(source),
      maxzoom: GEO_MAX,
      filter: ['==', ['get', 'band'], k],
      layout,
      paint: { 'line-color': typeColor, 'line-width': width(k), 'line-opacity': geoFade },
    })
    hlLayer({
      id: L(source, `cn-hls-${k}`),
      type: 'line',
      source: L(source, 'hls'),
      'source-layer': 'track',
      minzoom: 9,
      filter: NONE,
      layout,
      paint: { 'line-color': typeColor, 'line-width': width(k) },
    })
  })
}

let run = 0 // the click being shown; an answer for an older one is dropped
let onMove: (() => void) | null = null
let shown = false

/** Show a station's stretches and fade the place names away from its trains; null puts everything back. */
export function setCnServices(map: MLMap, cn: CnFocus | null) {
  const mine = ++run
  if (onMove) map.off('moveend', onMove)
  onMove = null
  if (shown) {
    shown = false
    ;(map.getSource(src(TILES)) as GeoJSONSource | undefined)?.setData(EMPTY)
    for (let k = 0; k < BAND_W.length; k++) if (map.getLayer(hlsId(k))) map.setFilter(hlsId(k), NONE)
    restorePlaces(map)
  }
  if (!cn || !map.getSource(src(TILES))) return
  shown = true
  fadePlaces(map, cn.rec.pts, cn.cities)
  const cells = new Map<string, Record<string, number[]>>() // way cells in, for this click
  const asked = new Map<string, Promise<void>>() // in flight; a failed one is dropped, so the next move asks again
  let geo = false
  let filtered = -1
  const draw = async () => {
    if (map.getZoom() < GEO_MAX && !geo) {
      const list = await loadCnStretches().catch(() => null)
      if (list && mine === run) {
        geo = true
        ;(map.getSource(src(TILES)) as GeoJSONSource).setData({ type: 'FeatureCollection', features: stretchFeatures(list, cn.bands) })
      }
    }
    if (map.getZoom() < 9) return
    const have = await loadCnCells().catch(() => null)
    if (!have) return
    const b = map.getBounds()
    const need = wayCells([b.getWest(), b.getSouth(), b.getEast(), b.getNorth()]).filter((c) => have.has(c) && !cells.has(c) && !asked.has(c))
    for (const c of need)
      asked.set(
        c,
        loadCnWays(c).then(
          (w) => void cells.set(c, w),
          () => void asked.delete(c)
        )
      )
    await Promise.all(asked.values())
    if (mine !== run || cells.size === filtered) return
    filtered = cells.size
    bandWays([...cells.values()], cn.bands).forEach((ids, k) =>
      map.setFilter(hlsId(k), ids.length ? (['in', ['get', 'id'], ['literal', ids]] as Expr) : NONE, { validate: false })
    )
  }
  onMove = () => void draw()
  map.on('moveend', onMove)
  void draw()
}
