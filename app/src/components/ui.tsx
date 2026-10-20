import { createContext, useContext, type ReactNode } from 'react'
import { TYPE_COLOR, textOn, type RailType } from '../palette'
import type { LineIndexEntry } from '../data'

export const Icon = {
  back: (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M15 5l-7 7 7 7" />
    </svg>
  ),
  chevron: (open = false) => (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      aria-hidden="true"
      style={{ transform: open ? 'rotate(180deg)' : undefined }}
    >
      <path d="m6 9 6 6 6-6" />
    </svg>
  ),
  list: (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
      <path d="M9 6h12M9 12h12M9 18h12M4 6h.01M4 12h.01M4 18h.01" />
    </svg>
  ),
  dots: (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <circle cx="5" cy="12" r="1.8" />
      <circle cx="12" cy="12" r="1.8" />
      <circle cx="19" cy="12" r="1.8" />
    </svg>
  ),
  map: (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M9 4 3 6v14l6-2 6 2 6-2V4l-6 2-6-2zM9 4v14M15 6v14" />
    </svg>
  ),
  close: (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
      <path d="M6 6l12 12M18 6 6 18" />
    </svg>
  ),
  check: (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.4"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="m5 12 5 5 9-10" />
    </svg>
  ),
  link: (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M10 14a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1 1M14 10a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l1-1" />
    </svg>
  ),
  brand: (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
      <path d="M8 3 6 21M16 3l2 18M7 8h10M6.5 13h11M6 18h12" />
    </svg>
  ),
  sliders: (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
      <path d="M4 7h10M18 7h2M4 17h2M10 17h10M14 4v6M6 14v6" />
    </svg>
  ),
  plus: (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
      <path d="M12 5v14M5 12h14" />
    </svg>
  ),
  minus: (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
      <path d="M5 12h14" />
    </svg>
  ),
}

export function Toggle({ on, onChange, children }: { on: boolean; onChange(v: boolean): void; children: ReactNode }) {
  return (
    <button type="button" role="switch" aria-checked={on} className="tgl" onClick={() => onChange(!on)}>
      <span className={`tgl-track${on ? ' on' : ''}`}>
        <span className="tgl-knob" />
      </span>
      {children}
    </button>
  )
}

export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string
  value: T
  options: { value: T; label: string; disabled?: boolean; title?: string }[]
  onChange(v: T): void
}) {
  return (
    <div role="group" aria-label={label} className="seg">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          aria-pressed={o.value === value}
          disabled={o.disabled}
          title={o.title}
          className={o.value === value ? 'on' : ''}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  )
}

export function shortName(e: Pick<LineIndexEntry, 'n' | 'r'> | null | undefined): string {
  if (!e) return 'Line'
  const n = (e.n ?? '').split(/\s*[:：]\s*/)[0].trim()
  return n || e.r || 'Line'
}

/** Coloured line badge: the operator colour with the ref inside. Lines without a colour get an outlined badge in their type colour. */
export function LineBadge({ e, size = 22 }: { e: LineIndexEntry | undefined; size?: number }) {
  if (!e) return null
  const label = (e.r && e.r.length <= 5 ? e.r : (shortName(e).match(/\b([A-Z]?\d+[a-z]?|[A-Z])\b/)?.[0] ?? e.r ?? '·')).slice(0, 5)
  const round = e.t === 'metro' || e.t === 'light_rail'
  const base: React.CSSProperties = { minWidth: size, height: size, borderRadius: round ? size / 2 : 4 }
  if (e.c)
    return (
      <span className="badge" style={{ ...base, background: e.c, color: textOn(e.c) }}>
        {label}
      </span>
    )
  const tc = TYPE_COLOR[e.t as RailType] ?? TYPE_COLOR.other
  return (
    <span className="badge outline" style={{ ...base, color: tc, borderColor: tc }}>
      {label}
    </span>
  )
}

export function Swatch({ type, w = 3 }: { type: RailType; w?: number }) {
  return (
    <svg width="26" height="10" viewBox="0 0 26 10" aria-hidden="true">
      <line x1="2" y1="5" x2="24" y2="5" stroke={TYPE_COLOR[type]} strokeWidth={w} strokeLinecap="round" />
    </svg>
  )
}

export type Lang = 'local' | 'en'
export const LangContext = createContext<Lang>('local')
export const useLang = () => useContext(LangContext)
/** Primary and secondary name for the chosen language: the other name shows underneath only when it differs. */
export function namePair(lang: Lang, local: string | null | undefined, en: string | null | undefined): [string, string | null] {
  const l = local || '',
    e = en || ''
  const [a, b] = lang === 'en' ? [e || l, l] : [l || e, e]
  return [a, b && b !== a ? b : null]
}

/** English short name, only when OSM has one that differs from the local name. */
export function shortNameEn(e: { n?: string | null; ne?: string | null } | null | undefined): string | null {
  if (!e?.ne) return null
  const en = e.ne.split(/\s*[:：]\s*/)[0].trim()
  const local = (e.n ?? '').split(/\s*[:：]\s*/)[0].trim()
  return en && en !== local ? en : null
}

export function PlaneIcon({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 26 26" aria-hidden="true">
      <circle cx="13" cy="13" r="11.5" fill="#fff" stroke="#8E8A82" strokeWidth="1.4" />
      <path
        transform="translate(4.6 4.6) scale(0.7)"
        d="M21 16v-2l-8-5V3.5c0-.83-.67-1.5-1.5-1.5S10 2.67 10 3.5V9l-8 5v2l8-2.5V19l-2 1.5V22l3.5-1 3.5 1v-1.5L13 19v-5.5l8 2.5z"
        fill="#45423C"
      />
    </svg>
  )
}
