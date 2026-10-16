import type { RailType } from './palette'
import { DATA_BASE as base } from './config'

export interface LineIndexEntry { n: string | null; ne?: string | null; r: string | null; c: string | null; t: RailType; o: string | null; f: string | null; to: string | null; km: number }
export type LineIndex = Record<string, LineIndexEntry>

export interface StationRecord {
  id: string; name: string | null; name_en: string | null; tier: 1 | 2 | 3; construction?: boolean; terminus: boolean; hsr: boolean; modes: string[]
  platforms: number; platform_count: number; platform_refs: string[]; platform_refs_other: string[]
  line_count: number; lines: Record<string, number[]>; rail_lines?: { id: string; name: string | null; name_en: string | null }[]
  complex: { id: string; name: string | null; others: { id: string; name: string | null; name_en: string | null; modes: string[] }[] } | null
  airport?: string | null
  osm: string | null; wikidata: string[] | null; pt: [number, number]
  /** Wikidata id of the operator whose logo (logos/<id>.png) the card shows; null when we have none */
  logo?: string | null
  tags: { id: string; tags: Record<string, string> }[]; model: Record<string, string | number | boolean | null>
}
export interface LineRecord {
  id: number; name: string | null; name_en?: string | null; tags: Record<string, string>; route_ids: string[]; ref: string | null; type: RailType; colour: string | null; operator: string | null
  network: string | null; from: string | null; to: string | null; km: number; osm: string
  routes: { id: string; name: string | null; from: string | null; to: string | null; km: number; stations: string[] }[]
  station_names: Record<string, { n: string | null; ne: string | null }>
  hl?: Highlight
}

/** What a line or named railway is drawn with when highlighted: its track pieces' ids (a layer filter on the track layer, z9 and closer) and a
 *  simplified geometry (the merged overview below z9 has no per-piece ids). Written by pipeline/rail-extract/src/tiles.rs. */
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

export const loadLineIndex = (slug: string) => get<LineIndex>(`${slug}/lines-index.json`)
/** Per-feature side files are packed into hash shards, a few per region (shards.json, written by pipeline/rail-extract/src/tiles.rs):
 *  a record lives in `<kind>/<fnv1a(key) % count>.json`, an object keyed by the record's key. Lines whose record is large stay their own file in lines-big/. */
interface Shards { stations: number; lines: number; lines_big: number[]; tags: number; 'name-hl': number }
const loadShards = (slug: string) => get<Shards>(`${slug}/shards.json`)
async function fromShard<T>(slug: string, kind: 'stations' | 'lines' | 'tags' | 'name-hl', key: string): Promise<T | undefined> {
  const n = (await loadShards(slug))[kind]
  const shard = await get<Record<string, T>>(`${slug}/${kind}/${fnv1a(key) % n}.json`)
  return Object.prototype.hasOwnProperty.call(shard, key) ? shard[key] : undefined
}
export async function loadStation(slug: string, id: string): Promise<StationRecord> {
  const r = await fromShard<StationRecord>(slug, 'stations', id)
  if (!r) throw new Error(`${slug}: no station ${id}`)
  return r
}
export async function loadLine(slug: string, id: number | string): Promise<LineRecord> {
  const big = (await loadShards(slug)).lines_big.includes(Number(id))
  const r = big ? await get<LineRecord>(`${slug}/lines-big/${id}.json`) : await fromShard<LineRecord>(slug, 'lines', String(id))
  if (!r) throw new Error(`${slug}: no line ${id}`)
  return r
}
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
  (await fromShard<Highlight>(slug, 'name-hl', name)) ?? null

export type Tags = Record<string, string | string[]>
/** Raw (filtered) OSM tags plus Rail Globe derived fields for one feature, from the id-sharded side files. */
export async function loadFeatureTags(slug: string, osmId: string): Promise<Tags | null> {
  if (!Number.isFinite(Number(osmId.split('/')[1]))) return null
  return (await fromShard<Tags>(slug, 'tags', osmId)) ?? null
}

/** Rail totals for a region and the areas inside it: provinces and cities with a metro (pipeline/rail-extract/src/stats.rs).
 *  Length is route km, double track counted once (about 10% accurate). */
export interface StatsArea {
  slug: string; name: string; local: string | null; kind: 'region' | 'area' | 'province' | 'city'; bbox: [number, number, number, number]
  km: Record<string, { open: number; con: number; prop?: number }>
  /** open passenger route km by speed band: the band's lower bound in km/h, or 'none' without a usable maxspeed */
  speed?: Record<string, number>
  /** the same for passenger track under construction */
  speed_con?: Record<string, number>
  /** the same for proposed passenger track (a test scope: Hong Kong) */
  speed_prop?: Record<string, number>
  lines: Record<string, number>
  stations: Record<string, number>
}
const statsCache = new Map<string, Promise<StatsArea[]>>()
export function loadStats(slug: string): Promise<StatsArea[]> {
  let p = statsCache.get(slug)
  if (!p) {
    p = fetch(`${base}${slug}/stats.json`).then((r) => (r.ok ? r.json() : { areas: [] })).then((j: { areas: StatsArea[] }) => j.areas).catch(() => [])
    statsCache.set(slug, p)
  }
  return p
}

/** The outline of a pilot region (its boundary file), for the "outline the area in view" switch */
const boundaryCache = new Map<string, Promise<GeoJSON.Geometry | null>>()
export function loadBoundary(slug: string): Promise<GeoJSON.Geometry | null> {
  let p = boundaryCache.get(slug)
  if (!p) {
    p = fetch(boundaryUrl(slug)).then((r) => (r.ok ? r.json() : null)).then((j: GeoJSON.Feature | null) => j?.geometry ?? null).catch(() => null)
    boundaryCache.set(slug, p)
  }
  return p
}

/** Simplified outlines of a region's areas (polygons of rings of [lon, lat]), for finding the area under the view centre */
export type Shapes = Record<string, number[][][][]>
const shapesCache = new Map<string, Promise<Shapes>>()
export function loadShapes(slug: string): Promise<Shapes> {
  let p = shapesCache.get(slug)
  if (!p) {
    p = fetch(`${base}${slug}/shapes.json`).then((r) => (r.ok ? r.json() : {})).catch(() => ({}))
    shapesCache.set(slug, p)
  }
  return p
}
