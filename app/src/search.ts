import { useCallback, useEffect, useState } from 'react'
import { DATA_BASE } from './config'
import type { View } from './MapView'

/** One row of search.json (written by `rail-extract places`): every country and province / state that has rail, and the cities with a
 *  metro or light rail system, one per metro area. */
export interface SearchItem {
  name: string
  nameEn: string | null
  local: string | null
  kind: 'country' | 'province' | 'city'
  center: [number, number]
  /** west, south, east, north of the outline; null for a city that has only a point */
  bbox: [number, number, number, number] | null
  /** zoom that fits the outline, or a fixed zoom by size for a city */
  zoom: number
  /** the area's own slug (its stats / outline files), null for a city without an outline */
  slug: string | null
  stations: number
  /** lower-case, accent-free name, English name and local name, for matching */
  key: string
}

type Row = [
  string,
  string | null,
  string | null,
  SearchItem['kind'],
  number,
  number,
  number | null,
  number | null,
  number | null,
  number | null,
  number,
  string | null,
  number,
]

const fold = (s: string) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase()

function parseSearch(rows: Row[]): SearchItem[] {
  return rows.map((r) => ({
    name: r[0],
    nameEn: r[1],
    local: r[2],
    kind: r[3],
    center: [r[4], r[5]],
    bbox: r[6] == null || r[7] == null || r[8] == null || r[9] == null ? null : [r[6], r[7], r[8], r[9]],
    zoom: r[10],
    slug: r[11],
    stations: r[12],
    key: fold([r[0], r[1], r[2]].filter(Boolean).join('\n')),
  }))
}

let loading: Promise<SearchItem[] | null> | null = null
/** search.json, fetched once; null when it could not be loaded */
function loadSearch(): Promise<SearchItem[] | null> {
  loading ??= fetch(`${DATA_BASE}search.json`)
    .then((r) => (r.ok ? r.json() : Promise.reject(r.status)))
    .then(parseSearch)
    .catch(() => {
      loading = null
      return null
    }) // a failure is not kept: the next call tries again
  return loading
}
export type SearchStatus = 'loading' | 'ready' | 'failed'
export function useSearch(): { items: SearchItem[]; status: SearchStatus; retry(): void } {
  const [state, setState] = useState<{ items: SearchItem[]; status: SearchStatus }>({ items: [], status: 'loading' })
  const [tries, setTries] = useState(0)
  useEffect(() => {
    let live = true
    loadSearch().then((i) => {
      if (live) setState(i ? { items: i, status: 'ready' } : { items: [], status: 'failed' })
    })
    return () => {
      live = false
    }
  }, [tries])
  const retry = useCallback(() => {
    setState({ items: [], status: 'loading' })
    setTries((n) => n + 1)
  }, [])
  return { ...state, retry }
}

const KIND_RANK = { country: 0, city: 1, province: 2 } as const

/** Items whose name, English name or local name starts with the query (or a word of it) come first, then those that contain it; within a
 *  group countries, then cities, then provinces, the bigger rail network first. */
export function searchItems(items: SearchItem[], query: string, limit = 12): SearchItem[] {
  const q = fold(query.trim())
  if (!q) return []
  const scored: { it: SearchItem; s: number }[] = []
  for (const it of items) {
    const i = it.key.indexOf(q)
    if (i < 0) continue
    const wordStart = i === 0 || /[\s\n\-'(]/.test(it.key[i - 1])
    scored.push({ it, s: (wordStart ? (i === 0 || it.key[i - 1] === '\n' ? 0 : 1) : 2) * 10 + KIND_RANK[it.kind] })
  }
  scored.sort((a, b) => a.s - b.s || b.it.stations - a.it.stations)
  return dropTwinProvinces(scored.slice(0, limit * 3).map((x) => x.it)).slice(0, limit)
}

const foldedNames = new WeakMap<SearchItem, string[]>()
const names = (it: SearchItem) => {
  let n = foldedNames.get(it)
  if (!n) foldedNames.set(it, (n = [it.name, it.nameEn].filter(Boolean).map((x) => fold(x!.trim()))))
  return n
}
const inBox = (b: SearchItem['bbox'], [x, y]: [number, number]) => !!b && x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3]
const area = (b: SearchItem['bbox']) => (b ? (b[2] - b[0]) * (b[3] - b[1]) : 0)

