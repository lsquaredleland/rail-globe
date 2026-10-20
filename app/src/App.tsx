import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { MapView, type Hit, type MapHandle, type View } from './MapView'
import { SPEED_BANDS, SPEED_NONE, type ColorMode, type MapState } from './mapStyle'
import type { Theme } from './palette'
import { regionBySlug, registerSearch, hsMinKmh, isKnownRegion, partAt, onPartsChange, DEFAULT_REGION } from './regions'
import { featured, itemInView, useSearch, type SearchItem } from './search'
import { TYPE_ORDER } from './palette'
import { loadBoundary, loadLine, loadLineIndex, loadNamedLines, loadRailways, loadStation, railwayOf, type LineIndex, type NamedLine, type Railways } from './data'
import { ignoreError } from './dev'
import { readHash, writeHash } from './urlState'
import { Toolbar, RegionPicker, type Controls } from './components/Toolbar'
import { Legend } from './components/Legend'
import { TooltipBody } from './components/Tooltip'
import { LinePanel, LinesList, ServicesPanel, StationPanel } from './components/Panels'
import { railTypeLabel } from './format'
import { Icon, LangContext, Segmented, Toggle, type Lang } from './components/ui'
import { SpeedCard, TypeCard, WorldCard } from './components/AreaCard'
import { areaForView, useStats } from './stats'
import { InspectPanel, InspectTooltipBody } from './components/Inspect'

