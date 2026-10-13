import { useEffect, useState } from 'react'
import { loadStats, type StatsArea } from '../data'
import { SOURCES } from '../regions'
import { TYPE_COLOR, TYPE_LABEL, TYPE_ORDER, type RailType } from '../palette'
import { km as fmtKm } from '../format'
import type { View } from '../MapView'

/** The smallest area that holds the view's centre and is not much smaller than the view; null when the view is far wider than
 *  any area (zoomed out over the sea, say). Width is in degrees of longitude. */
export function areaForView(areas: StatsArea[], v: View, screenW: number): StatsArea | null {
  const [lon, lat] = v.center
  const viewSpan = (360 * screenW) / (512 * 2 ** v.zoom)
  let best: StatsArea | null = null
  let bestSpan = Infinity
  for (const a of areas) {
    const [w, s, e, n] = a.bbox
    if (lon < w || lon > e || lat < s || lat > n) continue
    const span = e - w
    if (span < viewSpan * 0.2 || span >= bestSpan) continue
    best = a
    bestSpan = span
  }
  return best
}

const n0 = (n: number) => Math.round(n).toLocaleString('en-US')

export function RegionCard({ view, units }: { view: View; units: 'km' | 'mi' }) {
  const [areas, setAreas] = useState<StatsArea[]>([])
  useEffect(() => {
    let live = true
    Promise.all(SOURCES.map(loadStats)).then((all) => { if (live) setAreas(all.flat()) })
    return () => { live = false }
  }, [])
  const a = areaForView(areas, view, window.innerWidth)
  if (!a) return null
  const types = (TYPE_ORDER as string[]).concat('freight').filter((t) => a.km[t])
  const open = types.reduce((s, t) => s + a.km[t].open, 0)
  const con = types.reduce((s, t) => s + a.km[t].con, 0)
  const st = a.stations
  return (
    <aside className="region-card" aria-label={`Rail in ${a.name}`}>
      <div className="rc-head"><strong>{a.name}</strong>{a.local && a.local !== a.name && <span className="muted sm"> {a.local}</span>}</div>
      <div className="rc-total"><strong>{fmtKm(open, units)}</strong> <span className="muted sm">of track{con > 0 ? `, ${fmtKm(con, units)} under construction` : ''}</span></div>
      <div className="rc-rows">
        {types.map((t) => (
          <div key={t} className="rc-row">
            <i style={{ background: TYPE_COLOR[t as RailType] }} />
            <span className="grow">{TYPE_LABEL[t]}</span>
            <span>{fmtKm(a.km[t].open, units)}</span>
          </div>
        ))}
      </div>
      <div className="rc-foot muted sm">
        {st.open != null && <>{n0(st.open)} stations{st.metro ? ` · ${n0(st.metro)} metro` : ''}{st.light_rail ? ` · ${n0(st.light_rail)} tram / light rail` : ''}{st.construction ? ` · ${n0(st.construction)} being built` : ''}</>}
      </div>
      <div className="rc-foot muted sm">Every track counts: double track is measured twice. Lines run: {Object.values(a.lines).reduce((s, n) => s + n, 0)}.</div>
    </aside>
  )
}
