import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { Region } from '../regions'
import { searchItems, type SearchItem, type SearchStatus } from '../search'
import type { ColorMode } from '../mapStyle'
import type { StatsArea } from '../data'
import { AREA_KIND } from './AreaCard'
import { Icon, Segmented, Toggle, type Lang } from './ui'
import type { Theme } from '../palette'

export interface Controls {
  /** the area the card shows (what the view is over); the picker names it, falling back to the default region */
  area: StatsArea | null; inView: SearchItem | null; items: SearchItem[]; featured: SearchItem[]; placesStatus: SearchStatus; retryPlaces(): void; pickPlace(it: SearchItem): void; region: Region; fallback: Region; mode: ColorMode; hsrOnly: boolean; setHsrOnly(v: boolean): void; construction: boolean; proposed: boolean; outline: boolean; freight: boolean; units: 'km' | 'mi'; lang: Lang; setLang(l: Lang): void; theme: Theme; setTheme(t: Theme): void; inspect: boolean
  setRegion(slug: string): void; setMode(m: ColorMode): void; setConstruction(v: boolean): void; setProposed(v: boolean): void; setOutline(v: boolean): void; setFreight(v: boolean): void
  setUnits(u: 'km' | 'mi'): void; setInspect(v: boolean): void; openLines(): void; copyLink(): void; copied: boolean
}

function useOutside(open: boolean, close: () => void) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!open) return
    const h = (e: PointerEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) close() }
    const k = (e: KeyboardEvent) => { if (e.key === 'Escape') close() }
    document.addEventListener('pointerdown', h)
    document.addEventListener('keydown', k)
    return () => { document.removeEventListener('pointerdown', h); document.removeEventListener('keydown', k) }
  }, [open, close])
  return ref
}

const ITEM_KIND = { country: 'Country', province: 'Province / state', city: 'City' }

/** The region button names what the map is over (country, then province or city as you zoom in); opening it is a search over every
 *  country, province / state and metro city with rail. */
export function RegionPicker({ c, block }: { c: Controls; block?: boolean }) {
  const [open, setOpen] = useState(false)
  const [q, setQ] = useState('')
  const [at, setAt] = useState(0)
  const button = useRef<HTMLButtonElement>(null)
  const close = useCallback(() => { setOpen(false); setQ('') }, [])
  const ref = useOutside(open, close)
  const hits = useMemo(() => (q.trim() ? searchItems(c.items, q, 14) : c.featured), [q, c.items, c.featured])
  useEffect(() => setAt(0), [hits])
  const kbd = useRef(false)   // scroll the list only for arrow keys: a hover must not move it under the pointer
  useEffect(() => { if (kbd.current) document.getElementById(`place-${at}`)?.scrollIntoView({ block: 'nearest' }) }, [at, hits])
  const pick = (it: SearchItem) => { c.pickPlace(it); close(); button.current?.focus() }
  const closeToButton = () => { close(); button.current?.focus() }
  const status = c.placesStatus === 'failed' ? 'Places could not load' : c.placesStatus === 'loading' ? 'Loading places…' : q.trim() && !hits.length ? 'No place with rail matches' : ''
  const heading = q.trim() ? (hits.length ? 'Results' : '') : 'Biggest rail networks'
  // a city without an outline (no statistics of its own) is more exact than the province round it
  const city = c.inView?.kind === 'city' && !c.inView.bbox && c.area?.kind !== 'city' ? c.inView : null
  const name = city ? (city.nameEn ?? city.name) : c.area?.name ?? c.inView?.nameEn ?? c.inView?.name ?? c.fallback.name
  const kind = city ? ITEM_KIND.city : c.area ? AREA_KIND[c.area.kind] : c.inView ? ITEM_KIND[c.inView.kind]   : c.fallback.sub
  return (
    <div className={`pop-wrap${block ? ' block' : ''}`} ref={ref} onKeyDown={(e) => { if (open && e.key === 'Escape') { e.stopPropagation(); closeToButton() } }}
      onBlur={(e) => { if (open && e.relatedTarget && !e.currentTarget.contains(e.relatedTarget as Node)) close() }}>
      <button ref={button} type="button" className="btn region" aria-haspopup="dialog" aria-expanded={open}
        onClick={() => { if (open) close(); else { setOpen(true); if (c.placesStatus === 'failed') c.retryPlaces() } }}>
        {Icon.map}<strong>{name}</strong><span className="muted">{kind}</span>{Icon.chevron(open)}
      </button>
      <div role="status" aria-live="polite" className="sr-only">{open ? status : ''}</div>
      {open && (
        <div className="pop search-pop" role="dialog" aria-label="Choose a place">
          <input autoFocus className="search-in" type="search" enterKeyHint="go" placeholder="Search countries, states, cities" aria-label="Search places"
            role="combobox" aria-expanded={hits.length > 0} aria-autocomplete="list" aria-controls="place-results" aria-activedescendant={hits[at] ? `place-${at}` : undefined}
            value={q} onChange={(e) => setQ(e.target.value)} autoComplete="off" spellCheck={false}
            onKeyDown={(e) => {
              if (e.nativeEvent.isComposing || e.keyCode === 229) return
              if (e.key === 'ArrowDown') { kbd.current = true; e.preventDefault(); setAt((a) => Math.max(0, Math.min(hits.length - 1, a + 1))) }
              else if (e.key === 'ArrowUp') { kbd.current = true; e.preventDefault(); setAt((a) => Math.max(0, Math.min(hits.length - 1, a - 1))) }
              else if (e.key === 'Enter' && hits[at]) pick(hits[at])
            }} />
          {heading && <span className="eyebrow pad">{heading}</span>}
          {status && <span className="eyebrow pad" aria-hidden="true">{status}</span>}
          <div role="listbox" id="place-results" aria-label="Places" className="search-list">
            {hits.map((r, i) => (
              <div key={`${r.kind}:${r.slug ?? r.name}:${i}`} id={`place-${i}`} role="option" aria-selected={i === at} className={`opt${i === at ? ' sel' : ''}`} onMouseEnter={() => { kbd.current = false; setAt(i) }} onClick={() => pick(r)}>
                <span className="grow"><span className="name">{r.nameEn ?? r.name} {r.local && r.local !== r.nameEn && <span className="muted">{r.local}</span>}</span><span className="sub">{ITEM_KIND[r.kind]}</span></span>
              </div>
            ))}
          </div>
          <span className="hint">Picking a place moves the map; you can still pan anywhere</span>
        </div>
      )}
    </div>
  )
}

