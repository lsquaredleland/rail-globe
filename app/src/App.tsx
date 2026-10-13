import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { MapView, type Hit, type MapHandle, type View } from './MapView'
import type { ColorMode, MapState } from './mapStyle'
import { regionBySlug } from './regions'
import { TYPE_ORDER } from './palette'
import { loadLineIndex, loadNamedLines, loadRailways, loadStation, railwayOf, type LineIndex, type NamedLine, type Railways } from './data'
import { readHash, writeHash } from './urlState'
import { Toolbar, RegionPicker, type Controls } from './components/Toolbar'
import { Legend } from './components/Legend'
import { TooltipBody } from './components/Tooltip'
import { LinePanel, LinesList, StationPanel } from './components/Panels'
import { Icon, LangContext, Segmented, Toggle, type Lang } from './components/ui'
import { RegionCard } from './components/RegionCard'
import { areaForView, useStats } from './stats'
import { InspectPanel, InspectTooltipBody } from './components/Inspect'

type Selected = { kind: 'station'; slug: string; id: string } | { kind: 'line'; slug: string; id: number } | { kind: 'lines' } | { kind: 'inspect'; hit: Hit } | null

function useMedia(q: string) {
  const [m, setM] = useState(() => matchMedia(q).matches)
  useEffect(() => {
    const mq = matchMedia(q)
    const h = () => setM(mq.matches)
    mq.addEventListener('change', h)
    return () => mq.removeEventListener('change', h)
  }, [q])
  return m
}

const hitKey = (h: Hit) => `${h.kind}:${h.slug}:${h.props.id ?? h.props.lid}`

