import type { Hit } from '../MapView'
import { railwayOf, type LineIndex, type Railways } from '../data'
import { speedBand } from '../mapStyle'
import { AREA_LABEL, areaSize, electrification, gauge, km, lineBreakdown, maxSpeed, railTypeLabel, SERVICE_LABEL, tierLabel, UNKNOWN } from '../format'
import { TYPE_LABEL } from '../palette'
import { LineBadge, namePair, shortName, shortNameEn, useLang } from './ui'

/** zoomed-out tiles keep a speed band, not the tag: "300-349 km/h", "400+ km/h", "under 80 km/h" */
const bandSpeed = (kmh: number) => { const b = speedBand(kmh); return b.min === 0 ? 'under 80 km/h' : `${b.label}${b.label.includes('km/h') ? '' : ' km/h'}` }

const Row = ({ k, v }: { k: string; v: string }) => (
  <div className="tt-row"><span className="k">{k}</span><span className={v === UNKNOWN ? 'v dim' : 'v'}>{v}</span></div>
)

function Names({ local, en }: { local: string; en: string }) {
  const [main, other] = namePair(useLang(), local, en)
  return (
    <div className="tt-names">
      <span className="tt-title" lang="und">{main || 'Unnamed station'}</span>
      {other && <span className="tt-sub">{other}</span>}
    </div>
  )
}

export function TooltipBody({ hit, index, railways, touch, onMore, units = 'km' }: { hit: Hit; index: LineIndex | undefined; railways?: Railways; touch: boolean; onMore?: () => void; units?: 'km' | 'mi' }) {
  const p = hit.props
  const lang = useLang()
  if (hit.kind === 'station') {
    const pc = Number(p.pc)
    const lc = Number(p.lc)
    return (
      <>
        <Names local={String(p.n ?? '')} en={String(p.ne ?? '')} />
        <div className={`pill tier${p.tier}`}>{Number(p.con) === 1 ? 'Station under construction' : tierLabel(Number(p.tier), Number(p.hsr) === 1)}</div>
        {p.ap && <Row k="Airport" v={String(p.ap)} />}
        <Row k="Platforms" v={pc > 0 ? String(pc) : UNKNOWN} />
        <div className="tt-row"><span className="k">Lines in OSM</span>
          <span className="v">{lc > 0 ? <><strong>{lc}</strong> <span className="sm muted">{lineBreakdown(String(p.lct ?? ''))}</span></> : <span className="dim">none mapped in OSM</span>}</span></div>
        {touch ? <button type="button" className="more" onClick={onMore}>More</button> : <div className="tt-foot">Click for platforms, lines and complex</div>}
      </>
    )
  }
  if (hit.kind === 'line') {
    const e = index?.[String(p.lid)]
    const [lnMain, lnOther] = namePair(lang, shortName(e), shortNameEn(e))
    return (
      <>
        <div className="row gap8"><LineBadge e={e} /><div className="tt-names"><span className="tt-title">{lnMain}</span>{lnOther && <span className="tt-sub">{lnOther}</span>}</div></div>
        <div className="tt-sub2">{e ? `${TYPE_LABEL[e.t] ?? e.t}${e.o ? ` · ${e.o}` : ''}` : UNKNOWN}</div>
        {e && (e.f || e.to) && <div className="ends"><span>{e.f ?? UNKNOWN}</span><span className="arrow">↔</span><span>{e.to ?? UNKNOWN}</span></div>}
        {e && !e.c && <div className="tt-foot">No colour in OSM: drawn in its rail-type colour</div>}
        {touch ? <button type="button" className="more" onClick={onMore}>More</button> : <div className="tt-foot">Click for stations in order</div>}
      </>
    )
  }
  if (hit.kind === 'area') {
    const label = AREA_LABEL[String(p.k)] ?? 'Railway area'
    const ha = Number(p.ha)
    return (
      <>
        <div className="tt-title lg">{p.n ? String(p.n) : label}</div>
        {p.n && <div className="tt-sub2">{label}</div>}
        {p.iata && <Row k="IATA" v={String(p.iata)} />}
        {p.k === 'platform' ? <Row k="Platform" v={p.ref ? String(p.ref) : UNKNOWN} /> : <Row k="Area" v={Number.isFinite(ha) && ha > 0 ? areaSize(ha, units) : UNKNOWN} />}
        {p.op && <Row k="Operator" v={String(p.op)} />}
      </>
    )
  }
  const ids = String(p.ln ?? '').split(',').filter(Boolean)
  const names = ids.map((id) => shortName(index?.[id])).filter(Boolean)
  // zoomed out, track carries its railway's number (nk) instead of its name
  const railway = railwayOf(railways, p)
  const trackName = p.n ? String(p.n) : railway?.[0] ?? ''
  const trackNameEn = p.ne ? String(p.ne) : railway?.[1] ?? null
  const isYard = p.type === 'yard'
  // the line's name leads; with no mapped line the track's own name (e.g. 成渝铁路) does; the rail type is the fallback
  const title = isYard ? (trackName || 'Yard track') : (names[0] ?? (trackName || railTypeLabel(String(p.type))))
  const en = isYard || !names.length ? trackNameEn : shortNameEn(index?.[ids[0]])
  const line = names.length ? index?.[ids[0]] : undefined
  const [tMain, tOther] = namePair(lang, title, en)
  const rest = names.slice(1)
  const sub = isYard
    ? `Yard and service track${SERVICE_LABEL[String(p.sv)] ? ` · ${SERVICE_LABEL[String(p.sv)]}` : ''}`
    : `${railTypeLabel(String(p.type), p.usage as string | undefined)}${p.prop ? ' · proposed' : p.con ? ' · under construction' : ''}`
  return (
    <>
      <div className="tt-names"><div className="tt-title lg">{tMain}</div>{tOther && <div className="tt-sub">{tOther}</div>}</div>
      <div className="tt-sub2">{sub}</div>
      {rest.length > 0 && <div className="tt-row"><span className="k">Also</span><span className="v">{rest.slice(0, 3).join(', ') + (rest.length > 3 ? ` +${rest.length - 3}` : '')}</span></div>}
      {line?.o && <Row k="Operator" v={line.o} />}
      {line?.f && line?.to && <Row k="Route" v={`${line.f} → ${line.to}`} />}
      {line && line.km > 0 && <Row k="Length" v={km(line.km, units)} />}
      {!line && !isYard && railway && railway[3] > 0 && <Row k="Railway length" v={km(railway[3], units)} />}
      {!isYard && names.length === 0 && <div className="tt-row"><span className="k">Lines</span><span className="v dim">none mapped in OSM</span></div>}
      {names.length > 0 && trackName && trackName !== title && <div className="tt-row"><span className="k">Track name</span><span className="v">{trackName}</span></div>}
      {/* zoomed out, track comes from the merged overview, which has no per-piece tags */}
      {p.id != null && <Row k="Electrified" v={electrification(p.el as string, p.v as string, p.fq as string)} />}
      {/* the exact tag from z8; zoomed out only the speed band is in the tiles */}
      {p.id != null && !isYard && <Row k="Max speed" v={maxSpeed(p.ms as string)} />}
      {p.id == null && !isYard && p.kmh != null && <Row k="Max speed" v={bandSpeed(Number(p.kmh))} />}
      {p.id != null && <Row k="Gauge" v={gauge(p.g as string)} />}
    </>
  )
}
