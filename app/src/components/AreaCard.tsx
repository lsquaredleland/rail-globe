import { Fragment } from 'react'
import type { StatsArea } from '../data'
import { INACTIVE, SPEED_BANDS, SPEED_NONE, SPEED_UNKNOWN } from '../mapStyle'
import { TYPE_COLOR, TYPE_LABEL, TYPE_ORDER, TYPE_WIDTH, type RailType } from '../palette'

type Units = 'km' | 'mi'
export const AREA_KIND: Record<string, string> = { province: 'Province', city: 'City', region: 'Pilot region', area: 'Area' }

const MI = 0.621371
const conv = (v: number, u: Units) => (u === 'km' ? v : v * MI)
/** a length without its unit: whole numbers from 100 up, one decimal below */
const num = (v: number, u: Units) => { const x = conv(v, u); return x >= 100 ? Math.round(x).toLocaleString('en-US') : x < 0.05 ? '0' : x.toFixed(1) }
const pct = (share: number) => (share < 0.005 ? '<1%' : `${Math.round(share * 100)}%`)
const n0 = (n: number) => Math.round(n).toLocaleString('en-US')

/** the smallest of 1, 2 or 5 x 10^n that keeps `total / unit` at or under `max` */
function niceUnit(total: number, max: number): number {
  for (let e = 0; e < 8; e++) for (const m of [1, 2, 5]) if (total / (m * 10 ** e) <= max) return m * 10 ** e
  return 1e8
}
const unitLabel = (v: number, u: Units) => `${Math.round(conv(v, u)).toLocaleString('en-US')} ${u}`

const HIDDEN_FILL = '#E6E3DC'

/** A grid of squares, one per `unit` of the total: each type takes its share in order, so a type smaller than a square is a part-filled one. */
function Squares({ parts, unit, cols = 21 }: { parts: { color: string; v: number; off?: boolean }[]; unit: number; cols?: number }) {
  const total = parts.reduce((s, p) => s + p.v, 0)
  const n = Math.max(1, Math.ceil(total / unit - 1e-9))
  const cells = Array.from({ length: n }, (_, i) => {
    const a = i * unit, b = (i + 1) * unit
    const stops: string[] = []
    let at = 0, from = 0
    for (const p of parts) {
      const lo = Math.max(from, a), hi = Math.min(from + p.v, b)
      if (hi > lo) { const x0 = ((lo - a) / unit) * 100, x1 = ((hi - a) / unit) * 100; const c = p.off ? HIDDEN_FILL : p.color; stops.push(`${c} ${x0}% ${x1}%`); at = x1 }
      from += p.v
    }
    if (at < 99.9) stops.push(`transparent ${at}% 100%`)
    return `linear-gradient(90deg, ${stops.join(', ')})`
  })
  return <div className="sq-grid" style={{ gridTemplateColumns: `repeat(${cols}, 9px)` }} aria-hidden="true">{cells.map((g, i) => <i key={i} style={{ background: g }} />)}</div>
}

function Outlined({ n, unit, max = 100 }: { n: number; unit: number; max?: number }) {
  const count = Math.min(max, Math.max(1, Math.ceil(n / unit - 1e-9)))
  return <div className="sq-grid out" aria-hidden="true">{Array.from({ length: count }, (_, i) => <i key={i} />)}</div>
}

function Dots({ parts, unit }: { parts: { color: string; v: number }[]; unit: number }) {
  const dots: string[] = []
  let acc = 0, placed = 0
  for (const p of parts) { acc += p.v; while ((placed + 1) * unit <= acc + unit / 2 && placed * unit < acc) { dots.push(p.color); placed++ } }
  return <div className="dot-grid" aria-hidden="true">{dots.map((c, i) => <i key={i} style={{ background: c }} />)}</div>
}

function Head({ area }: { area: StatsArea }) {
  return (
    <div className="ac-head">
      <strong>{area.name}</strong>{area.local && area.local !== area.name && <span lang="zh" className="ac-local">{area.local}</span>}
      <span className="muted sm ac-kind">{AREA_KIND[area.kind]}</span>
    </div>
  )
}

function Tools({ allOn, allOff, onAll, onNone }: { allOn: boolean; allOff: boolean; onAll(): void; onNone(): void }) {
  return (
    <span className="ac-tools">
      <button type="button" className="ac-btn" disabled={allOn} onClick={onAll}>All</button>
      <button type="button" className="ac-btn" disabled={allOff} onClick={onNone}>None</button>
    </span>
  )
}

