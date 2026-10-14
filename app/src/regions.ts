import { DATA_BASE } from './config'

export interface Region {
  slug: string
  name: string
  sub: string
  local?: string
  center: [number, number] // lon, lat
  zoom: number
  zoomPhone: number
  /** the region whose tiles hold this one, when it lies inside a bigger region: its own outline, Lines list and side files,
   *  but its rail is the bigger region's, so overlapping regions are not loaded and drawn twice */
  tiles?: string
  /** no card of its own: an area of the bigger region (a city, a province) already is this region */
  noCard?: boolean
}

export const REGIONS: Region[] = [
  { slug: 'sf-bay-area', name: 'SF Bay Area', sub: '9 counties', center: [-122.22, 37.7], zoom: 9.3, zoomPhone: 8.4 },
  { slug: 'greater-bay-area', name: 'Greater Bay Area', sub: 'Pearl River Delta + HK + Macao', local: '粤港澳大湾区', center: [113.55, 22.75], zoom: 8.2, zoomPhone: 7.3, tiles: 'china' },
  { slug: 'paris', name: 'Paris', sub: 'Île-de-France', center: [2.35, 48.86], zoom: 10.6, zoomPhone: 9.6 },
  { slug: 'chengdu', name: 'Chengdu', sub: 'Municipality', local: '成都', center: [104.07, 30.66], zoom: 10.2, zoomPhone: 9.2, tiles: 'china', noCard: true },
  { slug: 'cn-beijing', name: 'Beijing', sub: 'Municipality', local: '北京', center: [116.4, 39.93], zoom: 9.6, zoomPhone: 8.6, tiles: 'china', noCard: true },
  { slug: 'china', name: 'China', sub: 'Mainland + HK + Macao', local: '中国', center: [108.0, 34.5], zoom: 3.8, zoomPhone: 2.8 },
]

export const regionBySlug = (s: string) => REGIONS.find((r) => r.slug === s) ?? REGIONS[1]

/** The one tile source on the map: the sf-bay-area, paris and china PMTiles joined (rail-extract build, join_pilot). Style layer ids are `pilot:<name>`. */
export const TILES = 'pilot'
type Part = { slug: string; bounds: [number, number, number, number] }
let parts: Part[] = []
/** The per-region tile files that were joined, with their data bounds (PMTiles headers), from pilot.parts.json (written by the join in
 *  rail-extract). Await once at startup before `partAt` / `regionOf`. They do not overlap. Tiles carry no region field, so a
 *  feature's region (its side files: tags, lines, stations, highlights) is found from where it lies. */
export async function loadParts() {
  const r = await fetch(`${DATA_BASE}${TILES}.parts.json`)
  if (!r.ok) throw new Error(`${TILES}.parts.json: ${r.status}`)
  parts = await r.json()
}
/** the joined file a point (lon, lat) belongs to: the part whose bounds hold it, else the nearest */
export function partAt([lng, lat]: [number, number]): string {
  const d = (b: number[]) => Math.hypot(Math.max(b[0] - lng, 0, lng - b[2]), Math.max(b[1] - lat, 0, lat - b[3]))
  return parts.reduce((a, p) => (d(p.bounds) < d(a.bounds) ? p : a)).slug
}
/** the region (a joined file's slug) a rendered feature belongs to, from its first point; `fallback` (a pointer position) when that is not a numeric pair */
export function regionOf(f: { geometry: GeoJSON.Geometry }, fallback?: [number, number]): string {
  let c = (f.geometry as GeoJSON.LineString | GeoJSON.Polygon | GeoJSON.Point).coordinates as unknown
  while (Array.isArray(c) && Array.isArray(c[0])) c = c[0]
  const ok = Array.isArray(c) && typeof c[0] === 'number' && typeof c[1] === 'number'
  const pt = ok ? (c as [number, number]) : fallback
  return pt ? partAt(pt) : parts[0].slug
}
