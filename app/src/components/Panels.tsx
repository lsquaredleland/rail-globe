import { useEffect, useState } from 'react'
import { logoUrl } from '../config'
import { loadLine, loadStation, type LineIndex, type LineRecord, type NamedLine, type StationRecord } from '../data'
import { km, tierLabel, UNKNOWN } from '../format'
import { TYPE_COLOR, TYPE_LABEL, TYPE_ORDER, TYPE_WIDTH, type RailType } from '../palette'
import { Icon, LineBadge, namePair, shortName, shortNameEn, Swatch, useLang } from './ui'

function useLoaded<T>(load: () => Promise<T>, key: string): { data: T | null; error: boolean } {
  const [s, setS] = useState<{ key: string; data: T | null; error: boolean }>({ key: '', data: null, error: false })
  useEffect(() => {
    let live = true
    load().then((d) => live && setS({ key, data: d, error: false })).catch(() => live && setS({ key, data: null, error: true }))
    return () => { live = false }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])
  return s.key === key ? { data: s.data, error: s.error } : { data: null, error: false }
}

export function PanelFrame({ onClose, onBack, backLabel = 'All lines', children, label }: { onClose(): void; onBack?: () => void; backLabel?: string; children: React.ReactNode; label: string }) {
  return (
    <aside className="panel" aria-label={label}>
      {onBack && <button type="button" className="back" onClick={onBack}>{Icon.back}{backLabel}</button>}
      <button type="button" className="close" aria-label="Close panel" onClick={onClose}>{Icon.close}</button>
      {children}
    </aside>
  )
}

const osmUrl = (id: string | null | undefined) => (id ? `https://www.openstreetmap.org/${id}` : null)