/** Rail types of the area under the view: kilometres as a grid of squares, then each type as a switch for the map. */
export function TypeCard({ area: a, units, hidden, freight, construction, hsrOnly, onToggleType, onShowAll, onHideAll, onToggleFreight, onToggleConstruction }: {
  area: StatsArea | null; units: Units; hidden: string[]; freight: boolean; construction: boolean; hsrOnly: boolean
  onToggleType(t: string): void; onShowAll(): void; onHideAll(): void; onToggleFreight(): void; onToggleConstruction(): void
}) {
  if (!a) return null
  const types = TYPE_ORDER.filter((t) => a.km[t]?.open > 0)
  const open = (t: string) => a.km[t]?.open ?? 0
  const total = types.reduce((s, t) => s + open(t), 0)
  const con = types.concat('freight' as RailType).reduce((s, t) => s + (a.km[t]?.con ?? 0), 0)
  const freightKm = open('freight')
  const unit = niceUnit(total, 250)
  const st = a.stations
  const metro = st.metro ?? 0, tram = st.light_rail ?? 0, main = Math.max(0, (st.open ?? 0) - metro - tram)
  const sUnit = niceUnit(st.open ?? 0, 120)
  return (
    <aside className="area-card" aria-label={`Rail in ${a.name}`}>
      <Head area={a} />
      <div className="ac-total">
        <div><strong className="ac-big">{num(total, units)} {units}</strong><div className="muted sm">passenger route in service</div></div>
        <span className="ac-scale"><i />= {unitLabel(unit, units)}</span>
      </div>
      <Squares unit={unit} parts={types.map((t) => ({ color: hsrOnly && t !== 'high_speed' ? INACTIVE : TYPE_COLOR[t], v: open(t), off: hidden.includes(t) }))} />
      <div className="ac-sec"><span className="eyebrow">On the map</span>
        <Tools allOn={hidden.length === 0} allOff={TYPE_ORDER.every((t) => hidden.includes(t))} onAll={onShowAll} onNone={onHideAll} /></div>
      <div className="ac-rows">
        {types.map((t) => {
          const off = hidden.includes(t)
          const grey = hsrOnly && t !== 'high_speed'
          const sw = grey ? INACTIVE : TYPE_COLOR[t]
          return (
            <button key={t} type="button" className={`ac-row${off ? ' off' : ''}${grey ? ' grey' : ''}`} aria-pressed={!off} title={off ? `Show ${TYPE_LABEL[t]}` : `Hide ${TYPE_LABEL[t]}`} onClick={() => onToggleType(t)}>
              <i className="sw" style={{ background: off ? 'transparent' : sw, borderColor: sw }} />
              <span className="grow"><span className="nm">{TYPE_LABEL[t]}</span>{off && <span className="muted sm"> hidden</span>}</span>
              <span className="v">{num(open(t), units)}</span><span className="p muted">{pct(open(t) / total)}</span>
            </button>
          )
        })}
      </div>
      {hsrOnly && <div className="ac-grey muted"><i />HSR only is on: other types are greyed on the map</div>}
      {(con > 0 || freightKm > 0) && (
        <div className="ac-not">
          <div className="ac-sec"><span className="eyebrow">Not in the total</span><span className="muted sm">same scale, outlined</span></div>
          {con > 0 && (
            <button type="button" className={`ac-line${construction ? '' : ' off'}`} aria-pressed={construction} onClick={onToggleConstruction} title={construction ? 'Hide under construction' : 'Show under construction'}>
              <span className="txt"><span className="nm">Being built</span><span>{num(con, units)} {units}</span><span className="muted sm">{construction ? 'on the map' : 'hidden'}</span></span>
              <Outlined n={con} unit={unit} />
            </button>
          )}
          {freightKm > 0 && (
            <button type="button" className={`ac-line${freight ? '' : ' off'}`} aria-pressed={freight} onClick={onToggleFreight} title={freight ? 'Hide freight' : 'Show freight'}>
              <span className="txt"><span className="nm">Freight</span><span>{num(freightKm, units)} {units}</span><span className="muted sm">{freight ? 'on the map' : 'hidden'}</span></span>
              <Outlined n={freightKm} unit={unit} />
            </button>
          )}
        </div>
      )}
      {st.open != null && st.open > 0 && (
        <div className="ac-st">
          <div className="ac-total"><strong>{n0(st.open)} stations</strong><span className="ac-scale small"><b />= {n0(sUnit)} {sUnit === 1 ? 'station' : 'stations'}</span></div>
          <Dots unit={sUnit} parts={[{ color: TYPE_COLOR.metro, v: metro }, { color: '#B8A889', v: main }, { color: TYPE_COLOR.light_rail, v: tram }]} />
          <div className="ac-key muted">
            {metro > 0 && <span><b style={{ background: TYPE_COLOR.metro }} />{n0(metro)} metro</span>}
            {main > 0 && <span><b style={{ background: '#B8A889' }} />{n0(main)} main-line</span>}
            {tram > 0 && <span><b style={{ background: TYPE_COLOR.light_rail }} />{n0(tram)} light rail, tram</span>}
            {!!st.construction && <span>{n0(st.construction)} being built</span>}
          </div>
        </div>
      )}
      <div className="muted sm ac-foot">Route length: double track counts once (about 10% accuracy). {Object.values(a.lines).reduce((s, n) => s + n, 0)} lines.</div>
    </aside>
  )
}

