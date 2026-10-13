import type { StatsArea } from '../data'
import { TYPE_COLOR, TYPE_LABEL, TYPE_ORDER, type RailType } from '../palette'
import { km as fmtKm } from '../format'

const n0 = (n: number) => Math.round(n).toLocaleString('en-US')
const KIND: Record<string, string> = { province: 'Province', city: 'City with a metro', region: 'Pilot region', area: 'Area' }

/** Rail totals for the area under the view centre (chosen in App by areaForView) */
export function RegionCard({ area: a, units }: { area: StatsArea | null; units: 'km' | 'mi' }) {
  if (!a) return null
  const types = (TYPE_ORDER as string[]).concat('freight').filter((t) => a.km[t])
  const open = types.reduce((s, t) => s + a.km[t].open, 0)
  const con = types.reduce((s, t) => s + a.km[t].con, 0)
  const st = a.stations
  return (
    <aside className="region-card" aria-label={`Rail in ${a.name}`}>
      <div className="rc-head"><strong>{a.name}</strong>{a.local && a.local !== a.name && <span lang="zh" className="muted sm"> {a.local}</span>}<span className="rc-kind muted sm">{KIND[a.kind]}</span></div>
      <div className="rc-total"><strong>{fmtKm(open, units)}</strong> <span className="muted sm">of route{con > 0 ? `, ${fmtKm(con, units)} under construction` : ''}</span></div>
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
      <div className="rc-foot muted sm">Route length: double track counts once (about 10% accuracy). {Object.values(a.lines).reduce((s, n) => s + n, 0)} lines.</div>
    </aside>
  )
}
