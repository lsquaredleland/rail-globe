import { DATA_BASE } from './config'
import type { SearchItem } from './search'

export interface Region {
  slug: string
  name: string
  sub: string
  local?: string
  center: [number, number] // lon, lat
  zoom: number
  zoomPhone: number
  /** no card of its own: an area of the bigger region (a city, a province) already is this region */
  noCard?: boolean
}

/** Where the map starts without a link: the whole world */
export const DEFAULT_REGION: Region = {
  slug: 'world',
  name: 'World',
  sub: 'All countries with rail',
  center: [15, 28],
  zoom: 2.2,
  zoomPhone: 1.6,
  noCard: true,
}

const dynamic = new Map<string, Region>()
const SUB = { country: 'Country', province: 'Province / state', city: 'City' } as const
/** Makes the countries and provinces of search.json known as regions (a link to `#r=de-bavaria` finds its centre and zoom).  */
export function registerSearch(items: SearchItem[]) {
  for (const it of items) {
    if (!it.slug || dynamic.has(it.slug)) continue
    dynamic.set(it.slug, {
      slug: it.slug,
      name: it.nameEn ?? it.name,
      sub: SUB[it.kind],
      local: it.local ?? undefined,
      center: it.center,
      zoom: it.zoom,
      zoomPhone: Math.max(1.5, it.zoom - 1),
    })
  }
}
/** A region by slug: one made from search.json, else (an unknown slug) a placeholder that names the slug and sits at the default view. Never another region. */
export const regionBySlug = (s: string): Region => dynamic.get(s) ?? { ...DEFAULT_REGION, slug: s, name: s, sub: '', local: undefined, noCard: true }
export const isKnownRegion = (s: string) => dynamic.has(s)

/** The one tile source on the map: the countries' PMTiles joined into world.pmtiles (rail-extract world). Style layer ids are `world:<name>`. */
export const TILES = 'world'
type Part = { slug: string; bounds: [number, number, number, number] }
let parts: Part[] = []
/** `partAt` answers '' before world.parts.json is in, and the smallest bounds before the outlines of overlapping countries are: whatever is computed from it must run again then */
const partListeners = new Set<() => void>()
export const onPartsChange = (cb: () => void) => {
  partListeners.add(cb)
  return () => {
    partListeners.delete(cb)
  }
}
const partsChanged = () => partListeners.forEach((cb) => cb())
/** The per-region sources that were joined, with their outline bounds, from world.parts.json (written by the join in
 *  rail-extract). Await once at startup before `partAt` / `regionOf`. Bounds may overlap (see `partAt`). Tiles carry no region field, so a
 *  feature's region (its side files: tags, lines, stations, highlights) is found from where it lies. */
export async function loadParts() {
  const r = await fetch(`${DATA_BASE}${TILES}.parts.json`)
  if (!r.ok) throw new Error(`${TILES}.parts.json: ${r.status}`)
  parts = await r.json()
  partsChanged()
}
/** Outlines are fetched only where bounds overlap (neighbouring countries), never all at once: the world has 142 of them, 12 MB */
const outlineLoads = new Map<string, Promise<void>>()
const loadOuter = (slug: string) => {
  let p = outlineLoads.get(slug)
  if (!p) {
    p = fetch(`${DATA_BASE}${slug}.boundary.geojson`)
      .then((x) => (x.ok ? x.json() : null))
      .then((j: GeoJSON.Feature | null) => {
        const g = j?.geometry
        outers[slug] = !g ? [] : g.type === 'Polygon' ? [g.coordinates[0]] : g.type === 'MultiPolygon' ? g.coordinates.map((q) => q[0]) : []
      })
      .catch(() => {
        outers[slug] = []
      })
      .then(partsChanged)
    outlineLoads.set(slug, p)
  }
  return p
}
const boundsHold = (b: number[], lng: number, lat: number) => lng >= b[0] && lng <= b[2] && lat >= b[1] && lat <= b[3]
/** Loads the outlines `partAt` needs near a point: those of the parts whose bounds overlap there. Call when the view settles, so the first hover finds the right country. */
export function warmParts([lng, lat]: [number, number]): Promise<void> {
  const near = parts.filter((p) => boundsHold(p.bounds, lng, lat))
  return near.length > 1 ? Promise.all(near.map((p) => loadOuter(p.slug))).then(() => {}) : Promise.resolve()
}
const outers: Record<string, number[][][]> = {}
const inRing = (ring: number[][], x: number, y: number) => {
  let inside = false
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const [xi, yi] = ring[i],
      [xj, yj] = ring[j]
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside
  }
  return inside
}
/** the joined file a point (lon, lat) belongs to. Part bounds come from the outlines and may overlap (neighbouring countries), so this resolves by outline: the part whose outline holds the point, else the nearest bounds */
export function partAt([lng, lat]: [number, number]): string {
  if (!parts.length) return '' // parts.json not loaded yet
  const d = (b: number[]) => Math.hypot(Math.max(b[0] - lng, 0, lng - b[2]), Math.max(b[1] - lat, 0, lat - b[3]))
  const near = parts.filter((p) => d(p.bounds) === 0)
  if (near.length === 1) return near[0].slug
  const pool = near.length ? near : parts
  const area = (b: number[]) => (b[2] - b[0]) * (b[3] - b[1])
  if (near.length > 1) for (const p of near) void loadOuter(p.slug) // not warmed yet: the smallest bounds answer meanwhile
  const inside = near.length ? near.find((p) => outers[p.slug]?.some((r) => inRing(r, lng, lat))) : undefined
  return (inside ?? pool.reduce((a, p) => (d(p.bounds) < d(a.bounds) || (d(p.bounds) === d(a.bounds) && area(p.bounds) < area(a.bounds)) ? p : a))).slug
}
/** the region (a joined file's slug) a rendered feature belongs to, from its first point; `fallback` (a pointer position) when that is not a numeric pair */
export function regionOf(f: { geometry: GeoJSON.Geometry }, fallback?: [number, number]): string {
  let c = (f.geometry as GeoJSON.LineString | GeoJSON.Polygon | GeoJSON.Point).coordinates as unknown
  while (Array.isArray(c) && Array.isArray(c[0])) c = c[0]
  const ok = Array.isArray(c) && typeof c[0] === 'number' && typeof c[1] === 'number'
  const pt = ok ? (c as [number, number]) : fallback
  return pt ? partAt(pt) : parts[0].slug
}

/** Fallback for `hsMinKmh` before stats.json is in: China (a region or an area in it) starts high-speed at 250 km/h, everywhere else 200 km/h (UIC) */
export const isChina = (slug: string) => slug === 'china' || slug === 'cn' || slug === 'greater-bay-area' || slug === 'chengdu' || slug.startsWith('cn-')
/** high-speed threshold per region, from the region row of its stats.json (`hs_min_kmh`, the config's `high_speed_min_kmh`) */
const hsMins = new Map<string, number>()
export const setHsMin = (slug: string, kmh: number) => {
  hsMins.set(slug, kmh)
}
/** where high-speed starts in a region or area: the stats.json value once loaded, else China's 250 / the UIC 200 (isChina) */
export const hsMinKmh = (slug: string) => hsMins.get(slug) ?? (isChina(slug) ? 250 : 200)