const bandName = (i: number) => (i === 0 ? `${SPEED_BANDS[0].min}+ km/h` : SPEED_BANDS[i].min === 0 ? 'under 80' : `${SPEED_BANDS[i].min}–${SPEED_BANDS[i - 1].min - 1} km/h`)
/** swatch thickness per band: the rail type that typically runs at that speed */
const BAND_TYPE: RailType[] = ['high_speed', 'high_speed', 'high_speed', 'high_speed', 'intercity', 'regional', 'regional', 'metro', 'light_rail']

/** Track speed of the area under the view: km per band, each band a switch for the map. */
export function SpeedCard({ area: a, units, hiddenSpeed, hsrOnly, embedded, onToggle, onShowAll, onHideAll }: {
  area: StatsArea | null; units: Units; hiddenSpeed: number[]; hsrOnly: boolean; embedded?: boolean
  onToggle(m: number): void; onShowAll(): void; onHideAll(): void
}) {
  if (!a && !embedded) return null
  const km = (m: number) => a?.speed?.[m === SPEED_NONE ? 'none' : String(m)] ?? 0
  const all = [...SPEED_BANDS.map((b) => b.min), SPEED_NONE]
  const row = (m: number, i: number) => {
    const band = SPEED_BANDS[i]
    const off = hiddenSpeed.includes(m)
    const none = m === SPEED_NONE
    const hs = !none && m >= 250
    const w = none ? 1.6 : TYPE_WIDTH[BAND_TYPE[i]][1]
    const grey = hsrOnly && (none || m < 250)
    const v = km(m)
    return (
      <button key={m} type="button" className={`sp-row${off ? ' off' : ''}${hs ? ' hs' : ''}${grey ? ' grey' : ''}`} aria-pressed={!off} title={off ? `Show ${none ? 'speed not mapped' : bandName(i)}` : `Hide ${none ? 'speed not mapped' : bandName(i)}`} onClick={() => onToggle(m)}>
        <svg width="34" height="10" viewBox="0 0 44 12" aria-hidden="true">
          <line x1="3" y1="6" x2="41" y2="6" stroke={grey ? INACTIVE : none ? SPEED_UNKNOWN : band.color} strokeWidth={grey ? Math.min(w, 1.6) : w} strokeLinecap={none ? 'butt' : 'round'} strokeDasharray={none ? '4 3' : undefined} />
        </svg>
        <span className="grow"><span className="nm">{none ? 'Not mapped' : bandName(i)}</span>
          {m === 200 && <span className="sub">high-speed elsewhere, intercity here</span>}</span>
        <span className="v muted">{v > 0 ? `${num(v, units)} ${units}` : 'none'}</span>
      </button>
    )
  }
  const hidN = hiddenSpeed.length
  return (
    <aside className={`area-card sp${embedded ? ' embedded' : ''}`} aria-label={a ? `Track speed in ${a.name}` : 'Track speed'}>
      <div className="ac-sec top">
        <div><strong className="ac-title">Track speed</strong><div className="muted sm">{a ? `${a.name} · ` : ''}{hidN === 0 ? 'all bands on the map' : `${hidN} band${hidN > 1 ? 's' : ''} hidden`}</div></div>
        <Tools allOn={hidN === 0} allOff={hidN === all.length} onAll={onShowAll} onNone={onHideAll} />
      </div>
      <div className={`sp-hs${SPEED_BANDS.slice(0, 4).every((b) => hiddenSpeed.includes(b.min)) ? ' off' : ''}`}>
        <div className="sp-cap"><span className="eyebrow">High-speed on this map</span><i /></div>
        {SPEED_BANDS.slice(0, 4).map((b, i) => row(b.min, i))}
      </div>
      <div className="sp-line china"><b>250 · China line</b><i /></div>
      {row(200, 4)}
      <div className="sp-line uic"><b>200 · UIC line</b><i /></div>
      {SPEED_BANDS.slice(5).map((b, i) => <Fragment key={b.min}>{row(b.min, i + 5)}</Fragment>)}
      {row(SPEED_NONE, 0)}
      {hsrOnly && <div className="ac-grey muted"><i />Not high-speed (greyed)</div>}
      <div className="muted sm ac-foot">Colour from the maxspeed tag. High-speed here follows China's 250 km/h line. Passenger route in service, double track counted once.</div>
    </aside>
  )
}
