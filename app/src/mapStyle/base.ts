import type { GeoJSONSource, Map as MLMap, StyleSpecification } from 'maplibre-gl'
import { FILL } from '../palette'
import { boundaryUrl } from '../data'
import { L } from './shared'

const OFM = 'https://tiles.openfreemap.org/planet'
export const ATTRIBUTION = '© <a href="https://www.openstreetmap.org/copyright" target="_blank" rel="noopener">OpenStreetMap</a> contributors'

export function baseStyle(): StyleSpecification {
  return {
    version: 8,
    sources: { ofm: { type: 'vector', url: OFM, attribution: '' } },
    layers: [
      { id: 'background', type: 'background', paint: { 'background-color': FILL.land } },
      {
        id: 'landcover-wood',
        type: 'fill',
        source: 'ofm',
        'source-layer': 'landcover',
        filter: ['in', ['get', 'class'], ['literal', ['wood', 'forest']]],
        paint: { 'fill-color': FILL.park },
      },
      {
        id: 'landcover-farm',
        type: 'fill',
        source: 'ofm',
        'source-layer': 'landcover',
        filter: ['in', ['get', 'class'], ['literal', ['farmland', 'grass']]],
        paint: { 'fill-color': FILL.land },
      },
      {
        id: 'landuse-urban',
        type: 'fill',
        source: 'ofm',
        'source-layer': 'landuse',
        filter: ['in', ['get', 'class'], ['literal', ['residential', 'commercial', 'industrial', 'retail']]],
        paint: { 'fill-color': FILL.land },
      },
      { id: 'park', type: 'fill', source: 'ofm', 'source-layer': 'park', paint: { 'fill-color': FILL.park } },
      { id: 'water', type: 'fill', source: 'ofm', 'source-layer': 'water', paint: { 'fill-color': FILL.water } },
      {
        id: 'waterway',
        type: 'line',
        source: 'ofm',
        'source-layer': 'waterway',
        minzoom: 9,
        paint: { 'line-color': FILL.water, 'line-width': ['interpolate', ['linear'], ['zoom'], 9, 0.4, 14, 1.6] },
      },
      {
        id: 'aeroway-apron',
        type: 'fill',
        source: 'ofm',
        'source-layer': 'aeroway',
        minzoom: 10,
        filter: ['all', ['==', ['geometry-type'], 'Polygon'], ['in', ['get', 'class'], ['literal', ['apron', 'runway', 'taxiway', 'helipad']]]],
        paint: { 'fill-color': '#D9D3C4', 'fill-opacity': 0.9 },
      },
      {
        id: 'aeroway-taxiway',
        type: 'line',
        source: 'ofm',
        'source-layer': 'aeroway',
        minzoom: 10,
        filter: ['all', ['==', ['geometry-type'], 'LineString'], ['==', ['get', 'class'], 'taxiway']],
        paint: { 'line-color': '#BDB6A4', 'line-width': ['interpolate', ['linear'], ['zoom'], 10, 0.5, 14, 2.4, 17, 8] },
      },
      {
        id: 'aeroway-runway',
        type: 'line',
        source: 'ofm',
        'source-layer': 'aeroway',
        minzoom: 10,
        filter: ['all', ['==', ['geometry-type'], 'LineString'], ['==', ['get', 'class'], 'runway']],
        layout: { 'line-cap': 'butt' },
        paint: { 'line-color': '#A49C88', 'line-width': ['interpolate', ['linear'], ['zoom'], 10, 1.6, 12, 3, 14, 7, 17, 30] },
      },
      {
        id: 'road-major',
        type: 'line',
        source: 'ofm',
        'source-layer': 'transportation',
        minzoom: 7,
        filter: ['all', ['in', ['get', 'class'], ['literal', ['motorway', 'trunk', 'primary']]], ['!=', ['get', 'brunnel'], 'tunnel']],
        layout: { 'line-cap': 'round', 'line-join': 'round' },
        paint: { 'line-color': FILL.road, 'line-width': ['interpolate', ['linear'], ['zoom'], 7, 0.5, 12, 1.4, 16, 3.5] },
      },
      {
        id: 'road-secondary',
        type: 'line',
        source: 'ofm',
        'source-layer': 'transportation',
        minzoom: 11,
        filter: ['all', ['==', ['get', 'class'], 'secondary'], ['!=', ['get', 'brunnel'], 'tunnel']],
        layout: { 'line-cap': 'round', 'line-join': 'round' },
        paint: { 'line-color': FILL.road, 'line-width': ['interpolate', ['linear'], ['zoom'], 11, 0.4, 16, 2.4] },
      },
      {
        id: 'admin',
        type: 'line',
        source: 'ofm',
        'source-layer': 'boundary',
        minzoom: 3,
        filter: ['all', ['==', ['get', 'admin_level'], 4], ['!=', ['get', 'maritime'], 1]],
        layout: { 'line-join': 'round' },
        paint: { 'line-color': FILL.boundary, 'line-width': 0.8, 'line-dasharray': [6, 5] },
      },
      {
        id: 'admin-country',
        type: 'line',
        source: 'ofm',
        'source-layer': 'boundary',
        minzoom: 1,
        filter: ['all', ['==', ['get', 'admin_level'], 2], ['!=', ['get', 'maritime'], 1]],
        layout: { 'line-join': 'round' },
        paint: { 'line-color': FILL.boundary, 'line-width': ['interpolate', ['linear'], ['zoom'], 2, 0.7, 8, 1.5] },
      },
    ],
  }
}

/** A region's faint outline (every region, also those drawn from a bigger region's tiles). Added before the rail layers. */
export function addBoundary(map: MLMap, slug: string) {
  map.addSource(L(slug, 'boundary'), { type: 'geojson', data: boundaryUrl(slug) })
  map.addLayer({
    id: L(slug, 'boundary'),
    type: 'line',
    source: L(slug, 'boundary'),
    layout: { 'line-join': 'round' },
    paint: { 'line-color': '#B9B5AD', 'line-width': 1.1, 'line-dasharray': [6, 5], 'line-opacity': 0.8 },
  })
}

/** The outline of the area in view (the "Outline area in view" switch): a clear line with a soft inner shade, under the rail. Fed by `setOutline`. */
export function addOutline(map: MLMap) {
  map.addSource('area-outline', { type: 'geojson', data: { type: 'FeatureCollection', features: [] } })
  map.addLayer({
    id: 'area-outline-shade',
    type: 'line',
    source: 'area-outline',
    layout: { 'line-join': 'round' },
    paint: { 'line-color': '#1E1E1C', 'line-width': 12, 'line-blur': 10, 'line-offset': 5, 'line-opacity': 0.1 },
  })
  map.addLayer({
    id: 'area-outline',
    type: 'line',
    source: 'area-outline',
    layout: { 'line-join': 'round' },
    paint: { 'line-color': '#1E1E1C', 'line-width': 1.8, 'line-opacity': 0.6 },
  })
}
export function setOutline(map: MLMap, geometry: GeoJSON.Geometry | null) {
  ;(map.getSource('area-outline') as GeoJSONSource | undefined)?.setData({
    type: 'FeatureCollection',
    features: geometry ? [{ type: 'Feature', properties: {}, geometry }] : [],
  })
}
