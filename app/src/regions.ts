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
  { slug: 'ar', name: 'Argentina', sub: 'Whole country', center: [-64, -38], zoom: 3.9, zoomPhone: 3.0 },
  { slug: 'bb', name: 'Barbados', sub: 'Whole country', center: [-59.55, 13.18], zoom: 9.5, zoomPhone: 8.6 },
  { slug: 'bo', name: 'Bolivia', sub: 'Whole country', center: [-64.7, -16.7], zoom: 4.9, zoomPhone: 4.0 },
  { slug: 'br', name: 'Brazil', sub: 'Whole country', local: 'Brasil', center: [-52, -14], zoom: 3.5, zoomPhone: 2.6 },
  { slug: 'ca', name: 'Canada', sub: 'Whole country', center: [-98, 56], zoom: 2.9, zoomPhone: 2.0 },
  { slug: 'cl', name: 'Chile', sub: 'Whole country', center: [-71, -33], zoom: 3.2, zoomPhone: 2.3 },
  { slug: 'co', name: 'Colombia', sub: 'Whole country', center: [-73.5, 4.2], zoom: 4.8, zoomPhone: 3.9 },
  { slug: 'cr', name: 'Costa Rica', sub: 'Whole country', center: [-84.2, 9.9], zoom: 6.9, zoomPhone: 6.0 },
  { slug: 'cu', name: 'Cuba', sub: 'Whole country', center: [-79, 21.7], zoom: 5.4, zoomPhone: 4.5 },
  { slug: 'do', name: 'Dominican Republic', sub: 'Whole country', local: 'República Dominicana', center: [-70.5, 18.9], zoom: 6.8, zoomPhone: 5.9 },
  { slug: 'ec', name: 'Ecuador', sub: 'Whole country', center: [-78.4, -1.5], zoom: 6, zoomPhone: 5.1 },
  { slug: 'gt', name: 'Guatemala', sub: 'Whole country', center: [-90.4, 15.6], zoom: 6.6, zoomPhone: 5.7 },
  { slug: 'hn', name: 'Honduras', sub: 'Whole country', center: [-86.6, 14.8], zoom: 6.6, zoomPhone: 5.7 },
  { slug: 'ht', name: 'Haiti', sub: 'Whole country', local: 'Ayiti', center: [-72.8, 19.0], zoom: 7, zoomPhone: 6.1 },
  { slug: 'jm', name: 'Jamaica', sub: 'Whole country', center: [-77.3, 18.15], zoom: 8.1, zoomPhone: 7.2 },
  { slug: 'kn', name: 'Saint Kitts and Nevis', sub: 'Whole country', center: [-62.75, 17.33], zoom: 10.5, zoomPhone: 9.6 },
  { slug: 'mx', name: 'Mexico', sub: 'Whole country', local: 'México', center: [-102, 23.6], zoom: 4.6, zoomPhone: 3.7 },
  { slug: 'ni', name: 'Nicaragua', sub: 'Whole country', center: [-85.2, 12.8], zoom: 6.5, zoomPhone: 5.6 },
  { slug: 'pa', name: 'Panama', sub: 'Whole country', local: 'Panamá', center: [-80, 8.6], zoom: 6.9, zoomPhone: 6.0 },
  { slug: 'pe', name: 'Peru', sub: 'Whole country', local: 'Perú', center: [-75.5, -9.5], zoom: 4.7, zoomPhone: 3.8 },
  { slug: 'py', name: 'Paraguay', sub: 'Whole country', local: 'Paraguay / Paraguái', center: [-58.4, -23.4], zoom: 5.8, zoomPhone: 4.9 },
  { slug: 'sv', name: 'El Salvador', sub: 'Whole country', center: [-88.9, 13.7], zoom: 8, zoomPhone: 7.1 },
  { slug: 'tc', name: 'Turks and Caicos Islands', sub: 'Whole country', center: [-71.8, 21.8], zoom: 8.5, zoomPhone: 7.6 },
  { slug: 'tt', name: 'Trinidad and Tobago', sub: 'Whole country', center: [-61.3, 10.5], zoom: 8.3, zoomPhone: 7.4 },
  { slug: 'us', name: 'United States', sub: 'Whole country', center: [-98, 39], zoom: 3.3, zoomPhone: 2.4 },
  { slug: 'uy', name: 'Uruguay', sub: 'Whole country', center: [-56, -32.8], zoom: 6.2, zoomPhone: 5.3 },
  { slug: 've', name: 'Venezuela', sub: 'Whole country', center: [-66.2, 7.6], zoom: 5.2, zoomPhone: 4.3 },
]

export const regionBySlug = (s: string) => REGIONS.find((r) => r.slug === s) ?? REGIONS[1]

/** The tile sources on the map, each a joined PMTiles file with its own `<name>.parts.json` (rail-extract build, join_pilot):
 *  the pilot regions, and the Americas. Style layer ids are `<source>:<name>`, one set per source. */
export const TILE_SOURCES = ['pilot', 'americas'] as const
type Part = { slug: string; bounds: [number, number, number, number]; source: string }
let parts: Part[] = []
/** The per-region tile files that were joined, with their data bounds (PMTiles headers), from each source's parts.json (written by the join in
 *  rail-extract). Await once at startup before `partAt` / `regionOf`. Within a source they do not overlap. Tiles carry no region field, so a
 *  feature's region (its side files: tags, lines, stations, highlights) is found from where it lies. */
export async function loadParts() {
  parts = (await Promise.all(TILE_SOURCES.map(async (source) => {
    const r = await fetch(`${DATA_BASE}${source}.parts.json`)
    if (!r.ok) throw new Error(`${source}.parts.json: ${r.status}`)
    return ((await r.json()) as Omit<Part, 'source'>[]).map((p) => ({ ...p, source }))
  }))).flat()
  // the countries' bounding boxes overlap (France, Germany, Switzerland, Italy, the Americas), so where they do the outlines decide
  await Promise.all(parts.map((p) => fetch(`${DATA_BASE}${p.slug}.boundary.geojson`).then((x) => (x.ok ? x.json() : null)).then((j: GeoJSON.Feature | null) => {
    const g = j?.geometry
    outers[p.slug] = !g ? [] : g.type === 'Polygon' ? [g.coordinates[0]] : g.type === 'MultiPolygon' ? g.coordinates.map((q) => q[0]) : []
  }).catch(() => { outers[p.slug] = [] })))
}
/** the tile source (style layer set) that draws a region: the source of its own part, else of the part it lies in (`tiles`) */
export function sourceOf(slug: string): string {
  const own = parts.find((p) => p.slug === slug) ?? parts.find((p) => p.slug === REGIONS.find((r) => r.slug === slug)?.tiles)
  return own?.source ?? TILE_SOURCES[0]
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
