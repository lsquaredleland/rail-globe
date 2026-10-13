import type { RailType } from './palette'

const base = new URL('data/', document.baseURI).href

export interface LineIndexEntry { n: string | null; ne?: string | null; r: string | null; c: string | null; t: RailType; o: string | null; f: string | null; to: string | null; km: number }
export type LineIndex = Record<string, LineIndexEntry>

export interface StationRecord {
  id: string; name: string | null; name_en: string | null; tier: 1 | 2 | 3; construction?: boolean; terminus: boolean; hsr: boolean; modes: string[]
  platforms: number; platform_count: number; platform_refs: string[]; platform_refs_other: string[]
  line_count: number; lines: Record<string, number[]>; rail_lines?: { id: string; name: string | null; name_en: string | null }[]
  complex: { id: string; name: string | null; others: { id: string; name: string | null; name_en: string | null; modes: string[] }[] } | null
  airport?: string | null
  osm: string | null; wikidata: string[] | null; pt: [number, number]
  tags: { id: string; tags: Record<string, string> }[]; model: Record<string, string | number | boolean | null>
}
export interface LineRecord {
  id: number; name: string | null; name_en?: string | null; tags: Record<string, string>; route_ids: string[]; ref: string | null; type: RailType; colour: string | null; operator: string | null
  network: string | null; from: string | null; to: string | null; km: number; osm: string
  routes: { id: string; name: string | null; from: string | null; to: string | null; km: number; stations: string[] }[]
  station_names: Record<string, { n: string | null; ne: string | null }>
  hl?: Highlight
}

/** What a line or named railway is drawn with when highlighted: its track pieces' ids (feature state, z8 and closer) and a
 *  simplified geometry (the merged overview below z8 has no per-piece ids). Written by pipeline/rail-extract/src/tiles.rs. */
export interface Highlight { ways: string[]; geom: GeoJSON.MultiLineString | null; type: RailType; oc?: string }

export interface NamedLine { n: string; ne?: string | null; t: RailType; km: number; ways: number; bbox: [number, number, number, number] }

const cache = new Map<string, Promise<unknown>>()
function get<T>(path: string): Promise<T> {
  let p = cache.get(path)
  if (!p) {
    p = fetch(base + path).then((r) => { if (!r.ok) throw new Error(`${path}: ${r.status}`); return r.json() })
    p.catch(() => cache.delete(path))
    cache.set(path, p)
  }
  return p as Promise<T>
}

const safe = (id: string) => id.replace(/\//g, '-')
export const loadLineIndex = (slug: string) => get<LineIndex>(`${slug}/lines-index.json`)
export const loadStation = (slug: string, id: string) => get<StationRecord>(`${slug}/stations/${safe(id)}.json`)
export const loadLine = (slug: string, id: number | string) => get<LineRecord>(`${slug}/lines/${id}.json`)
export const pmtilesUrl = (slug: string) => `pmtiles://${base}${slug}.pmtiles`
export const boundaryUrl = (slug: string) => `${base}${slug}.boundary.geojson`
export const loadNamedLines = (slug: string) => get<NamedLine[]>(`${slug}/named-lines.json`)

/** Every railway name of a region, numbered (track pieces of the zoomed-out overview carry the number `nk`):
 *  [name, English name, most prominent rail type, length in km] */
export type RailwayEntry = [string, string | null, RailType, number]
export interface Railways { list: RailwayEntry[]; byName: Map<string, RailwayEntry> }
const railwaysCache = new Map<string, Promise<Railways>>()
export const loadRailways = (slug: string): Promise<Railways> => {
  let p = railwaysCache.get(slug)
  if (!p) {
    p = get<RailwayEntry[]>(`${slug}/railways.json`).then((list) => ({ list, byName: new Map(list.map((r) => [r[0], r])) }))
    p.catch(() => railwaysCache.delete(slug))
    railwaysCache.set(slug, p)
  }
  return p
}
/** a track's railway: by its name (zoomed in) or its number (zoomed-out overview) */
export const railwayOf = (rw: Railways | undefined, props: Record<string, string | number>): RailwayEntry | undefined =>
  !rw ? undefined : props.n != null ? rw.byName.get(String(props.n)) : props.nk != null ? rw.list[Number(props.nk)] : undefined

/** 32-bit FNV-1a of the UTF-8 bytes: the same hash rail-extract (tiles.rs) shards the named-railway highlight files by */
function fnv1a(s: string): number {
  let h = 0x811c9dc5
  for (const b of new TextEncoder().encode(s)) h = Math.imul(h ^ b, 0x01000193) >>> 0
  return h >>> 0
}
export const loadLineHighlight = async (slug: string, id: number) => (await loadLine(slug, id)).hl ?? null
export const loadNameHighlight = async (slug: string, name: string) =>
  (await get<Record<string, Highlight>>(`${slug}/name-hl/${fnv1a(name) % 256}.json`))[name] ?? null

export type Tags = Record<string, string | string[]>
/** Raw (filtered) OSM tags plus Rail Globe derived fields for one feature, from the id-sharded side files. */
export async function loadFeatureTags(slug: string, osmId: string): Promise<Tags | null> {
  const n = Number(osmId.split('/')[1])
  if (!Number.isFinite(n)) return null
  const shard = await get<Record<string, Tags>>(`${slug}/tags/${n % 256}.json`)
  return shard[osmId] ?? null
}
