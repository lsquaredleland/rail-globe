import { useEffect, useState } from 'react'
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

type Row = [string, string | null, string | null, SearchItem['kind'], number, number, number | null, number | null, number | null, number | null, number, string | null, number]

const fold = (s: string) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase()

export function parseSearch(rows: Row[]): SearchItem[] {
  return rows.map((r) => ({
    name: r[0], nameEn: r[1], local: r[2], kind: r[3], center: [r[4], r[5]],
    bbox: r[6] == null || r[7] == null || r[8] == null || r[9] == null ? null : [r[6], r[7], r[8], r[9]],
    zoom: r[10], slug: r[11], stations: r[12],
    key: fold([r[0], r[1], r[2]].filter(Boolean).join('\n')),
  }))
}

let loading: Promise<SearchItem[]> | null = null
/** search.json, fetched once; [] when the build has none */
export function loadSearch(): Promise<SearchItem[]> {
  loading ??= fetch(`${DATA_BASE}search.json`).then((r) => (r.ok ? r.json() : [])).then(parseSearch).catch(() => [])
  return loading
}
export function useSearch(): SearchItem[] {
  const [items, setItems] = useState<SearchItem[]>([])
  useEffect(() => { let live = true; loadSearch().then((i) => { if (live) setItems(i) }); return () => { live = false } }, [])
  return items
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
  return scored.slice(0, limit).map((x) => x.it)
}

/** What to list before anything is typed: the biggest rail networks, countries then cities */
export function featured(items: SearchItem[], each = 6): SearchItem[] {
  const top = (k: SearchItem['kind']) => items.filter((i) => i.kind === k).sort((a, b) => b.stations - a.stations).slice(0, each)
  return [...top('country'), ...top('city')]
}

const viewSpan = (v: View, screenW: number) => (360 * screenW) / (512 * 2 ** v.zoom)   // degrees of longitude on screen

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
  if (!best && part && v.zoom >= 3.5) best = items.find((it) => it.kind === 'country' && it.slug === part) ?? null   // the country is bigger than the screen
  if (v.zoom >= 8.5) {
    const near = items.filter((it) => it.kind === 'city' && !it.bbox && Math.hypot(it.center[0] - lon, (it.center[1] - lat) * 1.2) < vs * 0.25)
    if (near.length) return near.sort((a, b) => b.stations - a.stations)[0]
  }
  return best
}
