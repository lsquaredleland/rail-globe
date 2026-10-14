import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react'
import * as maplibregl from 'maplibre-gl'   // MapLibre 6 is ESM-only: no default export
import type { GeoJSONSource, Map as MLMap, MapGeoJSONFeature } from 'maplibre-gl'
import { Protocol } from 'pmtiles'
import 'maplibre-gl/dist/maplibre-gl.css'
import { addAirportIcon, addBoundary, addRailSource, applyState, areaLayers, baseStyle, highlightLine, hitLayer, prefetchHighlights, setFocus, stationLayers, strandLayer, yardHitLayer, type MapState } from './mapStyle'
import { loadParts, regionOf, REGIONS, TILES } from './regions'

export type Hit =
  | { kind: 'station'; slug: string; props: Record<string, string | number>; lngLat: [number, number] }
  | { kind: 'line'; slug: string; props: Record<string, string | number>; lngLat: [number, number] }
  | { kind: 'track'; slug: string; props: Record<string, string | number>; lngLat: [number, number] }
  | { kind: 'area'; slug: string; props: Record<string, string | number>; lngLat: [number, number] }

export interface View { center: [number, number]; zoom: number }
export interface MapHandle {
  flyTo(v: View): void
  fitBounds(b: [number, number, number, number]): void
  getZoom(): number
  zoomBy(d: number): void
  resizeSoon(): void
}

interface Props {
  state: MapState
  units: 'km' | 'mi'
  initial: View
  highlightSlug: string | null   // the region selectedLine / selectedTrackName belong to
  selectedLine: number | null
  selectedTrackName: string | null
  /** What a hit highlights (its line, else its named railway), for highlighting straight from the hit test, without a React
   *  render in between. null when hovering must not highlight (a line is selected, touch screens). */
  hoverHighlight: ((hit: Hit | null) => { slug: string; line: number | null; name: string | null } | null) | null
  focusSlug: string | null   // a line or railway is selected (clicked): the region whose lines stay, all others fade
  selectedStation: [number, number] | null
  touch: boolean
  onHover(hit: Hit | null, point: { x: number; y: number }): void
  onClick(hit: Hit | null, point: { x: number; y: number }): void
  onMoveEnd(v: View): void
}

let protocolAdded = false
// MapLibre 6 keeps its tile worker in two module files next to the library, which a bundler (and Vite's dependency cache) does
// not carry along: production uses the copies vite.config.ts emits, dev the files in node_modules
maplibregl.setWorkerUrl(new URL(import.meta.env.PROD ? 'maplibre/maplibre-gl-worker.mjs' : 'node_modules/maplibre-gl/dist/maplibre-gl-worker.mjs', document.baseURI).href)

