import { useCallback, useEffect, useState, type MutableRefObject } from 'react'
import type { View } from './MapView'
import { SPEED_BANDS, SPEED_NONE, type ColorMode } from './mapStyle'
import { TYPE_ORDER, type Theme } from './palette'
import { hsMinKmh } from './regions'
import { writeHash, type UrlState } from './urlState'

/** The settings the URL hash carries (urlState.ts): read once at start, written back whenever one changes or the map moves. */
export function useSettings(init: UrlState, view: MutableRefObject<View | null>) {
  const [region, setRegionState] = useState(init.region)
  const [mode, setMode] = useState<ColorMode>(init.mode)
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

  const sync = useCallback(() => writeHash({ region, mode, hsrOnly, construction, proposed, freight, units, lang, hidden, hiddenSpeed, inspect, outline, theme, view: view.current }),
    [region, mode, hsrOnly, construction, proposed, freight, units, lang, hidden, hiddenSpeed, inspect, outline, theme, view])
  useEffect(sync, [sync])

  return {
    region, setRegionState, mode, setMode, hsrOnly, setHsrOnlyState, construction, setConstruction, proposed, setProposed, freight, setFreight, units, setUnits,
    lang, setLang, theme, setTheme, hidden, setHidden, hiddenSpeed, setHiddenSpeed, inspect, setInspect, outline, setOutline, sync,
  }
}
type Settings = ReturnType<typeof useSettings>

/** One state behind every switch: the type cards and the speed card hide the same track. High-speed is both a type and the bands from
 *  `hsMinKmh(areaSlug)` up (250 in China, 200 elsewhere, or what the region's stats.json says), so hiding it in one card hides it in the other. */
export function useTypeSpeedSwitches(s: Settings, areaSlug: string) {
  const { hidden, setHidden, hiddenSpeed, setHiddenSpeed, setHsrOnlyState, setFreight } = s
  const HS_BANDS = SPEED_BANDS.filter((b) => b.min >= hsMinKmh(areaSlug)).map((b) => b.min)
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
  const speedProps = { hiddenSpeed, hsrOnly: s.hsrOnly, hsMin: hsMinKmh(areaSlug), onShowAll: showAll, onHideAll: hideAll, onToggle: toggleBand }
  return { setHsrOnly, legendProps, speedProps }
}