export function StationPanel({ slug, id, index, onClose, onLine, onStation, onHoverLine }: {
  slug: string; id: string; index: LineIndex | undefined; onClose(): void; onLine(id: number): void; onStation(id: string): void; onHoverLine(id: number | null): void
}) {
  const lang = useLang()
  const { data: s, error } = useLoaded<StationRecord>(() => loadStation(slug, id), `${slug}/${id}`)
  if (error) return <PanelFrame onClose={onClose} label="Station"><p className="muted">Could not load this station.</p></PanelFrame>
  if (!s) return <PanelFrame onClose={onClose} label="Station"><p className="muted">Loading…</p></PanelFrame>
  const [sMain, en] = namePair(lang, s.name, s.name_en)
  const link = osmUrl(s.osm)
  return (
    <PanelFrame onClose={onClose} label="Station">
      <div className="row gap8 title-row">
        {s.logo && <img key={s.logo} className="op-logo" src={logoUrl(s.logo)} alt="" loading="lazy" onError={(e) => { e.currentTarget.style.display = 'none' }} />}
        <h2 className="p-title">{sMain || 'Unnamed station'}</h2>
      </div>
      {en && <div className="p-sub">{en}</div>}
      <div className="row gap8 wrap">
        <span className={`pill tier${s.tier}`}>{s.construction ? 'Station under construction' : tierLabel(s.tier, s.hsr)}</span>
        {s.terminus && <span className="pill plain">Terminus</span>}
        {s.airport && <span className="pill plain">Airport · {s.airport}</span>}
      </div>
      <div className="facts">
        <div><span className="k">Platforms</span><span className={s.platform_count ? 'v' : 'v dim'}>{s.platform_count || UNKNOWN}</span></div>
        <div><span className="k">Lines mapped in OSM</span><span className={s.line_count ? 'v' : 'v dim'}>{s.line_count || 'none'}</span></div>
      </div>
      {s.line_count > 0 && (
        <section>
          <h3 className="eyebrow">Lines stopping here <span className="muted sm">(highlighted on the map)</span></h3>
          {TYPE_ORDER.filter((t) => s.lines[t]).map((t) => (
            <div key={t} className="grp">
              <div className="grp-head"><Swatch type={t} w={t === 'high_speed' ? 4 : 3} />{TYPE_LABEL[t]}<span className="count">{s.lines[t].length}</span></div>
              {s.lines[t].slice(0, 12).map((lid) => (
                <button key={lid} type="button" className="line-row" onClick={() => onLine(lid)}
                  onMouseEnter={() => onHoverLine(lid)} onMouseLeave={() => onHoverLine(null)} onFocus={() => onHoverLine(lid)} onBlur={() => onHoverLine(null)}>
                  <LineBadge e={index?.[String(lid)]} /><span className="grow col"><span>{shortName(index?.[String(lid)])}</span>{shortNameEn(index?.[String(lid)]) && <span className="muted sm">{shortNameEn(index?.[String(lid)])}</span>}</span>
                </button>
              ))}
              {s.lines[t].length > 12 && <div className="muted sm pad-l">+ {s.lines[t].length - 12} more</div>}
            </div>
          ))}
        </section>
      )}
      {(s.rail_lines?.length ?? 0) > 0 && (
        <section>
          <h3 className="eyebrow">Railway lines through here</h3>
          {s.rail_lines!.map((l) => {
            const [a, b] = namePair(lang, l.name, l.name_en)
            return <div key={l.id} className="line-row static"><span className="grow col"><span>{a}</span>{b && <span className="muted sm">{b}</span>}</span></div>
          })}
          <div className="muted sm mt">Named lines in OSM (infrastructure), not train services</div>
        </section>
      )}
      {(s.platform_refs.length > 0 || s.platform_refs_other.length > 0) && (
        <section>
          <h3 className="eyebrow">Platforms</h3>
          <div className="chips">{s.platform_refs.map((r) => <span key={r} className="chip">{r}</span>)}</div>
          {s.platform_refs_other.length > 0 && <div className="muted sm mt">Metro and tram platforms: {s.platform_refs_other.join(', ')}</div>}
          <div className="muted sm mt">{s.platform_count - s.platform_refs.length - s.platform_refs_other.length > 0 ? `${s.platform_count - s.platform_refs.length - s.platform_refs_other.length} more without a platform number in OSM` : ''}</div>
        </section>
      )}
      {s.complex && (
        <section>
          <h3 className="eyebrow">Part of</h3>
          <div className="p-strong">{s.complex.name} complex</div>
          <ul className="plain-list">
            <li>This station ({s.modes.join(', ').replace(/_/g, ' ')})</li>
            {s.complex.others.map((o) => <li key={o.id}><button type="button" className="link" onClick={() => onStation(o.id)}>{namePair(lang, o.name, o.name_en)[0] || o.id}</button> <span className="muted sm">{o.modes.join(', ').replace(/_/g, ' ')}</span></li>)}
          </ul>
        </section>
      )}
      {link && <a className="osm-link" href={link} target="_blank" rel="noopener noreferrer">View on OpenStreetMap ↗</a>}
    </PanelFrame>
  )
}

