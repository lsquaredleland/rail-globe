import { useEffect, useMemo, useState } from 'react'
import { loadShapes, loadStats, type Shapes, type StatsArea } from './data'
import { regionBySlug } from './regions'
import type { SearchItem } from './search'
import type { View } from './MapView'

export interface StatsData {
  /** every loaded region's own totals (its bounds say where the region is) */
  regions: StatsArea[]
  /** areas inside the regions that draw from their own tiles: provinces, cities, Hong Kong */
  areas: StatsArea[]
  shapes: Shapes
}

type Loaded = Record<string, [StatsArea[], Shapes]>

/** The statistics of the regions the view can be over. Pilot: the fixed list of regions, once. World: the countries (search.json) whose
 *  outline box holds the view centre, loaded as the map moves there and kept, so only the countries the user visits are fetched. */
export function useStats(items: SearchItem[], view: View, picked: string): StatsData {
  const [loaded, setLoaded] = useState<Loaded>({})
  const [lon, lat] = view.center
  const wanted = useMemo(() => {
    const w = view.zoom < 3.2 ? [] : items.filter((i) => i.kind === 'country' && i.slug && i.bbox && lon >= i.bbox[0] && lon <= i.bbox[2] && lat >= i.bbox[1] && lat <= i.bbox[3]).map((i) => i.slug!)
    return picked && items.some((i) => i.slug === picked && i.kind === 'country') ? [...new Set([...w, picked])] : w
  }, [items, lon, lat, view.zoom, picked])
  useEffect(() => {
    let live = true
    for (const slug of wanted) {
      if (loaded[slug]) continue
      Promise.all([loadStats(slug), loadShapes(slug)]).then((r) => { if (live) setLoaded((l) => (l[slug] ? l : { ...l, [slug]: r })) })
    }
    return () => { live = false }
  }, [wanted, loaded])
  return useMemo(() => {
    const slugs = Object.keys(loaded)
    const regions = slugs.map((s) => loaded[s][0].find((x) => x.slug === s)).filter((a): a is StatsArea => !!a)
    // regions inside a bigger region (Chengdu, Beijing, ...) are cards of the bigger region's areas already
    const areas = slugs.flatMap((s) => (regionBySlug(s).tiles ? [] : loaded[s][0].filter((a) => a.kind !== 'region')))   // a region's own row is in `regions`
    return { regions, areas, shapes: Object.assign({}, ...slugs.map((s) => loaded[s][1])) }
  }, [loaded])
}

const viewSpan = (v: View, screenW: number) => (360 * screenW) / (512 * 2 ** v.zoom)   // degrees of longitude on screen

function inRings(polys: number[][][][], x: number, y: number): boolean {
  let inside = false
  for (const poly of polys) for (const ring of poly) {
    for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
      const [xi, yi] = ring[i], [xj, yj] = ring[j]
      if ((yi > y) !== (yj > y) && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside
    }
  }
  return inside
}

/** The smallest of `areas` that holds the view's centre and fills at least half the screen (its width is at least half the
 *  screen's, so all of China is not shown as one province), else null. Areas with an outline are tested against it, the rest against their bounds. */
export function areaForView<T extends StatsArea>(areas: T[], shapes: Shapes, v: View, screenW: number, minShare = 0.5): T | null {
  const [lon, lat] = v.center
  const vs = viewSpan(v, screenW)
  let best: T | null = null
  let bestSpan = Infinity
  for (const a of areas) {
    const [w, s, e, n] = a.bbox
    if (lon < w || lon > e || lat < s || lat > n) continue
    const span = e - w
    if (span < vs * minShare || span >= bestSpan) continue
    const shape = shapes[a.slug]
    if (shape && !inRings(shape, lon, lat)) continue
    best = a
    bestSpan = span
  }
  return best
}
