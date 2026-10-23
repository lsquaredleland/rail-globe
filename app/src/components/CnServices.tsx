import { useEffect, useState } from 'react'
import { canExpand, cityRows, clock, cnFocus, destLookup, hm, partEnglish, snapshotText, type CnFocus } from '../cnServices'
import { loadCnIds, loadCnRecord, type CnCity, type CnRecord, type CnSnapshot } from '../data'
import { ignoreError } from '../dev'
import { namePair, useLang } from './ui'

function CityRow({ c }: { c: CnCity }) {
  const lang = useLang()
  const [open, setOpen] = useState(false)
  const [main, other] = namePair(lang, c[0], c[1])
  const can = canExpand(c)
  const body = (
    <>
      <span className="col">
        <span>
          {main}
          {can && (
            <span className="muted sm" aria-hidden="true">
              {open ? ' ▾' : ' ▸'}
            </span>
          )}
        </span>
        {other && <span className="muted sm">{other}</span>}
      </span>
      <span className="muted sm">
        {c[2]}/day · {hm(c[3])}
      </span>
    </>
  )
  return (
    <div>
      {can ? (
        <button type="button" className="tt-row cn-city" aria-expanded={open} onClick={() => setOpen(!open)}>
          {body}
        </button>
      ) : (
        <div className="tt-row cn-city">{body}</div>
      )}
      {open &&
        c[4].map((p) => {
          const [a, b] = namePair(lang, p[0], p[1])
          return (
            <div key={p[0]} className="tt-row cn-part">
              <span className="col">
                <span>{a}</span>
                {b && <span className="muted sm">{b}</span>}
              </span>
              <span className="muted sm">
                {p[2]}/day · {hm(p[3])}
              </span>
            </div>
          )
        })}
    </div>
  )
}

function Bars({ v, label }: { v: number[]; label: string }) {
  const max = Math.max(1, ...v)
  return (
    <figure className="cn-bars">
      <svg viewBox="0 0 250 52" width="100%" role="img" aria-label={`${label} by hour, 0 to 23`}>
        {v.map((n, i) => (
          <rect key={i} x={2 + i * 10} y={36 - (n / max) * 32} width={7} height={Math.max(n ? 1 : 0, (n / max) * 32)} rx={1} fill="var(--ink2)">
            <title>{`${i}:00 ${n}`}</title>
          </rect>
        ))}
        <line x1={0} x2={250} y1={36.5} y2={36.5} stroke="var(--line2)" />
        {[0, 6, 12, 18, 23].map((i) => (
          <text key={i} x={2 + i * 10 + 3.5} y={47} fontSize={8} textAnchor="middle" fill="var(--ink3)">
            {i}
          </text>
        ))}
        <text x={248} y={8} fontSize={8} textAnchor="end" fill="var(--ink3)">{`max ${max}`}</text>
      </svg>
      <figcaption className="muted sm">{label}</figcaption>
    </figure>
  )
}

/** The station panel's section for a China station with trains: its record from the 12306 timetable (departures and arrivals, hourly bars, destination
 *  cities). `onFocus` gets what the map shows (its trains' stations and stretches) while the section is open, and null when it closes. */
export function CnServices({ id, onFocus }: { id: string; onFocus(cn: CnFocus | null): void }) {
  const lang = useLang()
  const [rec, setRec] = useState<{ id: string; rec: CnRecord; snapshot: CnSnapshot } | null>(null)
  const [all, setAll] = useState(false)
  useEffect(() => {
    let live = true
    setAll(false)
    loadCnRecord(id)
      .then(async (r) => {
        const focus = r && cnFocus(r.key, r.rec, await loadCnIds().catch(() => ({})))
        if (!live) return
        setRec(r && { id, rec: r.rec, snapshot: r.snapshot })
        if (focus) onFocus(focus)
      })
      .catch(ignoreError(`china services ${id}`)) // no records in this build: the panel stays as it was
    return () => {
      live = false
      onFocus(null)
    }
  }, [id, onFocus])
  if (rec?.id !== id) return null
  const r = rec.rec
  const { rows, more } = cityRows(r.dest, all)
  return (
    <section>
      <h3 className="eyebrow">Trains (12306, one day)</h3>
      <div className="facts">
        <div>
          <span className="k">Departures/day</span>
          <span className="v">{r.dep}</span>
        </div>
        <div>
          <span className="k">Arrivals/day</span>
          <span className="v">{r.arr}</span>
        </div>
        <div>
          <span className="k">First departure</span>
          <span className="v">{r.first != null ? clock(r.first) : '–'}</span>
        </div>
        <div>
          <span className="k">Last departure</span>
          <span className="v">{r.last != null ? clock(r.last) : '–'}</span>
        </div>
      </div>
      <Bars v={r.hd} label="Departures by hour" />
      <Bars v={r.ha} label="Arrivals by hour" />
      <div className="muted sm mt">{r.stops} trains stop here</div>
      <h3 className="eyebrow mt">Top destination cities</h3>
      <div className="grp">
        {rows.map((c) => (
          <CityRow key={`${lang}${c[0]}`} c={c} />
        ))}
        {more > 0 && (
          <button type="button" className="link pad-l" onClick={() => setAll(!all)}>
            {all ? 'Show fewer' : `+${more} more cities`}
          </button>
        )}
      </div>
      <div className="muted sm mt">{snapshotText(rec.snapshot)}</div>
    </section>
  )
}

/** The tooltip of a station the focused station's trains stop at: both directions, in the app's tooltip style. */
export function CnDestTip({ cn, hit, style }: { cn: CnFocus; hit: { id: string; cx: string; n: string; ne: string }; style: React.CSSProperties }) {
  const lang = useLang()
  const found = destLookup(cn.rec, cn.ids, hit.id, hit.cx)
  if (!found) return null
  const { d } = found
  const [a] = namePair(lang, cn.rec.name, cn.rec.name_en)
  const [b, b2] = namePair(lang, hit.n, partEnglish(cn.rec, hit.n) ?? hit.ne)
  const dir = (from: string, to: string, n: number, fast: number, first: number, last: number) => (
    <div key={from}>
      <div className="tt-row">
        <span className="k">{`${from} → ${to}`}</span>
        <span className="v">{n ? `${n} trains/day` : 'none'}</span>
      </div>
      {n > 0 && (
        <div className="tt-row">
          <span className="k">{`fastest ${hm(fast)}`}</span>
          <span className="v">{`first ${clock(first)}, last ${clock(last)}`}</span>
        </div>
      )}
    </div>
  )
  return (
    <div className="tooltip" role="tooltip" style={style}>
      <div className="tt-names">
        <span className="tt-title">{`${a} ↔ ${b}`}</span>
        {b2 && <span className="tt-sub">{b2}</span>}
      </div>
      {dir(a, b, d[0], d[1], d[2], d[3])}
      {dir(b, a, d[4], d[5], d[6], d[7])}
    </div>
  )
}