export function MoreMenu({ c }: { c: Controls }) {
  const [open, setOpen] = useState(false)
  const ref = useOutside(open, () => setOpen(false))
  return (
    <div className="pop-wrap" ref={ref}>
      <button type="button" className="btn icon" aria-label="More options" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen(!open)}>{Icon.dots}</button>
      {open && (
        <div role="menu" className="pop right">
          <div className="row between pad">
            <span>Units</span>
            <Segmented label="Units" value={c.units} options={[{ value: 'km', label: 'km' }, { value: 'mi', label: 'mi' }]} onChange={c.setUnits} />
          </div>
          <div className="row between"><span>Map</span><Segmented label="Map" value={c.theme} options={[{ value: 'light', label: 'Light' }, { value: 'dark', label: 'Dark' }]} onChange={c.setTheme} /></div>
            <div className="row between pad">
            <span>Names</span>
            <Segmented label="Names" value={c.lang} options={[{ value: 'local', label: 'Local' }, { value: 'en', label: 'English' }]} onChange={c.setLang} />
          </div>
          <div className="sep" />
          <Toggle on={c.proposed} onChange={c.setProposed}><span className="col"><span>Proposed track</span><span className="muted sm">Hong Kong only for now</span></span></Toggle>
          <Toggle on={c.outline} onChange={c.setOutline}><span className="col"><span>Outline area in view</span><span className="muted sm">A line round the area that fills half the screen; gone when zoomed out</span></span></Toggle>
          <Toggle on={c.inspect} onChange={(v) => { c.setInspect(v); setOpen(false) }}><span className="col"><span>Inspect OSM tags</span><span className="muted sm">For mappers: tooltips show raw tags</span></span></Toggle>
          <div className="sep" />
          <button type="button" role="menuitem" className="item" onClick={c.copyLink}>{c.copied ? Icon.check : Icon.link}{c.copied ? 'Link copied' : 'Copy link to this view'}</button>
        </div>
      )}
    </div>
  )
}

export function Toolbar({ c }: { c: Controls }) {
  return (
    <div role="toolbar" aria-label="Map controls" className="toolbar">
      <div className="brand">{Icon.brand}Rail Globe</div>
      <i className="vsep" />
      <Segmented label="Mode" value="explore" onChange={() => {}} options={[
        { value: 'explore', label: 'Explore' }, { value: 'compare', label: 'Compare', disabled: true, title: 'Compare view arrives in phase 4' }]} />
      <i className="vsep" />
      <RegionPicker c={c} />
      <i className="vsep" />
      <Segmented label="Colour lines by" value={c.mode} onChange={c.setMode} options={[{ value: 'type', label: 'Rail type' }, { value: 'operator', label: 'Operator colours' }, { value: 'speed', label: 'Speed' }]} />
      <Toggle on={c.hsrOnly} onChange={c.setHsrOnly}>HSR only</Toggle>
      <Toggle on={c.construction} onChange={c.setConstruction}>Construction</Toggle>
      <i className="vsep" />
      <button type="button" className="btn" onClick={c.openLines}>{Icon.list}Lines</button>
      <MoreMenu c={c} />
    </div>
  )
}