type Services = { kind: 'services'; slug: string; ids: number[]; title: string; titleEn: string | null; sub: string }
type Selected = { kind: 'station'; slug: string; id: string } | { kind: 'line'; slug: string; id: number; from?: { sel: Selected; pt: [number, number] | null } } | Services | { kind: 'lines' } | { kind: 'inspect'; hit: Hit } | null

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
  const [hsrOnly, setHsrOnlyState] = useState(init.hsrOnly)
  const [construction, setConstruction] = useState(init.construction)
  const [proposed, setProposed] = useState(init.proposed)
  const [freight, setFreight] = useState(init.freight)
  const [units, setUnits] = useState(init.units)
  const [lang, setLang] = useState(init.lang)
  const [theme, setTheme] = useState<Theme>(init.theme)
  const [hidden, setHidden] = useState<string[]>(init.hidden)
  const [hiddenSpeed, setHiddenSpeed] = useState<number[]>(() => (init.hidden.includes('high_speed') ? [...new Set([...init.hiddenSpeed, 400, 350, 300, 250])] : init.hiddenSpeed))
  const [inspect, setInspect] = useState(init.inspect)
  const [outline, setOutline] = useState(init.outline)
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

  const { items: found, status: placesStatus, retry: retryPlaces } = useSearch()   // search.json: the countries, provinces / states and cities with rail
  const ensureIndex = useCallback((slug: string) => {
    loadLineIndex(slug).then((ix) => setIndexes((p) => (p[slug] ? p : { ...p, [slug]: ix }))).catch(ignoreError(`${slug}/lines-index.json`))
    loadRailways(slug).then((rw) => setRailways((p) => (p[slug] ? p : { ...p, [slug]: rw }))).catch(ignoreError(`${slug}/railways.json`))
  }, [])
  // side files (lines, railways) exist per country; a province or the world placeholder has none
  const hasData = useCallback((slug: string) => found.some((i) => i.kind === 'country' && i.slug === slug), [found])
  useEffect(() => { if (hasData(region.slug)) ensureIndex(region.slug) }, [region.slug, ensureIndex, hasData])
  useEffect(() => {
    if (!hasData(region.slug)) return
    loadNamedLines(region.slug).then((n) => setNamed((p) => (p[region.slug] ? p : { ...p, [region.slug]: n }))).catch(ignoreError(`${region.slug}/named-lines.json`))
  }, [region.slug, hasData])

  // the card follows the view; so does the region in the picker (no fly: the map is already where the user put it)
  const items = useMemo(() => {
    registerSearch(found)
    return found
  }, [found])
  const foundFeatured = useMemo(() => featured(found), [found])
  const stats = useStats(found, viewNow, region.slug)
  const [boundaryGeo, setBoundaryGeo] = useState<{ slug: string; geo: GeoJSON.Geometry | null } | null>(null)
  // the card follows the outline: it is there only while an area fills the screen (the same test as the outline), and goes away when zoomed out further
  // World build: a country's box can cover far islands and its neighbours, so the country under the view centre is the one whose outline holds it
  const [partsVer, setPartsVer] = useState(0)   // partAt changes answer once world.parts.json and the outlines of overlapping countries load: the card is worked out again then, without waiting for a pan
  const viewNowRef = useRef(viewNow)
  viewNowRef.current = viewNow
  const partSeen = useRef('')
  useEffect(() => onPartsChange(() => { const p = partAt(viewNowRef.current.center); if (p !== partSeen.current) { partSeen.current = p; setPartsVer((v) => v + 1) } }), [])
  const overRegions = useMemo(() => { void partsVer; return stats.regions.filter((r) => r.slug === partAt(viewNow.center)) }, [stats, viewNow, partsVer])
  const allAreas = useMemo(() => [...overRegions.filter((r) => !regionBySlug(r.slug).noCard), ...stats.areas], [overRegions, stats])
  // the area that fills half the screen: it gets the outline, which goes away when zoomed out further
  // the place picked in the search keeps the card and outline while it still fills 15% of the screen's width (else Chengdu would give way to its New Area)
  const [pickedSlug, setPickedSlug] = useState<string | null>(null)
  const viewArea = useMemo(() => {
    const p = pickedSlug ? allAreas.filter((a) => a.slug === pickedSlug) : []
    return areaForView(p, stats.shapes, viewNow, window.innerWidth, 0.15) ?? areaForView(allAreas, stats.shapes, viewNow, window.innerWidth)
  }, [allAreas, stats, viewNow, pickedSlug])
  const viewItem = useMemo(() => { void partsVer; return itemInView(found, viewNow, window.innerWidth, partAt(viewNow.center)) }, [found, viewNow, partsVer])
  // the card shows exactly when the outline can: the area that fills the screen, and nothing at the zoom levels where no area does
  const cardArea = viewArea
  // the outline of the area in view: its simplified shape, else its boundary file
  const shape = viewArea ? stats.shapes[viewArea.slug] : undefined
  const needBoundary = outline && viewArea && !shape ? viewArea.slug : null
  useEffect(() => { if (needBoundary) loadBoundary(needBoundary).then((geo) => setBoundaryGeo({ slug: needBoundary, geo })) }, [needBoundary])
  const outlineGeo = useMemo<GeoJSON.Geometry | null>(() => {
    if (!outline || !viewArea) return null
    if (shape) return { type: 'MultiPolygon', coordinates: shape }
    return boundaryGeo?.slug === viewArea.slug ? boundaryGeo.geo : null
  }, [outline, viewArea, shape, boundaryGeo])
  useEffect(() => {
    const r = areaForView(overRegions, stats.shapes, viewNow, window.innerWidth)
    // depends on the view only: picking a region in the menu must not be undone by the (not yet moved) old view
    if (r) setRegionState((cur) => (cur.slug === r.slug ? cur : regionBySlug(r.slug)))
  }, [overRegions, stats, viewNow])

  // a link to a country or province (#r=de-bavaria) before search.json arrived named a placeholder: now it is known, go there (unless the link had a view)
  useEffect(() => {
    if (!found.length || init.view || !isKnownRegion(region.slug)) return
    const r = regionBySlug(region.slug)
    if (r !== region) { setRegionState(r); map.current?.flyTo({ center: r.center, zoom: compact ? r.zoomPhone : r.zoom }) }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [found])

  const mapState = useMemo<MapState>(() => ({ mode, construction, proposed, freight, hidden, hiddenSpeed, hsrOnly, lang, theme }), [mode, construction, proposed, freight, hidden, hiddenSpeed, hsrOnly, lang, theme])
  const sync = useCallback(() => writeHash({ region, mode, hsrOnly, construction, proposed, freight, units, lang, hidden, hiddenSpeed, inspect, outline, theme, view: view.current }), [region, mode, hsrOnly, construction, proposed, freight, units, lang, hidden, hiddenSpeed, inspect, outline, theme])
  useEffect(sync, [sync])

  const setRegion = (slug: string) => {
    const r = regionBySlug(slug)
    setRegionState(r)
    setSelected(null); setSelPt(null); setPeek(null)
    map.current?.flyTo({ center: r.center, zoom: compact ? r.zoomPhone : r.zoom })
  }

  /** a pick from the search: a hand-tuned region as before, else fly to the outline's box (country, province, a city with an outline) or to the city */
  const pickPlace = (it: SearchItem) => {
    setPickedSlug(it.slug)
    setSelected(null); setSelPt(null); setPeek(null)
    // a city's box can be mostly water or sprawl (Boston): its own point lies inside it, the middle of its box may not
    const center: [number, number] = it.bbox && it.kind !== 'city' ? [(it.bbox[0] + it.bbox[2]) / 2, (it.bbox[1] + it.bbox[3]) / 2] : it.center
    map.current?.flyTo({ center, zoom: compact ? Math.max(1.5, it.zoom - 1) : it.zoom })
  }

  const openStation = (slug: string, id: string, pt?: [number, number], fly = false) => {
    setSelected({ kind: 'station', slug, id }); setPeek(null); setHover(null); setSelTrackName(null); ensureIndex(slug)
    if (pt) setSelPt(pt)
    if (fly || !pt) {
      loadStation(slug, id).then((s) => {
        setSelPt(s.pt)
        const z = Math.max(map.current?.getZoom() ?? 0, 13)
        map.current?.flyTo({ center: s.pt, zoom: z })
      }).catch(ignoreError(`station ${slug}/${id}`))
    }
  }
  const openLine = (slug: string, id: number, from?: { sel: Selected; pt: [number, number] | null }) => { setSelected({ kind: 'line', slug, id, from }); setListHoverLine(null); setListHoverName(null); setPeek(null); setHover(null); setSelPt(null); ensureIndex(slug) }
  const closePanel = () => { setSelected(null); setSelPt(null); setSelTrackName(null); setListHoverLine(null); setListHoverName(null) }
  const openList = () => { setSelected({ kind: 'lines' }); setSelPt(null); setSelTrackName(null); setPeek(null); setHover(null) }
  const openNamed = (n: NamedLine) => { setSelTrackName(n.n); setSelTrackSlug(null); map.current?.fitBounds(n.bbox) }
  const openHit = (h: Hit) => {
    if (inspect) { setSelected({ kind: 'inspect', hit: h }); setSelPt(h.kind === 'station' ? h.lngLat : null); setPeek(null); setHover(null); setSelTrackName(null); return }
    if (h.kind === 'station') openStation(h.slug, String(h.props.id), h.lngLat)
    else if (h.kind === 'line') openLine(h.slug, Number(h.props.lid))
    else if (h.kind === 'track') {
      // outside inspect mode a click on track opens the line that runs on it, or lets the user choose when several do; with none, the Lines list on that named railway
      const ids = [...new Set(String(h.props.ln ?? '').split(',').map(Number).filter((n) => Number.isFinite(n) && n > 0))]
      if (ids.length === 1) openLine(h.slug, ids[0])
      else if (ids.length > 1) {
        const rw = railwayOf(railwaysRef.current[h.slug], h.props)
        const name = rw?.[0] ?? (h.props.n ? String(h.props.n) : '')
        const type = String(h.props.type ?? '')
        setSelected({ kind: 'services', slug: h.slug, ids, title: name || railTypeLabel(type), titleEn: rw?.[1] ?? (h.props.ne ? String(h.props.ne) : null), sub: `${railTypeLabel(type, h.props.usage as string | undefined)}${h.props.prop ? ' · proposed' : h.props.con ? ' · under construction' : ''}` })
        setSelPt(null); setPeek(null); setHover(null); setSelTrackName(null); setListHoverLine(null); setListHoverName(null); ensureIndex(h.slug)
      } else {
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

  // One state behind every switch: the type cards and the speed card hide the same track. High-speed is both a type and the four
  // bands from 250 km/h up (200 outside China), so hiding it in one card hides it in the other.
  const areaSlug = cardArea?.slug ?? (partAt(viewNow.center) || region.slug)   // zoomed out: the country under the centre sets where high-speed starts
  const HS_BANDS = SPEED_BANDS.filter((b) => b.min >= hsMinKmh(areaSlug)).map((b) => b.min)   // 250 in China, 200 elsewhere
  const allSpeed = [...SPEED_BANDS.map((b) => b.min), SPEED_NONE]
  const withHs = (sp: number[], off: boolean) => (off ? [...new Set([...sp, ...HS_BANDS])] : sp.filter((m) => !HS_BANDS.includes(m)))
  // HSR only needs high-speed track to show: turning it on brings high-speed (type and speed bands) back
  const setHsrOnly = (on: boolean) => {
    setHsrOnlyState(on)
    if (on) { setHidden((h) => h.filter((x) => x !== 'high_speed')); setHiddenSpeed((sp) => withHs(sp, false)) }
  }
  const toggleType = (t: string) => {
    const off = !hidden.includes(t)
    setHidden((h) => (off ? [...h, t] : h.filter((x) => x !== t)))
    // after None every speed band is hidden too, and track must pass both cards: turning a type on from there shows all speeds
    if (!off && allSpeed.every((m) => hiddenSpeed.includes(m))) setHiddenSpeed([])
    else if (t === 'high_speed') setHiddenSpeed((sp) => withHs(sp, off))
  }
  const toggleBand = (m: number) => {
    const sp = hiddenSpeed.includes(m) ? hiddenSpeed.filter((x) => x !== m) : [...hiddenSpeed, m]
    // after None every type is hidden too: turning a band on from there shows all types, of that band
    if (!sp.includes(m) && TYPE_ORDER.every((t) => hidden.includes(t))) setHidden([])
    setHiddenSpeed(sp)
    if (HS_BANDS.includes(m)) {
      const allHsOff = HS_BANDS.every((b) => sp.includes(b))
      setHidden((h) => (allHsOff ? (h.includes('high_speed') ? h : [...h, 'high_speed']) : h.filter((x) => x !== 'high_speed')))
    }
  }
  const showAll = () => { setHidden([]); setHiddenSpeed([]) }
  const hideAll = () => { setHidden([...TYPE_ORDER]); setHiddenSpeed(allSpeed) }
  const legendProps = { hidden, onShowAll: showAll, onHideAll: hideAll, onToggleFreight: () => setFreight((v) => !v), onToggleType: toggleType }
  const speedProps = { hiddenSpeed, hsrOnly, china: hsMinKmh(areaSlug) >= 250, onShowAll: showAll, onHideAll: hideAll, onToggle: toggleBand }

  const copyLink = () => {
    navigator.clipboard?.writeText(location.href).catch(ignoreError('clipboard'))
    setCopied(true)
    setTimeout(() => setCopied(false), 1800)
  }

  const controls: Controls = {
    area: cardArea, inView: viewItem, items, featured: foundFeatured, placesStatus, retryPlaces, pickPlace, region, fallback: DEFAULT_REGION, mode, hsrOnly, setHsrOnly, construction, proposed, setProposed, outline, setOutline, freight, units, inspect, setInspect, lang, setLang, theme, setTheme, setRegion, setMode, setConstruction, setFreight, setUnits, copied, copyLink,
    openLines: () => { openList(); setSheetOpen(false) },
  }

  const selectedLine = selected?.kind === 'line' ? selected.id : null
  // a selected station highlights every line that stops there, a selected track every service on it; the rest is veiled
  const [stLines, setStLines] = useState<{ key: string; ids: number[] } | null>(null)
  const stKey = selected?.kind === 'station' ? `${selected.slug}/${selected.id}` : null
  useEffect(() => {
    if (selected?.kind !== 'station') return
    const { slug, id } = selected
    let live = true
    loadStation(slug, id).then((s) => live && setStLines({ key: `${slug}/${id}`, ids: Object.values(s.lines).flat() })).catch(ignoreError(`station ${slug}/${id}`))
    return () => { live = false }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stKey])
  const groupLines = useMemo<number[] | null>(() => {
    if (selected?.kind === 'services') return selected.ids
    return stKey && stLines?.key === stKey && stLines.ids.length ? stLines.ids : null
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected, stKey, stLines])
  const idx = indexes[region.slug]
  const panelIdx = selected && (selected.kind === 'station' || selected.kind === 'line' || selected.kind === 'services') ? indexes[selected.slug] : idx

  // keep the map sized when the panel opens on desktop
  useEffect(() => { map.current?.resizeSoon() }, [selected?.kind, compact])

  // the line under the pointer is highlighted until something is selected (a track piece with no mapped line falls back to its name)
  const hh = !touch && !selected && hover ? hover.hit : null
  const firstLn = hh?.kind === 'track' ? Number(String(hh.props.ln ?? '').split(',')[0]) : NaN
  const hoverLine = hh?.kind === 'line' ? Number(hh.props.lid) : Number.isFinite(firstLn) && firstLn > 0 ? firstLn : null
  // a track with no mapped line highlights its railway: by name zoomed in, by railway number (nk) in the zoomed-out overview
  const hoverName = hh?.kind === 'track' && hoverLine == null ? (railwayOf(railways[hh.slug], hh.props)?.[0] ?? (hh.props.n ? String(hh.props.n) : null)) : null
  // the region the highlighted line belongs to: the selected line's, else the Lines list's (current region), else the hovered one's
  const hlLine = selectedLine ?? listHoverLine ?? groupLines ?? hoverLine
  const hlName = selectedLine == null && selTrackName == null ? (listHoverName ?? (listHoverLine == null ? hoverName : null)) : selTrackName
  const hlSlug = selected?.kind === 'line' || (groupLines && (selected?.kind === 'services' || selected?.kind === 'station')) ? selected.slug
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
  // stations: only those on the focused lines (the selected line, the services on a clicked track, a station's lines, or the row hovered in a panel) stay at full strength
  const focusLines = selectedLine ?? listHoverLine ?? groupLines
  const focusKey = focusLines == null || hlSlug == null ? '' : `${hlSlug}|${Array.isArray(focusLines) ? focusLines.join(',') : focusLines}|${selected?.kind === 'station' ? selected.id : ''}`
  const [focusStations, setFocusStations] = useState<string[] | null>(null)
  useEffect(() => {
    if (!focusKey) { setFocusStations(null); return }
    const [slug, ids, own] = focusKey.split('|')
    let live = true
    Promise.all(ids.split(',').map((id) => loadLine(slug, Number(id)).catch(() => null))).then((ls) => {
      if (!live) return
      const set = new Set<string>(own ? [own] : [])
      for (const l of ls) for (const r of l?.routes ?? []) for (const s of r.stations) set.add(s)
      setFocusStations(set.size ? [...set] : null)
    })
    return () => { live = false }   // the previous set stays on the map until the new one is in: no flash between rows
  }, [focusKey])
  const tip = !touch && hover && !selected ? hover : null
  const W = wrap.current?.clientWidth ?? 1200
  const tipLeft = tip ? (tip.x + 330 > W ? tip.x - 322 : tip.x + 16) : 0

  return (
    <LangContext.Provider value={lang}>
    <div className={`app${compact ? ' compact' : ''}`} ref={wrap}>
      <MapView ref={map} state={mapState} units={units} initial={init.view ?? { center: region.center, zoom: compact ? region.zoomPhone : region.zoom }}
        highlightSlug={hlLine != null || hlName != null ? hlSlug : null} selectedLine={hlLine} selectedTrackName={hlName} hoverHighlight={touch || selected || listHoverLine != null || listHoverName != null ? null : hoverHighlight} focusSlug={selected?.kind === 'line' || (groupLines && selected?.kind === 'station') || selected?.kind === 'services' ? selected.slug : selTrackName != null ? (selTrackSlug ?? region.slug) : null} focusStations={focusStations} selectedStation={selPt} outline={outlineGeo} touch={touch}
        onHover={onHover} onClick={onClick} onMoveEnd={(v) => { view.current = v; setViewNow(v); sync() }} />

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

      {/* toolbar and card share one column, so the card moves down when the toolbar grows (wraps) */}
      {!compact && <div className="left-stack"><Toolbar c={controls} />{mode === 'speed'
        ? <SpeedCard area={cardArea} units={units} construction={construction} proposed={proposed} {...speedProps} />
        : cardArea
          ? mode !== 'operator' && <TypeCard area={cardArea} units={units} hidden={hidden} freight={freight} construction={construction} proposed={proposed} hsrOnly={hsrOnly}
            onToggleType={legendProps.onToggleType} onShowAll={legendProps.onShowAll} onHideAll={legendProps.onHideAll} onToggleFreight={legendProps.onToggleFreight} />
          : <WorldCard operator={mode === 'operator'} zoom={viewNow.zoom} hidden={hidden} freight={freight} hsrOnly={hsrOnly}
            onToggleType={legendProps.onToggleType} onShowAll={legendProps.onShowAll} onHideAll={legendProps.onHideAll} onToggleFreight={legendProps.onToggleFreight} />}</div>}
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
          onLine={(id) => openLine(selected.slug, id, { sel: selected, pt: selPt })} onStation={(id) => openStation(selected.slug, id, undefined, true)} onHoverLine={setListHoverLine} />
      )}
      {selected?.kind === 'line' && (
        <LinePanel slug={selected.slug} id={selected.id} units={units} onClose={closePanel}
          onBack={selected.from ? () => { setSelected(selected.from!.sel); setSelPt(selected.from!.pt); setListHoverLine(null) } : openList}
          backLabel={selected.from ? (selected.from.sel?.kind === 'station' ? 'Station' : 'Services here') : undefined} onStation={(id) => openStation(selected.slug, id, undefined, true)} />
      )}
      {selected?.kind === 'services' && (
        <ServicesPanel ids={selected.ids} index={panelIdx} title={selected.title} titleEn={selected.titleEn} sub={selected.sub} units={units} onClose={closePanel}
          onLine={(id) => openLine(selected.slug, id, { sel: selected, pt: null })} onHoverLine={setListHoverLine} />
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
            <Toggle on={outline} onChange={setOutline}>Outline area in view</Toggle>
            <button type="button" className="btn wide" onClick={controls.openLines}>{Icon.list}Line list<span className="muted grow-r">{idx ? Object.keys(idx).length : ''}</span></button>
            <h3 className="eyebrow">Legend</h3>
            {mode === 'speed'
              ? <SpeedCard area={cardArea} units={units} construction={construction} proposed={proposed} embedded {...speedProps} />
              : mode === 'operator'
                ? <WorldCard embedded operator zoom={viewNow.zoom} hidden={hidden} freight={freight} hsrOnly={hsrOnly} onToggleType={legendProps.onToggleType} onShowAll={legendProps.onShowAll} onHideAll={legendProps.onHideAll} onToggleFreight={legendProps.onToggleFreight} />
                : <Legend mode={mode} hsrOnly={hsrOnly} open setOpen={() => {}} freight={freight} index={idx} compact {...legendProps} />}
            <h3 className="eyebrow">More</h3>
            <div className="row between"><span>Units</span><Segmented<'km' | 'mi'> label="Units" value={units} onChange={setUnits} options={[{ value: 'km', label: 'km' }, { value: 'mi', label: 'mi' }]} /></div>
            <div className="row between"><span>Map</span><Segmented<Theme> label="Map" value={theme} onChange={setTheme} options={[{ value: 'light', label: 'Light' }, { value: 'dark', label: 'Dark' }]} /></div>
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