export const MapView = forwardRef<MapHandle, Props>(function MapView(props, ref) {
  const el = useRef<HTMLDivElement>(null)
  const mapRef = useRef<MLMap | null>(null)
  const zoomEl = useRef<HTMLDivElement>(null) // debug readout, remove when no longer needed
  const loaded = useRef(false)
  const p = useRef(props)
  p.current = props
  const scale = useRef<maplibregl.ScaleControl | null>(null)

  useImperativeHandle(ref, () => ({
    fitBounds: (b) => mapRef.current?.fitBounds([[b[0], b[1]], [b[2], b[3]]], { padding: window.innerWidth > 720 ? { top: 90, left: 60, right: 430, bottom: 60 } : { top: 80, left: 30, right: 30, bottom: 380 }, maxZoom: 12.5, duration: 900 }),
    flyTo: (v) => mapRef.current?.flyTo({ center: v.center, zoom: v.zoom, duration: 900, essential: true }),
    getZoom: () => mapRef.current?.getZoom() ?? 0,
    zoomBy: (d) => mapRef.current?.zoomTo((mapRef.current?.getZoom() ?? 0) + d, { duration: 250 }),
    // only when the container really changed size: map.resize() reallocates the canvas and fires move events
    resizeSoon: () => setTimeout(() => {
      const m = mapRef.current
      const c = m?.getContainer()
      if (m && c && (c.clientWidth !== m.getCanvas().clientWidth || c.clientHeight !== m.getCanvas().clientHeight)) m.resize()
    }, 50),
  }))

  useEffect(() => {
    if (!protocolAdded) {
      maplibregl.addProtocol('pmtiles', new Protocol().tile)
      protocolAdded = true
    }
    const map = new maplibregl.Map({
      container: el.current!, style: baseStyle(), center: p.current.initial.center, zoom: p.current.initial.zoom,
      minZoom: 2, maxZoom: 17, attributionControl: false, dragRotate: false, pitchWithRotate: false, maxPitch: 0,
      fadeDuration: 100,   // 0 makes MapLibre redo full label placement every frame (never paused); any fade keeps it time-sliced
    })
    mapRef.current = map
    if (import.meta.env.DEV || location.search.includes('debug')) (window as unknown as { __map: MLMap }).__map = map   // ?debug: for profiling a production build
    map.touchZoomRotate.disableRotation()
    map.addControl(new maplibregl.AttributionControl({ compact: true }), 'bottom-right')
    const sc = new maplibregl.ScaleControl({ maxWidth: 120, unit: p.current.units === 'km' ? 'metric' : 'imperial' })
    scale.current = sc
    map.addControl(sc, 'bottom-left')

    map.on('load', async () => {
      await loadParts()   // the joined files' bounds: needed to find a feature's region
      if (mapRef.current !== map) return   // removed while waiting
      for (const r of REGIONS) addBoundary(map, r.slug)
      addRailSource(map, TILES)   // one source: the pilot regions' tiles joined; regions inside China are drawn from China's part
      map.addSource('selection', { type: 'geojson', data: { type: 'FeatureCollection', features: [] } })
      map.addLayer({ id: 'selection', type: 'circle', source: 'selection',
        paint: { 'circle-radius': 15, 'circle-color': 'rgba(0,0,0,0)', 'circle-stroke-color': '#1E1E1C', 'circle-stroke-width': 2, 'circle-stroke-opacity': 0.55 } })
      addAirportIcon(map)
      loaded.current = true
      applyState(map, p.current.state)
      highlightLine(map, p.current.highlightSlug, p.current.selectedLine, p.current.selectedTrackName)
      setFocus(map, p.current.focusSlug)
      updateSelection(map, p.current.selectedStation)
    })

    const hitAt = (pt: { x: number; y: number }): Hit | null => {
      if (!loaded.current) return null
      const box: [[number, number], [number, number]] = [[pt.x - 6, pt.y - 6], [pt.x + 6, pt.y + 6]]
      const ll = map.unproject([pt.x, pt.y])
      const lngLat: [number, number] = [ll.lng, ll.lat]
      const q = (layers: string[]) => map.queryRenderedFeatures(box, { layers: layers.filter((l) => map.getLayer(l)) })
      // the region (sf-bay-area, paris, china) whose side files a feature's ids refer to: found from where the feature lies
      const slugOf = (f: MapGeoJSONFeature) => regionOf(f, lngLat)
      const toProps = (f: MapGeoJSONFeature) => f.properties as Record<string, string | number>

      const st = q(stationLayers(TILES))
      if (st.length) {
        const best = [...st].sort((a, b) => Number(a.properties.tier) - Number(b.properties.tier))[0]
        const g = best.geometry as GeoJSON.Point
        return { kind: 'station', slug: slugOf(best), props: toProps(best), lngLat: g.coordinates as [number, number] }
      }
      if (p.current.state.mode === 'operator' && map.getZoom() >= 12) {
        const ln = q([strandLayer(TILES), `${strandLayer(TILES)}-hs`])
          .filter((f) => !p.current.state.hsrOnly || f.properties.t === 'high_speed')
        if (ln.length) return { kind: 'line', slug: slugOf(ln[0]), props: toProps(ln[0]), lngLat }
      }
      const tr = q([hitLayer(TILES)])
      if (tr.length) {
        // most prominent type wins when tracks overlap
        const order = ['high_speed', 'intercity', 'regional', 'metro', 'light_rail', 'monorail', 'funicular', 'other', 'freight']
        const best = [...tr].sort((a, b) => order.indexOf(String(a.properties.type)) - order.indexOf(String(b.properties.type)))[0]
        return { kind: 'track', slug: slugOf(best), props: toProps(best), lngLat }
      }
      const yd = p.current.state.hsrOnly ? [] : q([yardHitLayer(TILES)])
      if (yd.length) return { kind: 'track', slug: slugOf(yd[0]), props: toProps(yd[0]), lngLat }
      // station grounds, yards, buildings and platforms (polygons are hit inside, with no tolerance)
      const pt1 = map.queryRenderedFeatures([pt.x, pt.y], { layers: areaLayers(TILES).filter((l) => map.getLayer(l)) })
      if (pt1.length) return { kind: 'area', slug: slugOf(pt1[0]), props: toProps(pt1[0]), lngLat }
      return null
    }

    // at most one hit test per frame, and no React update while the pointer moves over empty map
    let pending: { x: number; y: number } | null = null
    let hovering = false
    map.on('mousemove', (e) => {
      if (p.current.touch) return
      if (!pending) requestAnimationFrame(() => {
        const pt = pending!
        pending = null
        const hit = hitAt(pt)
        map.getCanvas().style.cursor = hit ? 'pointer' : ''
        if (!hit && !hovering) return
        hovering = !!hit
        const hh = p.current.hoverHighlight
        if (hh) { const t = hh(hit); highlightLine(map, t?.slug ?? null, t?.line ?? null, t?.name ?? null) }
        p.current.onHover(hit, pt)
      })
      pending = { x: e.point.x, y: e.point.y }
    })
    map.on('mouseout', () => { hovering = false; p.current.onHover(null, { x: 0, y: 0 }) })
    map.on('click', (e) => p.current.onClick(hitAt(e.point), e.point))
    const showZoom = () => { if (zoomEl.current) zoomEl.current.textContent = `z${map.getZoom().toFixed(2)}` }
    map.on('move', showZoom); showZoom()
    map.on('movestart', () => { hovering = false; p.current.onHover(null, { x: 0, y: 0 }) })
    // when the map settles in a view it has not prefetched yet (also the first one), load the highlight data of the lines on screen
    let prefetched = ''
    map.on('idle', () => {
      const c = map.getCenter()
      const key = `${c.lng.toFixed(3)}/${c.lat.toFixed(3)}/${map.getZoom().toFixed(2)}`
      if (!loaded.current || key === prefetched) return
      prefetched = key
      setTimeout(() => { if (!map.isMoving()) prefetchHighlights(map) }, 150)
    })
    map.on('moveend', () => {
      const c = map.getCenter()
      p.current.onMoveEnd({ center: [c.lng, c.lat], zoom: map.getZoom() })
    })
    return () => { map.remove(); mapRef.current = null; loaded.current = false }
  }, [])

  useEffect(() => { const m = mapRef.current; if (m && loaded.current) applyState(m, props.state) }, [props.state])
  useEffect(() => { const m = mapRef.current; if (m && loaded.current) highlightLine(m, props.highlightSlug, props.selectedLine, props.selectedTrackName) }, [props.highlightSlug, props.selectedLine, props.selectedTrackName])
  useEffect(() => { const m = mapRef.current; if (m && loaded.current) setFocus(m, props.focusSlug) }, [props.focusSlug])
  useEffect(() => { const m = mapRef.current; if (m && loaded.current) updateSelection(m, props.selectedStation) }, [props.selectedStation])
  useEffect(() => {
    const m = mapRef.current
    if (!m || !scale.current) return
    m.removeControl(scale.current)
    scale.current = new maplibregl.ScaleControl({ maxWidth: 120, unit: props.units === 'km' ? 'metric' : 'imperial' })
    m.addControl(scale.current, 'bottom-left')
  }, [props.units])

  return <><div ref={el} className="map" /><div ref={zoomEl} className="zoom-debug" aria-hidden="true" /></>
})

function updateSelection(map: MLMap, pt: [number, number] | null) {
  const src = map.getSource('selection') as GeoJSONSource | undefined
  if (!src) return
  src.setData({ type: 'FeatureCollection', features: pt ? [{ type: 'Feature', properties: {}, geometry: { type: 'Point', coordinates: pt } }] : [] })
}