export function LinePanel({ slug, id, units, onClose, onBack, backLabel, onStation }: {
  slug: string; id: number; units: 'km' | 'mi'; onClose(): void; onBack?: () => void; backLabel?: string; onStation(id: string, lngLatHint?: undefined): void
}) {
  const lang = useLang()
  const { data: l, error } = useLoaded<LineRecord>(() => loadLine(slug, id), `${slug}/${id}`)
  const [ri, setRi] = useState(0)
  useEffect(() => setRi(0), [id])
  if (error) return <PanelFrame onClose={onClose} onBack={onBack} backLabel={backLabel} label="Line"><p className="muted">Could not load this line.</p></PanelFrame>
  if (!l) return <PanelFrame onClose={onClose} onBack={onBack} backLabel={backLabel} label="Line"><p className="muted">Loading…</p></PanelFrame>
  const route = l.routes[Math.min(ri, l.routes.length - 1)]
  const t = l.type as RailType
  const link = osmUrl(l.osm)
  const [lMain, lOther] = namePair(lang, shortName({ n: l.name, r: l.ref }), shortNameEn({ n: l.name, ne: l.name_en }))
  return (
    <PanelFrame onClose={onClose} onBack={onBack} backLabel={backLabel} label="Line">
      <div className="row gap8">
        <LineBadge e={{ n: l.name, r: l.ref, c: l.colour, t, o: l.operator, f: l.from, to: l.to, km: l.km }} size={26} />
        <h2 className="p-title">{lMain}</h2>
      </div>
      {lOther && <div className="p-en">{lOther}</div>}
      <div className="p-sub">{TYPE_LABEL[t]}{l.operator ? ` · ${l.operator}` : ''}</div>
      <div className="facts">
        <div><span className="k">Length</span><span className="v">{l.km ? km(l.km, units) : UNKNOWN}</span></div>
        <div><span className="k">Stations</span><span className="v">{route ? route.stations.length : 0}</span></div>
        <div><span className="k">Colour</span><span className="v row gap8"><i className="dot" style={{ background: l.colour ?? TYPE_COLOR[t] }} />{l.colour ?? 'not in OSM'}</span></div>
      </div>
      {l.routes.length > 1 && (
        <label className="sel-label">Variant ({l.routes.length})
          <select value={ri} onChange={(e) => setRi(Number(e.target.value))}>
            {l.routes.map((r, i) => <option key={r.id} value={i}>{(r.name ?? `${r.from ?? '?'} → ${r.to ?? '?'}`).slice(0, 60)}</option>)}
          </select>
        </label>
      )}
      <section>
        <h3 className="eyebrow">Stations in order</h3>
        {route && route.stations.length > 0 ? (
          <ol className="stops">
            {route.stations.map((sid, i) => {
              const nm = l.station_names[sid]
              return (
                <li key={sid + i}>
                  <i className="stop-dot" style={{ borderColor: l.colour ?? TYPE_COLOR[t] }} />
                  <button type="button" className="link" onClick={() => onStation(sid)}>{namePair(lang, nm?.n, nm?.ne)[0] || sid}</button>
                  {namePair(lang, nm?.n, nm?.ne)[1] && <span className="muted sm"> {namePair(lang, nm?.n, nm?.ne)[1]}</span>}
                </li>
              )
            })}
          </ol>
        ) : <p className="muted">No stops are mapped for this line in OSM.</p>}
      </section>
      {link && <a className="osm-link" href={link} target="_blank" rel="noopener noreferrer">View on OpenStreetMap ↗</a>}
    </PanelFrame>
  )
}

/** The services that run on a clicked piece of track: pick one to see its stations; all of them are highlighted on the map meanwhile */
export function ServicesPanel({ ids, index, title, titleEn, sub, units, onClose, onLine, onHoverLine }: {
  ids: number[]; index: LineIndex | undefined; title: string; titleEn: string | null; sub: string; units: 'km' | 'mi'; onClose(): void; onLine(id: number): void; onHoverLine(id: number | null): void
}) {
  const lang = useLang()
  const [main, other] = namePair(lang, title, titleEn)
  const rows = ids.map((id) => [id, index?.[String(id)]] as const)
    .sort((a, b) => TYPE_ORDER.indexOf(a[1]?.t ?? 'other') - TYPE_ORDER.indexOf(b[1]?.t ?? 'other') || (a[1]?.r ?? a[1]?.n ?? '').localeCompare(b[1]?.r ?? b[1]?.n ?? '', undefined, { numeric: true }))
  return (
    <PanelFrame onClose={onClose} label="Services">
      <h2 className="p-title">{main}</h2>
      {other && <div className="p-en">{other}</div>}
      <div className="p-sub">{sub}</div>
      <section>
        <h3 className="eyebrow">{ids.length} services run here <span className="muted sm">(all highlighted on the map)</span></h3>
        <div className="grp">{rows.map(([id, e]) => {
          const [a, b] = namePair(lang, shortName(e), shortNameEn(e))
          return (
            <button key={id} type="button" className="line-row" onClick={() => onLine(id)}
              onMouseEnter={() => onHoverLine(id)} onMouseLeave={() => onHoverLine(null)} onFocus={() => onHoverLine(id)} onBlur={() => onHoverLine(null)}>
              <LineBadge e={e} />
              <span className="grow col"><span>{a || `Line ${id}`}</span>{b && <span className="muted sm">{b}</span>}{e && (e.f || e.to) && <span className="muted sm ell">{e.f ?? '?'} ↔ {e.to ?? '?'}</span>}</span>
              <span className="muted sm">{e?.km ? km(e.km, units) : ''}</span>
            </button>
          )
        })}</div>
      </section>
    </PanelFrame>
  )
}

