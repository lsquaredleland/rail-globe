import { useEffect, useRef, useState } from 'react'
import { REGIONS, type Region } from '../regions'
import type { ColorMode } from '../mapStyle'
import { Icon, Segmented, Toggle, type Lang } from './ui'

export interface Controls {
  region: Region; mode: ColorMode; hsrOnly: boolean; setHsrOnly(v: boolean): void; construction: boolean; proposed: boolean; freight: boolean; units: 'km' | 'mi'; lang: Lang; setLang(l: Lang): void; inspect: boolean
  setRegion(slug: string): void; setMode(m: ColorMode): void; setConstruction(v: boolean): void; setProposed(v: boolean): void; setFreight(v: boolean): void
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

export function RegionPicker({ c, block }: { c: Controls; block?: boolean }) {
  const [open, setOpen] = useState(false)
  const ref = useOutside(open, () => setOpen(false))
  return (
    <div className={`pop-wrap${block ? ' block' : ''}`} ref={ref}>
      <button type="button" className="btn region" aria-haspopup="listbox" aria-expanded={open} onClick={() => setOpen(!open)}>
        {Icon.map}<strong>{c.region.name}</strong><span className="muted">{c.region.sub}</span>{Icon.chevron(open)}
      </button>
      {open && (
        <div role="listbox" aria-label="Region" className="pop">
          <span className="eyebrow pad">Pilot regions</span>
          {REGIONS.map((r) => (
            <div key={r.slug} role="option" aria-selected={r.slug === c.region.slug} tabIndex={0} className={`opt${r.slug === c.region.slug ? ' sel' : ''}`}
              onClick={() => { c.setRegion(r.slug); setOpen(false) }}
              onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { c.setRegion(r.slug); setOpen(false) } }}>
              <span className="grow"><span className="name">{r.name} {r.local && <span lang="zh" className="muted">{r.local}</span>}</span><span className="sub">{r.sub}</span></span>
              {r.slug === c.region.slug && Icon.check}
            </div>
          ))}
          <span className="hint">Picking a region moves the map; you can still pan anywhere</span>
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
          <div className="row between pad">
            <span>Names</span>
            <Segmented label="Names" value={c.lang} options={[{ value: 'local', label: 'Local' }, { value: 'en', label: 'English' }]} onChange={c.setLang} />
          </div>
          <div className="sep" />
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
      <div className="brand">{Icon.brand}Rail Atlas</div>
      <i className="vsep" />
      <Segmented label="Mode" value="explore" onChange={() => {}} options={[
        { value: 'explore', label: 'Explore' }, { value: 'compare', label: 'Compare', disabled: true, title: 'Compare view arrives in phase 4' }]} />
      <i className="vsep" />
      <RegionPicker c={c} />
      <i className="vsep" />
      <Segmented label="Colour lines by" value={c.mode} onChange={c.setMode} options={[{ value: 'type', label: 'Rail type' }, { value: 'operator', label: 'Operator colours' }, { value: 'speed', label: 'Speed' }]} />
      <Toggle on={c.hsrOnly} onChange={c.setHsrOnly}>HSR only</Toggle>
      <Toggle on={c.construction} onChange={c.setConstruction}>Construction</Toggle>
      <Toggle on={c.proposed} onChange={c.setProposed}>Proposed</Toggle>
      <i className="vsep" />
      <button type="button" className="btn" onClick={c.openLines}>{Icon.list}Lines</button>
      <MoreMenu c={c} />
    </div>
  )
}
