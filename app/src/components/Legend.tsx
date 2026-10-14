import { TYPE_COLOR, TYPE_LABEL, TYPE_ORDER, TYPE_WIDTH, type RailType } from '../palette'
import type { LineIndex } from '../data'
import { INACTIVE, type ColorMode } from '../mapStyle'
import { Icon, LineBadge, PlaneIcon } from './ui'

function Pill({ color, w, label, off, grey, onClick }: { color: string; w: number; label: string; off: boolean; grey?: boolean; onClick: () => void }) {
  return (
    <button type="button" className={`pill${off ? ' off' : ''}${grey ? ' grey' : ''}`} aria-pressed={!off} title={off ? `Show ${label}` : `Hide ${label}`} onClick={onClick}>
      <svg width="22" height="10" viewBox="0 0 22 10" aria-hidden="true"><line x1="2" y1="5" x2="20" y2="5" stroke={grey ? INACTIVE : color} strokeWidth={grey ? Math.min(w, 1.6) : w} strokeLinecap="round" /></svg>
      <span>{label}</span>
    </button>
  )
}

function Row({ color, w, dash, label, note, dim, off, grey, onClick }: {
  color: string; w: number; dash?: boolean; label: string; note?: string; dim?: boolean; off?: boolean; grey?: boolean; onClick?: () => void
}) {
  const inner = (
    <>
      <svg width="30" height="10" viewBox="0 0 30 10" aria-hidden="true">
        <line x1="2" y1="5" x2="28" y2="5" stroke={grey ? INACTIVE : color} strokeWidth={grey ? Math.min(w, 1.6) : w} strokeLinecap={dash ? 'butt' : 'round'} strokeDasharray={dash ? `${w * 2} ${w * 1.5}` : undefined} />
      </svg>
      <span className="grow" style={{ textDecoration: off ? 'line-through' : undefined }}>{label}</span>{note && <span className="muted sm">{note}</span>}
    </>
  )
  const style = { opacity: dim || off ? 0.45 : grey ? 0.7 : 1 }
  return onClick
    ? <button type="button" className="lg-row lg-btn" aria-pressed={!off} title={off ? `Show ${label}` : `Hide ${label}`} style={style} onClick={onClick}>{inner}</button>
    : <div className="lg-row" style={style}>{inner}</div>
}

export function Legend({ mode, hsrOnly, open, setOpen, freight, index, compact, hidden, onToggleType, onShowAll, onHideAll, onToggleFreight }: {
  mode: ColorMode; hsrOnly: boolean; open: boolean; setOpen(v: boolean): void; freight: boolean; index: LineIndex | undefined; compact?: boolean
  hidden: string[]; onToggleType(t: string): void; onShowAll(): void; onHideAll(): void; onToggleFreight(): void
}) {
  const op = mode === 'operator'
  const allCount = (t: RailType) => Object.values(index ?? {}).filter((e) => e.t === t && e.c).length
  const groups: [RailType, [string, LineIndex[string]][]][] = TYPE_ORDER.map((t) => [
    t, Object.entries(index ?? {}).filter(([, e]) => e.t === t && e.c).sort((a, b) => (a[1].r ?? '').localeCompare(b[1].r ?? '', undefined, { numeric: true })).slice(0, 12)])
  return (
    <div className={`legend${compact ? ' compact' : ''}`}>
      <button type="button" className="lg-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <span className="row gap8">{op ? 'Lines in region' : 'Rail types'}
          {!open && !op && (
            <svg width="44" height="10" viewBox="0 0 44 10" aria-hidden="true">
              <line x1="2" y1="5" x2="10" y2="5" stroke={TYPE_COLOR.high_speed} strokeWidth={TYPE_WIDTH.high_speed[1]} strokeLinecap="round" /><line x1="14" y1="5" x2="20" y2="5" stroke={TYPE_COLOR.regional} strokeWidth={TYPE_WIDTH.regional[1]} strokeLinecap="round" />
              <line x1="24" y1="5" x2="30" y2="5" stroke={TYPE_COLOR.metro} strokeWidth={TYPE_WIDTH.metro[1]} strokeLinecap="round" /><line x1="34" y1="5" x2="42" y2="5" stroke={TYPE_COLOR.light_rail} strokeWidth={TYPE_WIDTH.light_rail[1]} strokeLinecap="round" />
            </svg>)}
        </span>
        {Icon.chevron(open)}
      </button>
      {open && !op && (
        <div className="lg-body">
          <div className="pill-head">
            <span className="muted sm">{hsrOnly ? 'HSR only is on: other types are greyed' : 'Click a type to hide or show it'}</span>
            <span className="pill-actions">
              <button type="button" className="pill sm" disabled={hidden.length === 0} onClick={onShowAll}>All</button>
              <button type="button" className="pill sm" disabled={TYPE_ORDER.every((t) => hidden.includes(t))} onClick={onHideAll}>None</button>
            </span>
          </div>
          <div className="pills">
            {TYPE_ORDER.map((t) => <Pill key={t} color={TYPE_COLOR[t]} w={TYPE_WIDTH[t][1]} label={TYPE_LABEL[t]} off={hidden.includes(t)} grey={hsrOnly && t !== 'high_speed'} onClick={() => onToggleType(t)} />)}
            <Pill color={TYPE_COLOR.freight} w={TYPE_WIDTH.freight[1]} label="Freight" off={!freight} grey={hsrOnly} onClick={onToggleFreight} />
          </div>
          {hsrOnly && <Row color={INACTIVE} w={1.6} label="Not high-speed (greyed)" />}
          <Row color={TYPE_COLOR.yard} w={TYPE_WIDTH.yard[1]} label="Yard, siding" note="z13+" dim />
          <div className="lg-row"><span style={{ width: 30, display: 'flex', justifyContent: 'center' }}><PlaneIcon /></span><span className="grow">Airport</span></div>
        </div>
      )}
      {open && op && (
        <div className="lg-body">
          <div className="muted sm">Colours set by each operator{hsrOnly ? '; lines that are not high-speed are greyed' : ''}</div>
          {groups.map(([t, l]) => (
            <div key={t} className="lg-group">
              <button type="button" className="eyebrow eb-btn" aria-pressed={!hidden.includes(t)} title={hidden.includes(t) ? `Show ${TYPE_LABEL[t]}` : `Hide ${TYPE_LABEL[t]}`}
                style={{ opacity: hidden.includes(t) ? 0.45 : hsrOnly && t !== 'high_speed' ? 0.5 : 1, textDecoration: hidden.includes(t) ? 'line-through' : undefined }} onClick={() => onToggleType(t)}>{TYPE_LABEL[t]} {allCount(t)}</button>
              <div className="chips" style={{ opacity: hidden.includes(t) ? 0.35 : hsrOnly && t !== 'high_speed' ? 0.4 : 1 }}>{l.map(([id, e]) => <LineBadge key={id} e={e} />)}</div>
            </div>
          ))}
          <div className="lg-note"><svg width="30" height="10" aria-hidden="true"><line x1="2" y1="5" x2="28" y2="5" stroke={TYPE_COLOR.high_speed} strokeOpacity="0.6" strokeWidth={TYPE_WIDTH.high_speed[1]} strokeLinecap="round" /></svg>
            <span>No colour in OSM: drawn fainter, in its rail-type colour</span></div>
        </div>
      )}
    </div>
  )
}
