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
  { slug: 'paris', name: 'Paris', sub: 'Île-de-France', center: [2.35, 48.86], zoom: 10.6, zoomPhone: 9.6, tiles: 'fr', noCard: true },
  { slug: 'fr', name: 'France', sub: 'Whole country', local: 'France', center: [2.6, 46.6], zoom: 5.2, zoomPhone: 4.3 },
  { slug: 'ch', name: 'Switzerland', sub: 'Whole country', local: 'Schweiz', center: [8.2, 46.8], zoom: 7, zoomPhone: 6 },
  { slug: 'it', name: 'Italy', sub: 'Whole country', local: 'Italia', center: [12.5, 42.4], zoom: 5.1, zoomPhone: 4.2 },
  { slug: 'chengdu', name: 'Chengdu', sub: 'Municipality', local: '成都', center: [104.07, 30.66], zoom: 10.2, zoomPhone: 9.2, tiles: 'china', noCard: true },
  { slug: 'cn-beijing', name: 'Beijing', sub: 'Municipality', local: '北京', center: [116.4, 39.93], zoom: 9.6, zoomPhone: 8.6, tiles: 'china', noCard: true },
  { slug: 'de', name: 'Germany', sub: 'Whole country', local: 'Deutschland', center: [10.45, 51.1], zoom: 5.4, zoomPhone: 4.5 },
  { slug: 'china', name: 'China', sub: 'Mainland + HK + Macao', local: '中国', center: [108.0, 34.5], zoom: 3.8, zoomPhone: 2.8 },
]

export const regionBySlug = (s: string) => REGIONS.find((r) => r.slug === s) ?? REGIONS[1]

/** The one tile source on the map: the per-region PMTiles (SF Bay Area, China and the countries) joined (rail-extract build, join_pilot). Style layer ids are `pilot:<name>`. */
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
  // the countries' bounding boxes overlap (France, Germany, Switzerland, Italy), so where they do the outlines decide
  await Promise.all(parts.map((p) => fetch(`${DATA_BASE}${p.slug}.boundary.geojson`).then((x) => (x.ok ? x.json() : null)).then((j: GeoJSON.Feature | null) => {
    const g = j?.geometry
    outers[p.slug] = !g ? [] : g.type === 'Polygon' ? [g.coordinates[0]] : g.type === 'MultiPolygon' ? g.coordinates.map((q) => q[0]) : []
  }).catch(() => { outers[p.slug] = [] })))
}
const outers: Record<string, number[][][]> = {}
const inRing = (ring: number[][], x: number, y: number) => {
  let inside = false
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const [xi, yi] = ring[i], [xj, yj] = ring[j]
    if ((yi > y) !== (yj > y) && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside
  }
  return inside
}
/** the joined file a point (lon, lat) belongs to: the part whose outline holds it (bounds first, outlines where they overlap), else the nearest */
export function partAt([lng, lat]: [number, number]): string {
  const d = (b: number[]) => Math.hypot(Math.max(b[0] - lng, 0, lng - b[2]), Math.max(b[1] - lat, 0, lat - b[3]))
  const near = parts.filter((p) => d(p.bounds) === 0)
  if (near.length === 1) return near[0].slug
  const pool = near.length ? near : parts
  const inside = near.length ? near.find((p) => outers[p.slug]?.some((r) => inRing(r, lng, lat))) : undefined
  return (inside ?? pool.reduce((a, p) => (d(p.bounds) < d(a.bounds) ? p : a))).slug
}
/** the region (a joined file's slug) a rendered feature belongs to, from its first point; `fallback` (a pointer position) when that is not a numeric pair */
export function regionOf(f: { geometry: GeoJSON.Geometry }, fallback?: [number, number]): string {
  let c = (f.geometry as GeoJSON.LineString | GeoJSON.Polygon | GeoJSON.Point).coordinates as unknown
  while (Array.isArray(c) && Array.isArray(c[0])) c = c[0]
  const ok = Array.isArray(c) && typeof c[0] === 'number' && typeof c[1] === 'number'
  const pt = ok ? (c as [number, number]) : fallback
  return pt ? partAt(pt) : parts[0].slug
}

/** China (a region or an area in it): high-speed starts at 250 km/h there, at 200 km/h everywhere else (UIC) */
export const isChina = (slug: string) => slug === 'china' || slug === 'greater-bay-area' || slug === 'chengdu' || slug.startsWith('cn-')
export const hsMinKmh = (slug: string) => (isChina(slug) ? 250 : 200)

// ---- where the map has rail: the regions' outlines, simplified, for the place labels (setPlaceRail)
function simplifyRing(ring: number[][], tol: number): number[][] {
  if (ring.length < 5) return ring
  const keep = new Uint8Array(ring.length)
  keep[0] = keep[ring.length - 1] = 1
  const stack: [number, number][] = [[0, ring.length - 1]]
  while (stack.length) {
    const [a, b] = stack.pop()!
    let max = 0, at = -1
    const [ax, ay] = ring[a], [bx, by] = ring[b]
    const dx = bx - ax, dy = by - ay, len2 = dx * dx + dy * dy || 1e-12
    for (let i = a + 1; i < b; i++) {
      const t = Math.max(0, Math.min(1, ((ring[i][0] - ax) * dx + (ring[i][1] - ay) * dy) / len2))
      const d = Math.hypot(ring[i][0] - (ax + t * dx), ring[i][1] - (ay + t * dy))
      if (d > max) { max = d; at = i }
    }
    if (max > tol && at > 0) { keep[at] = 1; stack.push([a, at], [at, b]) }
  }
  return ring.filter((_, i) => keep[i])
}
/** One MultiPolygon of the outlines of every region that has its own tiles, outer rings only, simplified to about 2 km. */
export async function loadRailArea(): Promise<GeoJSON.MultiPolygon> {
  const gs = await Promise.all(REGIONS.filter((r) => !r.tiles).map((r) => fetch(`${DATA_BASE}${r.slug}.boundary.geojson`).then((x) => (x.ok ? x.json() : null)).then((j: GeoJSON.Feature | null) => j?.geometry ?? null).catch(() => null)))
  const polys: GeoJSON.Position[][][] = []
  for (const g of gs) {
    const outers = !g ? [] : g.type === 'Polygon' ? [g.coordinates[0]] : g.type === 'MultiPolygon' ? g.coordinates.map((q) => q[0]) : []
    for (const o of outers) { const r = simplifyRing(o, 0.02); if (r.length >= 4) polys.push([r]) }
  }
  return { type: 'MultiPolygon', coordinates: polys }
}
