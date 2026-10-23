import type { CnCity, CnDst, CnRecord, CnSnapshot, CnStretches } from './data'

/** What the map shows for a clicked China station with trains: the ids of the stations its trains stop at, and its stretches by width band. */
export interface CnFocus {
  rec: CnRecord
  key: string
  lit: string[]
  bands: number[][]
  cities: string[]
  ids: Record<string, string> // any station or complex id -> the key of the record (ids.json)
}

export const hm = (m: number) => `${Math.floor(m / 60)}h${String(m % 60).padStart(2, '0')}`
export const clock = (m: number) => `${String(Math.floor((m % 1440) / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`
/** The station panel footnote crediting the 12306 timetable snapshot days and train classes. */
export function snapshotText(s: CnSnapshot): string {
  const days = s.map((d) => (d.classes.length ? `${d.date} (${d.classes.map((c) => (c === 'plain' ? 'other' : c)).join(', ')})` : d.date))
  return `Timetables: 12306, one day each: ${days.join(', ')}`
}

/** The stretch numbers of each width band: the record stores them ascending, each as the step from the one before (the first from 0). */
export function decodeBands(st: number[][]): number[][] {
  return st.map((steps) => {
    let at = 0
    return steps.map((d) => (at += d))
  })
}

/** The stations of the record, its destinations, and any other id of theirs (a complex's, ids.json) stay lit; everything else fades. */
export function cnFocus(key: string, rec: CnRecord, ids: Record<string, string>): CnFocus {
  const own = new Set([key, ...Object.keys(rec.dst)])
  const lit = new Set(own)
  for (const [id, to] of Object.entries(ids)) if (own.has(to)) lit.add(id)
  return { rec, key, lit: [...lit], bands: decodeBands(rec.st), cities: rec.dest.map((c) => c[0]), ids }
}

/** The destination cities as rows: the first `head` (all when `all`) and how many are left. */
export function cityRows(dest: CnCity[], all: boolean, head = 8): { rows: CnCity[]; more: number } {
  return { rows: all ? dest : dest.slice(0, head), more: Math.max(0, dest.length - head) }
}
/** A city served through more than one station expands into one row per station. */
export const canExpand = (c: CnCity) => c[4].length > 1

/** The record's trains to a hovered station: found by its station id or its complex id (the destinations are stored by station id). */
export function destLookup(rec: CnRecord, ids: Record<string, string>, id: string, cx: string): { key: string; d: CnDst } | null {
  for (const k of [id, cx]) {
    for (const c of [k, ids[k]]) if (c && Object.prototype.hasOwnProperty.call(rec.dst, c)) return { key: c, d: rec.dst[c] }
  }
  return null
}

/** The record's English name of a station its trains stop at (a tile's own English name is raw OSM text, "(China Railway)" and all), found by its Chinese name. */
const bare = (s: string) => s.replace(/\s+/g, '').replace(/站$/, '')
export function partEnglish(rec: CnRecord, zh: string): string | null {
  for (const c of rec.dest) for (const p of c[4]) if (bare(p[0]) === bare(zh)) return p[1]
  return null
}

/** The way files hold their stretches' ways by the 2 degree cell of each way's middle: the cells (`<lon>_<lat>`) that cover a view, `pad` degrees wider. */
export const CELL_DEG = 2
export function wayCells([w, s, e, n]: [number, number, number, number], pad = 0.5): string[] {
  const out: string[] = []
  for (let x = Math.floor((w - pad) / CELL_DEG); x <= Math.floor((e + pad) / CELL_DEG); x++)
    for (let y = Math.floor((s - pad) / CELL_DEG); y <= Math.floor((n + pad) / CELL_DEG); y++) out.push(`${x}_${y}`)
  return out
}

/** The tile ids ("way/123") of the ways of each band's stretches, from the way cells loaded so far. */
export function bandWays(cells: Record<string, number[]>[], bands: number[][]): string[][] {
  return bands.map((stretches) => {
    const ids = new Set<number>()
    for (const cell of cells) for (const s of stretches) for (const w of cell[s] ?? []) ids.add(w)
    return [...ids].sort((a, b) => a - b).map((w) => `way/${w}`)
  })
}

/** A stored line: the first point absolute in 1/q degrees, each next one a step from the one before (x0, y0, dx1, dy1, ...). */
export function decodeLine(part: number[], q: number): [number, number][] {
  const out: [number, number][] = []
  let x = 0
  let y = 0
  for (let i = 0; i < part.length; i += 2) {
    x += part[i]
    y += part[i + 1]
    out.push([x / q, y / q])
  }
  return out
}

/** The simplified geometry of the record's stretches for views below z9: one line feature per stretch, with its rail type and band. */
export function stretchFeatures(list: CnStretches, bands: number[][]): GeoJSON.Feature[] {
  return bands.flatMap((stretches, band) =>
    stretches
      .filter((s) => list.s[s])
      .map((s) => ({
        type: 'Feature' as const,
        properties: { type: list.s[s].t, band },
        geometry: { type: 'MultiLineString' as const, coordinates: list.s[s].c.map((p) => decodeLine(p, list.q)) },
      }))
  )
}
