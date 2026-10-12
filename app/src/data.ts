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
}

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

export type Tags = Record<string, string | string[]>
/** Raw (filtered) OSM tags plus Rail Globe derived fields for one feature, from the id-sharded side files. */
export async function loadFeatureTags(slug: string, osmId: string): Promise<Tags | null> {
  const n = Number(osmId.split('/')[1])
  if (!Number.isFinite(n)) return null
  const shard = await get<Record<string, Tags>>(`${slug}/tags/${n % 256}.json`)
  return shard[osmId] ?? null
}