export function LinesList({ index, named, regionName, units, onClose, onLine, onNamed, activeName, onHoverLine, onHoverNamed }: {
  index: LineIndex | undefined; named: NamedLine[] | undefined; regionName: string; units: 'km' | 'mi'; onClose(): void
  onLine(id: number): void; onNamed(n: NamedLine): void; activeName: string | null; onHoverLine(id: number | null): void; onHoverNamed(name: string | null): void
}) {
  const lang = useLang()
  const [filter, setFilter] = useState<RailType | 'all'>('all')
  const entries = Object.entries(index ?? {})
  const nl = named ?? []
  const present = TYPE_ORDER  // every rail type is listed, with 0 when the region has none
  const count = (t: RailType) => entries.filter(([, e]) => e.t === t).length + nl.filter((n) => n.t === t).length
  return (
    <PanelFrame onClose={onClose} label="Lines">
      <h2 className="p-title">Lines in {regionName}</h2>
      <div className="p-sub">{entries.length} lines with services mapped in OSM{nl.length ? `, ${nl.length} named railway lines` : ''}</div>
      <div className="chips tabs">
        <button type="button" className={`chip btn-chip${filter === 'all' ? ' on' : ''}`} onClick={() => setFilter('all')}>All {entries.length + nl.length}</button>
        {present.map((t) => (
          <button key={t} type="button" className={`chip btn-chip${filter === t ? ' on' : ''}`} onClick={() => setFilter(t)}>
            <i className="dot" style={{ background: TYPE_COLOR[t] }} />{TYPE_LABEL[t]} {count(t)}
          </button>
        ))}
      </div>
      {present.filter((t) => filter === 'all' || filter === t).map((t) => {
        const named = nl.filter((n) => n.t === t)
        return (
          <div key={t} className="grp">
            <div className="grp-head"><Swatch type={t} w={TYPE_WIDTH[t][1]} />{TYPE_LABEL[t]} <span className="muted sm">{count(t)}</span></div>
            {entries.filter(([, e]) => e.t === t).sort((a, b) => (a[1].r ?? a[1].n ?? '').localeCompare(b[1].r ?? b[1].n ?? '', undefined, { numeric: true })).map(([id, e]) => (
              <button key={id} type="button" className="line-row" onClick={() => onLine(Number(id))}
                onMouseEnter={() => onHoverLine(Number(id))} onMouseLeave={() => onHoverLine(null)} onFocus={() => onHoverLine(Number(id))} onBlur={() => onHoverLine(null)}>
                <LineBadge e={e} />
                <span className="grow col"><span>{namePair(lang, shortName(e), shortNameEn(e))[0]}</span>{namePair(lang, shortName(e), shortNameEn(e))[1] && <span className="muted sm">{namePair(lang, shortName(e), shortNameEn(e))[1]}</span>}{(e.f || e.to) && <span className="muted sm ell">{e.f ?? '?'} ↔ {e.to ?? '?'}</span>}</span>
                <span className="muted sm">{e.km ? km(e.km, units) : ''}</span>
              </button>
            ))}
            {named.map((n) => (
              <button key={n.n + n.t} type="button" className={`line-row${activeName === n.n ? ' active' : ''}`} onClick={() => onNamed(n)}
                onMouseEnter={() => onHoverNamed(n.n)} onMouseLeave={() => onHoverNamed(null)} onFocus={() => onHoverNamed(n.n)} onBlur={() => onHoverNamed(null)}>
                <span className="badge outline" style={{ minWidth: 22, height: 22, borderRadius: 4, color: TYPE_COLOR[t], borderColor: TYPE_COLOR[t] }}>–</span>
                <span className="grow col"><span>{namePair(lang, n.n, n.ne)[0]}</span>{namePair(lang, n.n, n.ne)[1] && <span className="muted sm">{namePair(lang, n.n, n.ne)[1]}</span>}<span className="muted sm">Named in OSM, no services mapped</span></span>
                <span className="muted sm">~{km(n.km, units)}</span>
              </button>
            ))}
          </div>
        )
      })}
      {nl.length > 0 && <p className="named-note">Named railway lines come from track names in OSM, for places where train services are not mapped as routes. Lengths are approximate.</p>}
    </PanelFrame>
  )
}