export default function App() {
  const init = useMemo(readHash, [])
  const compact = useMedia('(max-width: 719px)')
  const touch = useMedia('(pointer: coarse)')
  const [region, setRegionState] = useState(init.region)
  const [mode, setMode] = useState(init.mode)
  const [hsrOnly, setHsrOnly] = useState(init.hsrOnly)
  const [construction, setConstruction] = useState(init.construction)
  const [proposed, setProposed] = useState(init.proposed)
  const [freight, setFreight] = useState(init.freight)
  const [units, setUnits] = useState(init.units)
  const [lang, setLang] = useState(init.lang)
  const [hidden, setHidden] = useState<string[]>(init.hidden)
  const [inspect, setInspect] = useState(init.inspect)
  const [legendOpen, setLegendOpen] = useState(() => !matchMedia('(max-width: 719px)').matches)
  const [sheetOpen, setSheetOpen] = useState(false)
  const [selected, setSelected] = useState<Selected>(null)
  const [selPt, setSelPt] = useState<[number, number] | null>(null)
  const [hover, setHover] = useState<{ hit: Hit; x: number; y: number } | null>(null)
  const [peek, setPeek] = useState<Hit | null>(null)
  const [indexes, setIndexes] = useState<Record<string, LineIndex>>({})
  const [named, setNamed] = useState<Record<string, NamedLine[]>>({})
  const [railways, setRailways] = useState<Record<string, Railways>>({})
  const railwaysRef = useRef(railways)   // for openHit, which memoized click handlers call
  railwaysRef.current = railways
  const [selTrackName, setSelTrackName] = useState<string | null>(null)
  const [selTrackSlug, setSelTrackSlug] = useState<string | null>(null)   // the region part of the clicked track; null: the Lines list's region
  const [listHoverLine, setListHoverLine] = useState<number | null>(null)
  const [listHoverName, setListHoverName] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const view = useRef<View | null>(init.view)
  const [viewNow, setViewNow] = useState<View>(() => init.view ?? { center: init.region.center, zoom: init.region.zoom })
  const map = useRef<MapHandle>(null)
  const wrap = useRef<HTMLDivElement>(null)

  const ensureIndex = useCallback((slug: string) => {
    loadLineIndex(slug).then((ix) => setIndexes((p) => (p[slug] ? p : { ...p, [slug]: ix }))).catch(() => {})
    loadRailways(slug).then((rw) => setRailways((p) => (p[slug] ? p : { ...p, [slug]: rw }))).catch(() => {})
  }, [])
  useEffect(() => ensureIndex(region.slug), [region.slug, ensureIndex])
  useEffect(() => {
    loadNamedLines(region.slug).then((n) => setNamed((p) => (p[region.slug] ? p : { ...p, [region.slug]: n }))).catch(() => {})
  }, [region.slug])

  // the card follows the view; so does the pilot region in the picker (no fly: the map is already where the user put it)
  const stats = useStats()
  const cardArea = useMemo(() => areaForView([...stats.regions.filter((r) => !regionBySlug(r.slug).noCard), ...stats.areas], stats.shapes, viewNow, window.innerWidth), [stats, viewNow])
  useEffect(() => {
    const r = areaForView(stats.regions, stats.shapes, viewNow, window.innerWidth)
    // depends on the view only: picking a region in the menu must not be undone by the (not yet moved) old view
    if (r) setRegionState((cur) => (cur.slug === r.slug ? cur : regionBySlug(r.slug)))
  }, [stats, viewNow])

  const mapState = useMemo<MapState>(() => ({ mode, construction, proposed, freight, hidden, hsrOnly, lang }), [mode, construction, proposed, freight, hidden, hsrOnly, lang])
  const sync = useCallback(() => writeHash({ region, mode, hsrOnly, construction, proposed, freight, units, lang, hidden, inspect, view: view.current }), [region, mode, hsrOnly, construction, proposed, freight, units, lang, hidden, inspect])
  useEffect(sync, [sync])

  const setRegion = (slug: string) => {
    const r = regionBySlug(slug)
    setRegionState(r)
    setSelected(null); setSelPt(null); setPeek(null)
    map.current?.flyTo({ center: r.center, zoom: compact ? r.zoomPhone : r.zoom })
  }

  const openStation = (slug: string, id: string, pt?: [number, number], fly = false) => {
    setSelected({ kind: 'station', slug, id }); setPeek(null); setHover(null); setSelTrackName(null); ensureIndex(slug)
    if (pt) setSelPt(pt)
    if (fly || !pt) {
      loadStation(slug, id).then((s) => {
        setSelPt(s.pt)
        const z = Math.max(map.current?.getZoom() ?? 0, 13)
        map.current?.flyTo({ center: s.pt, zoom: z })
      }).catch(() => {})
    }
  }
  const openLine = (slug: string, id: number) => { setSelected({ kind: 'line', slug, id }); setListHoverLine(null); setListHoverName(null); setPeek(null); setHover(null); setSelPt(null); ensureIndex(slug) }
  const closePanel = () => { setSelected(null); setSelPt(null); setSelTrackName(null); setListHoverLine(null); setListHoverName(null) }
  const openList = () => { setSelected({ kind: 'lines' }); setSelPt(null); setSelTrackName(null); setPeek(null); setHover(null) }
  const openNamed = (n: NamedLine) => { setSelTrackName(n.n); setSelTrackSlug(null); map.current?.fitBounds(n.bbox) }
  const openHit = (h: Hit) => {
    if (inspect) { setSelected({ kind: 'inspect', hit: h }); setSelPt(h.kind === 'station' ? h.lngLat : null); setPeek(null); setHover(null); setSelTrackName(null); return }
    if (h.kind === 'station') openStation(h.slug, String(h.props.id), h.lngLat)
    else if (h.kind === 'line') openLine(h.slug, Number(h.props.lid))
    else if (h.kind === 'track') {
      // outside inspect mode a click on track opens the first line that runs on it, else the Lines list on that named railway
      const first = Number(String(h.props.ln ?? '').split(',')[0])
      if (Number.isFinite(first) && first > 0) openLine(h.slug, first)
      else {
        const name = railwayOf(railwaysRef.current[h.slug], h.props)?.[0] ?? (h.props.n ? String(h.props.n) : null)
        if (name) { setSelected({ kind: 'lines' }); setSelPt(null); setPeek(null); setHover(null); setSelTrackName(name); setSelTrackSlug(h.slug) }
      }
    }
  }

  const onHover = useCallback((hit: Hit | null, pt: { x: number; y: number }) => {
    if (!hit) { setHover(null); return }
    ensureIndex(hit.slug)
    setHover({ hit, x: pt.x, y: pt.y })
  }, [ensureIndex])

  const onClick = useCallback((hit: Hit | null) => {
    if (touch) {
      if (!hit) { setPeek(null); return }
      ensureIndex(hit.slug)
      setPeek((cur) => {
        if (cur && hitKey(cur) === hitKey(hit) && (inspect || hit.kind === 'station' || hit.kind === 'line' || (hit.kind === 'track' && !!(hit.props.ln || hit.props.n || hit.props.nk != null)))) { setTimeout(() => openHit(hit), 0); return cur }
        return hit
      })
      return
    }
    if (!hit) { closePanel(); return }
    openHit(hit)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [touch, ensureIndex, inspect])

  const legendProps = {
    hidden, onShowAll: () => setHidden([]), onHideAll: () => setHidden([...TYPE_ORDER]), onToggleFreight: () => setFreight((v) => !v),
    onToggleType: (t: string) => setHidden((h) => (h.includes(t) ? h.filter((x) => x !== t) : [...h, t])),
  }

  const copyLink = () => {
    navigator.clipboard?.writeText(location.href).catch(() => {})
    setCopied(true)
    setTimeout(() => setCopied(false), 1800)
  }

  const controls: Controls = {
    region, mode, hsrOnly, setHsrOnly, construction, proposed, setProposed, freight, units, inspect, setInspect, lang, setLang, setRegion, setMode, setConstruction, setFreight, setUnits, copied, copyLink,
    openLines: () => { openList(); setSheetOpen(false) },
  }

  const selectedLine = selected?.kind === 'line' ? selected.id : null
  const idx = indexes[region.slug]
  const panelIdx = selected && (selected.kind === 'station' || selected.kind === 'line') ? indexes[selected.slug] : idx

  // keep the map sized when the panel opens on desktop
  useEffect(() => { map.current?.resizeSoon() }, [selected?.kind, compact])

  // the line under the pointer is highlighted until something is selected (a track piece with no mapped line falls back to its name)
  const hh = !touch && !selected && hover ? hover.hit : null
  const firstLn = hh?.kind === 'track' ? Number(String(hh.props.ln ?? '').split(',')[0]) : NaN
  const hoverLine = hh?.kind === 'line' ? Number(hh.props.lid) : Number.isFinite(firstLn) && firstLn > 0 ? firstLn : null
  // a track with no mapped line highlights its railway: by name zoomed in, by railway number (nk) in the zoomed-out overview
  const hoverName = hh?.kind === 'track' && hoverLine == null ? (railwayOf(railways[hh.slug], hh.props)?.[0] ?? (hh.props.n ? String(hh.props.n) : null)) : null
  // the region the highlighted line belongs to: the selected line's, else the Lines list's (current region), else the hovered one's
  const hlLine = selectedLine ?? listHoverLine ?? hoverLine
  const hlName = selectedLine == null && selTrackName == null ? (listHoverName ?? (listHoverLine == null ? hoverName : null)) : selTrackName
  const hlSlug = selected?.kind === 'line' ? selected.slug
    : selTrackName != null && selTrackSlug ? selTrackSlug   // a clicked track keeps the region part it was hit in, so its highlight is not redone
    : listHoverLine != null || listHoverName != null || selTrackName != null ? region.slug
    : hh ? hh.slug : null
  // hovering highlights straight from the map's hit test (MapView), as long as nothing is selected
  const hoverHighlight = useCallback((hit: Hit | null) => {
    if (!hit) return null
    const first = hit.kind === 'track' ? Number(String(hit.props.ln ?? '').split(',')[0]) : NaN
    const line = hit.kind === 'line' ? Number(hit.props.lid) : Number.isFinite(first) && first > 0 ? first : null
    if (line != null) return { slug: hit.slug, line, name: null }
    if (hit.kind !== 'track') return null
    const name = railwayOf(railwaysRef.current[hit.slug], hit.props)?.[0] ?? (hit.props.n ? String(hit.props.n) : null)
    return name ? { slug: hit.slug, line: null, name } : null
  }, [])
  const tip = !touch && hover && !selected ? hover : null
  const W = wrap.current?.clientWidth ?? 1200
  const tipLeft = tip ? (tip.x + 330 > W ? tip.x - 322 : tip.x + 16) : 0

  return (
    <LangContext.Provider value={lang}>
    <div className={`app${compact ? ' compact' : ''}`} ref={wrap}>
      <MapView ref={map} state={mapState} units={units} initial={init.view ?? { center: region.center, zoom: compact ? region.zoomPhone : region.zoom }}
        highlightSlug={hlLine != null || hlName != null ? hlSlug : null} selectedLine={hlLine} selectedTrackName={hlName} hoverHighlight={touch || selected || listHoverLine != null || listHoverName != null ? null : hoverHighlight} focusSlug={selected?.kind === 'line' ? selected.slug : selTrackName != null ? (selTrackSlug ?? region.slug) : null} selectedStation={selPt} touch={touch}
        onHover={onHover} onClick={onClick} onMoveEnd={(v) => { view.current = v; setViewNow(v); sync() }} />

      {!compact && <Toolbar c={controls} />}
      {compact && (
        <>
          <div className="phone-top">
            <RegionPicker c={controls} block />
            <button type="button" className="btn icon big" aria-label="Map settings" onClick={() => setSheetOpen(true)}>{Icon.sliders}</button>
          </div>
          <div className="phone-bottom">
            <Segmented<ColorMode> label="Colour lines by" value={mode} onChange={setMode} options={[{ value: 'type', label: 'Type' }, { value: 'operator', label: 'Operator' }, { value: 'speed', label: 'Speed' }]} />
          </div>
        </>
      )}

      {!compact && <RegionCard area={cardArea} units={units} />}
      {!compact && !selected && <Legend mode={mode} hsrOnly={hsrOnly} open={legendOpen} setOpen={setLegendOpen} freight={freight} index={idx} {...legendProps} />}
      {!compact && (
        <div className="zoom" role="group" aria-label="Zoom">
          <button type="button" aria-label="Zoom in" onClick={() => map.current?.zoomBy(1)}>{Icon.plus}</button>
          <i />
          <button type="button" aria-label="Zoom out" onClick={() => map.current?.zoomBy(-1)}>{Icon.minus}</button>
        </div>
      )}

      {tip && (
        <div className={`tooltip${inspect ? ' inspect' : ''}`} role="tooltip" style={{ left: tipLeft, top: Math.min(tip.y + 14, (wrap.current?.clientHeight ?? 800) - 260) }}>
          {inspect ? <InspectTooltipBody hit={tip.hit} /> : <TooltipBody hit={tip.hit} index={indexes[tip.hit.slug]} railways={railways[tip.hit.slug]} touch={false} units={units} />}
        </div>
      )}
      {touch && peek && !selected && (
        <div className="peek" role="status">
          <button type="button" className="close" aria-label="Dismiss" onClick={() => setPeek(null)}>{Icon.close}</button>
          {inspect ? <><InspectTooltipBody hit={peek} /><button type="button" className="more" onClick={() => openHit(peek)}>More</button></> : <TooltipBody hit={peek} index={indexes[peek.slug]} railways={railways[peek.slug]} touch onMore={() => openHit(peek)} units={units} />}
        </div>
      )}

      {selected?.kind === 'station' && (
        <StationPanel slug={selected.slug} id={selected.id} index={panelIdx} onClose={closePanel}
          onLine={(id) => openLine(selected.slug, id)} onStation={(id) => openStation(selected.slug, id, undefined, true)} />
      )}
      {selected?.kind === 'line' && (
        <LinePanel slug={selected.slug} id={selected.id} units={units} onClose={closePanel} onBack={openList} onStation={(id) => openStation(selected.slug, id, undefined, true)} />
      )}
      {selected?.kind === 'inspect' && <InspectPanel hit={selected.hit} onClose={closePanel} />}
      {inspect && (
        <div className="insp-banner" role="status"><strong>Inspect on</strong><span className="muted-w">Tooltips show raw OSM tags</span><button type="button" onClick={() => { setInspect(false); closePanel() }}>Turn off</button></div>
      )}
      {selected?.kind === 'lines' && (
        <LinesList index={idx} named={named[region.slug]} regionName={region.name} units={units} onClose={closePanel} onLine={(id) => { setSelTrackName(null); openLine(region.slug, id) }} onNamed={openNamed} activeName={selTrackName} onHoverLine={setListHoverLine} onHoverNamed={setListHoverName} />
      )}

      {compact && sheetOpen && (
        <div className="sheet-wrap" onClick={() => setSheetOpen(false)}>
          <div className="sheet" role="dialog" aria-label="Map settings" onClick={(e) => e.stopPropagation()}>
            <div className="sheet-head"><h2>Map settings</h2><button type="button" className="close static" aria-label="Close" onClick={() => setSheetOpen(false)}>{Icon.close}</button></div>
            <Segmented label="Mode" value="explore" onChange={() => {}} options={[{ value: 'explore', label: 'Explore' }, { value: 'compare', label: 'Compare', disabled: true, title: 'Compare view arrives in phase 4' }]} />
            <h3 className="eyebrow">Colour lines by</h3>
            <Segmented<ColorMode> label="Colour lines by" value={mode} onChange={setMode} options={[{ value: 'type', label: 'Rail type' }, { value: 'operator', label: 'Operator colours' }, { value: 'speed', label: 'Speed' }]} />
            <h3 className="eyebrow">Show</h3>
            <Toggle on={hsrOnly} onChange={setHsrOnly}>HSR only</Toggle>
            <Toggle on={construction} onChange={setConstruction}>Under construction</Toggle>
            <Toggle on={proposed} onChange={setProposed}>Proposed (Hong Kong)</Toggle>
            <Toggle on={freight} onChange={setFreight}>Freight and industrial</Toggle>
            <button type="button" className="btn wide" onClick={controls.openLines}>{Icon.list}Line list<span className="muted grow-r">{idx ? Object.keys(idx).length : ''}</span></button>
            <h3 className="eyebrow">Legend</h3>
            <Legend mode={mode} hsrOnly={hsrOnly} open setOpen={() => {}} freight={freight} index={idx} compact {...legendProps} />
            <h3 className="eyebrow">More</h3>
            <div className="row between"><span>Units</span><Segmented<'km' | 'mi'> label="Units" value={units} onChange={setUnits} options={[{ value: 'km', label: 'km' }, { value: 'mi', label: 'mi' }]} /></div>
            <div className="row between"><span>Names</span><Segmented<Lang> label="Names" value={lang} onChange={setLang} options={[{ value: 'local', label: 'Local' }, { value: 'en', label: 'English' }]} /></div>
            <Toggle on={inspect} onChange={(v) => { setInspect(v); setSheetOpen(false) }}>Inspect OSM tags<span className="muted sm"> taps show raw tags</span></Toggle>
            <button type="button" className="btn wide" onClick={copyLink}>{copied ? Icon.check : Icon.link}{copied ? 'Link copied' : 'Copy link to this view'}</button>
          </div>
        </div>
      )}
    </div>
    </LangContext.Provider>
  )
}