/** Tokyo, Cairo and Moscow are both a city and a province: keep the city row and drop the province row of the same name whose box holds the
 *  city's point (or the city's box holds the province's centre), but only when the two are about the same size (the province box is at most
 *  4 times the city's, or at most 4 square degrees (Beijing is 3.4) when the city has no outline). New York state, Buenos Aires province and the like stay.
 *  A collapsed city with no outline of its own takes the province's box, zoom and slug, so the view still fits the whole area. */
function dropTwinProvinces(list: SearchItem[]): SearchItem[] {
  const cities = list.filter((i) => i.kind === 'city')
  const dropped = new Map<SearchItem, SearchItem>()
  const twinOf = new Map<SearchItem, SearchItem>()
  for (const p of list) {
    if (p.kind !== 'province') continue
    const pn = names(p)
    const c = cities.find(
      (c) =>
        names(c).some((n) => pn.includes(n)) &&
        (inBox(p.bbox, c.center) || inBox(c.bbox, p.center)) &&
        (c.bbox ? area(p.bbox) <= 4 * area(c.bbox) : area(p.bbox) <= 4)
    )
    if (c) {
      twinOf.set(p, c)
      dropped.set(c, p)
    }
  }
  return list
    .filter((i) => !twinOf.has(i))
    .map((i) => {
      const p = dropped.get(i)
      return p && !i.bbox ? { ...i, bbox: p.bbox, zoom: p.zoom, slug: p.slug } : i
    })
}

/** What to list before anything is typed: the biggest rail networks, countries then cities */
export function featured(items: SearchItem[], each = 6): SearchItem[] {
  const top = (k: SearchItem['kind']) =>
    items
      .filter((i) => i.kind === k)
      .sort((a, b) => b.stations - a.stations)
      .slice(0, each)
  return [...top('country'), ...top('city')]
}

const viewSpan = (v: View, screenW: number) => (360 * screenW) / (512 * 2 ** v.zoom) // degrees of longitude on screen

/** `part`: the data region (country) the view centre is in, when known. The country, province or city the view is over, for the region button before the statistics of its country have loaded: the smallest
 *  outline that holds the view centre and spans at least half the screen; else a city whose point is near the centre when zoomed in. */
export function itemInView(items: SearchItem[], v: View, screenW: number, part = ''): SearchItem | null {
  const [lon, lat] = v.center
  const vs = viewSpan(v, screenW)
  let best: SearchItem | null = null
  let bestSpan = Infinity
  for (const it of items) {
    if (!it.bbox) continue
    // `part`: the country the outlines say the centre is in. Boxes of countries and provinces with far islands or across the antimeridian (US, Netherlands, Alaska) hold other continents.
    if (part && (it.kind === 'country' ? it.slug !== part : !it.slug?.startsWith(part + '-'))) continue
    const [w, s, e, n] = it.bbox
    if (lon < w || lon > e || lat < s || lat > n) continue
    const span = e - w
    if (span < vs * 0.5 || span >= bestSpan) continue
    best = it
    bestSpan = span
  }
  if (!best && part && v.zoom >= 3.5) best = items.find((it) => it.kind === 'country' && it.slug === part) ?? null // the country is bigger than the screen
  if (v.zoom >= 8.5) {
    const near = items.filter((it) => it.kind === 'city' && !it.bbox && Math.hypot(it.center[0] - lon, (it.center[1] - lat) * 1.2) < vs * 0.25)
    if (near.length) return near.sort((a, b) => b.stations - a.stations)[0]
  }
  return best
}
