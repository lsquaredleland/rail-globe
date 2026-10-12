import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react'
import maplibregl, { type GeoJSONSource, type Map as MLMap, type MapGeoJSONFeature } from 'maplibre-gl'
import { Protocol } from 'pmtiles'
import 'maplibre-gl/dist/maplibre-gl.css'
import { addAirportIcon, addRegion, applyState, areaLayers, baseStyle, highlightLine, hitLayer, stationLayers, strandLayer, yardHitLayer, type MapState } from './mapStyle'
import { REGIONS } from './regions'

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
  selectedLine: number | null
  selectedTrackName: string | null
  selectedStation: [number, number] | null
  touch: boolean
  onHover(hit: Hit | null, point: { x: number; y: number }): void
  onClick(hit: Hit | null, point: { x: number; y: number }): void
  onMoveEnd(v: View): void
}

let protocolAdded = false

function slugOf(layerId: string) { return layerId.split(':')[0] }

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
    resizeSoon: () => setTimeout(() => mapRef.current?.resize(), 50),
  }))

  useEffect(() => {
    if (!protocolAdded) {
      maplibregl.addProtocol('pmtiles', new Protocol().tile)
      protocolAdded = true
    }
    const map = new maplibregl.Map({
      container: el.current!, style: baseStyle(), center: p.current.initial.center, zoom: p.current.initial.zoom,
      minZoom: 2, maxZoom: 17, attributionControl: false, dragRotate: false, pitchWithRotate: false, maxPitch: 0,
      fadeDuration: 0,
    })
    mapRef.current = map
    if (import.meta.env.DEV) (window as unknown as { __map: MLMap }).__map = map
    map.touchZoomRotate.disableRotation()
    map.addControl(new maplibregl.AttributionControl({ compact: true }), 'bottom-right')
    const sc = new maplibregl.ScaleControl({ maxWidth: 120, unit: p.current.units === 'km' ? 'metric' : 'imperial' })
    scale.current = sc
    map.addControl(sc, 'bottom-left')

    map.on('load', () => {
      for (const r of REGIONS) addRegion(map, r.slug)
      map.addSource('selection', { type: 'geojson', data: { type: 'FeatureCollection', features: [] } })
      map.addLayer({ id: 'selection', type: 'circle', source: 'selection',
        paint: { 'circle-radius': 15, 'circle-color': 'rgba(0,0,0,0)', 'circle-stroke-color': '#1E1E1C', 'circle-stroke-width': 2, 'circle-stroke-opacity': 0.55 } })
      addAirportIcon(map)
      loaded.current = true
      applyState(map, p.current.state)
      highlightLine(map, p.current.selectedLine, p.current.selectedTrackName)
      updateSelection(map, p.current.selectedStation)
    })

    const hitAt = (pt: { x: number; y: number }): Hit | null => {
      if (!loaded.current) return null
      const box: [[number, number], [number, number]] = [[pt.x - 6, pt.y - 6], [pt.x + 6, pt.y + 6]]
      const ll = map.unproject([pt.x, pt.y])
      const lngLat: [number, number] = [ll.lng, ll.lat]
      const q = (layers: string[]) => map.queryRenderedFeatures(box, { layers: layers.filter((l) => map.getLayer(l)) })
      const toProps = (f: MapGeoJSONFeature) => f.properties as Record<string, string | number>

      const st = q(REGIONS.flatMap((r) => stationLayers(r.slug)))
      if (st.length) {
        const best = [...st].sort((a, b) => Number(a.properties.tier) - Number(b.properties.tier))[0]
        const g = best.geometry as GeoJSON.Point
        return { kind: 'station', slug: slugOf(best.layer.id), props: toProps(best), lngLat: g.coordinates as [number, number] }
      }
      if (p.current.state.mode === 'operator' && map.getZoom() >= 12) {
        const ln = q(REGIONS.flatMap((r) => [strandLayer(r.slug), `${strandLayer(r.slug)}-hs`]))
          .filter((f) => !p.current.state.hsrOnly || f.properties.t === 'high_speed')
        if (ln.length) return { kind: 'line', slug: slugOf(ln[0].layer.id), props: toProps(ln[0]), lngLat }
      }
      const tr = q(REGIONS.map((r) => hitLayer(r.slug)))
      if (tr.length) {
        // most prominent type wins when tracks overlap
        const order = ['high_speed', 'intercity', 'regional', 'metro', 'light_rail', 'monorail', 'funicular', 'other', 'freight']
        const best = [...tr].sort((a, b) => order.indexOf(String(a.properties.type)) - order.indexOf(String(b.properties.type)))[0]
        return { kind: 'track', slug: slugOf(best.layer.id), props: toProps(best), lngLat }
      }
      const yd = p.current.state.hsrOnly ? [] : q(REGIONS.map((r) => yardHitLayer(r.slug)))
      if (yd.length) return { kind: 'track', slug: slugOf(yd[0].layer.id), props: toProps(yd[0]), lngLat }
      // station grounds, yards, buildings and platforms (polygons are hit inside, with no tolerance)
      const pt1 = map.queryRenderedFeatures([pt.x, pt.y], { layers: REGIONS.flatMap((r) => areaLayers(r.slug)).filter((l) => map.getLayer(l)) })
      if (pt1.length) return { kind: 'area', slug: slugOf(pt1[0].layer.id), props: toProps(pt1[0]), lngLat }
      return null
    }

    map.on('mousemove', (e) => {
      if (p.current.touch) return
      const hit = hitAt(e.point)
      map.getCanvas().style.cursor = hit ? 'pointer' : ''
      p.current.onHover(hit, e.point)
    })
    map.on('mouseout', () => p.current.onHover(null, { x: 0, y: 0 }))
    map.on('click', (e) => p.current.onClick(hitAt(e.point), e.point))
    const showZoom = () => { if (zoomEl.current) zoomEl.current.textContent = `z${map.getZoom().toFixed(2)}` }
    map.on('move', showZoom); showZoom()
    map.on('movestart', () => p.current.onHover(null, { x: 0, y: 0 }))
    map.on('moveend', () => {
      const c = map.getCenter()
      p.current.onMoveEnd({ center: [c.lng, c.lat], zoom: map.getZoom() })
    })
    return () => { map.remove(); mapRef.current = null; loaded.current = false }
  }, [])

  useEffect(() => { const m = mapRef.current; if (m && loaded.current) applyState(m, props.state) }, [props.state])
  useEffect(() => { const m = mapRef.current; if (m && loaded.current) highlightLine(m, props.selectedLine, props.selectedTrackName) }, [props.selectedLine, props.selectedTrackName])
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
