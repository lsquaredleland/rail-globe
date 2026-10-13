import { useEffect, useState } from 'react'
import { loadShapes, loadStats, type Shapes, type StatsArea } from './data'
import { REGIONS } from './regions'
import type { View } from './MapView'

export interface StatsData {
  /** every pilot region's own totals (its bounds say where the region is) */
  regions: StatsArea[]
  /** areas inside the regions that draw from their own tiles: provinces, cities, Hong Kong */
  areas: StatsArea[]
  shapes: Shapes
}

/** Loads the statistics of every region once. */
export function useStats(): StatsData {
  const [d, setD] = useState<StatsData>({ regions: [], areas: [], shapes: {} })
  useEffect(() => {
    let live = true
    Promise.all(REGIONS.map((r) => Promise.all([loadStats(r.slug), loadShapes(r.slug)]))).then((all) => {
      if (!live) return
      const regions = all.map(([a], i) => a.find((x) => x.slug === REGIONS[i].slug)).filter((a): a is StatsArea => !!a)
      // regions inside a bigger region (Chengdu, Beijing, ...) are cards of the bigger region's areas already
      const own = new Set(REGIONS.filter((r) => !r.tiles).map((r) => r.slug))
      const areas = all.flatMap(([a], i) => (own.has(REGIONS[i].slug) ? a : []))
      setD({ regions, areas, shapes: Object.assign({}, ...all.map(([, s]) => s)) })
    })
    return () => { live = false }
  }, [])
  return d
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

/** The smallest of `areas` that holds the view's centre and is not much smaller than the view (its width is at least a fifth of
 *  the screen's), else null. Areas with an outline are tested against it, the rest against their bounds. */
export function areaForView<T extends StatsArea>(areas: T[], shapes: Shapes, v: View, screenW: number): T | null {
  const [lon, lat] = v.center
  const vs = viewSpan(v, screenW)
  let best: T | null = null
  let bestSpan = Infinity
  for (const a of areas) {
    const [w, s, e, n] = a.bbox
    if (lon < w || lon > e || lat < s || lat > n) continue
    const span = e - w
    if (span < vs * 0.2 || span >= bestSpan) continue
    const shape = shapes[a.slug]
    if (shape && !inRings(shape, lon, lat)) continue
    best = a
    bestSpan = span
  }
  return best
}
