import type { Map as MLMap } from 'maplibre-gl'
import { THEME_COLORS, type Theme } from '../palette'
import { TILES } from '../regions'
import { L, get } from './shared'
import { PLACE_LAYERS } from './places'

const themeOf = new WeakMap<MLMap, Theme>()
/** Light or dark map colours: the basemap, the station symbols, the labels and the ground under stations. A no-op when the theme is unchanged. */
export function applyTheme(map: MLMap, theme: Theme, slug: string = TILES) {
  if (themeOf.get(map) === theme) return
  themeOf.set(map, theme)
  const t = THEME_COLORS[theme]
  const set = (id: string, prop: string, v: unknown) => { if (map.getLayer(id)) map.setPaintProperty(id, prop as never, v as never) }
  const r = (n: string) => L(slug, n)
  set('background', 'background-color', t.land)
  set('landcover-wood', 'fill-color', t.wood); set('landcover-farm', 'fill-color', t.farm); set('landuse-urban', 'fill-color', t.urban)
  set('park', 'fill-color', t.park)
  set('water', 'fill-color', t.water); set('water', 'fill-outline-color', t.waterEdge)
  set('waterway', 'line-color', t.water)
  set('road-major', 'line-color', t.road); set('road-secondary', 'line-color', t.road)
  set('admin', 'line-color', t.border4); set('admin-country', 'line-color', t.border)
  for (const id of PLACE_LAYERS) { set(id, 'text-color', t.label); set(id, 'text-halo-color', t.land) }
  set(r('veil'), 'background-color', t.land)
  set(r('airport-fill'), 'fill-color', t.airport); set(r('airport-line'), 'line-color', t.airportLine)
  set(r('airport-terminal'), 'fill-color', t.terminal); set(r('airport-terminal'), 'fill-outline-color', t.terminalEdge)
  set(r('ground'), 'fill-color', t.ground); set(r('yard-fill'), 'fill-color', t.yard); set(r('ground-edge'), 'line-color', t.groundEdge)
  set(r('buildings'), 'fill-color', t.building); set(r('buildings'), 'fill-outline-color', t.buildingEdge)
  set(r('platforms-fill'), 'fill-color', t.platform); set(r('platforms-line'), 'line-color', t.platform)
  set(r('casing'), 'line-color', t.casing)
  const ring = (id: string, w: unknown) => set(r(id), 'circle-stroke-color', w)
  for (const id of ['st1', 'st2', 'st3']) set(r(id), 'circle-color', t.stationFill)
  ring('st3', t.st3); ring('st2', t.ink); ring('st1', ['case', ['==', get('hsr'), 1], t.hsr, t.ink])
  for (const id of ['stcon1', 'stcon2', 'stcon3']) set(r(id), 'circle-color', t.stationFill)
  for (const id of ['lbl1', 'lbl2', 'lbl1t', 'lbl2t', 'lbl3', 'lblcon1', 'lblcon2', 'lblcon3']) { set(r(id), 'text-color', id.startsWith('lbl') && !id.includes('con') ? t.ink : t.ink3); set(r(id), 'text-halo-color', t.land) }
  for (const id of ['lbl-lines', 'lbl-track']) { set(r(id), 'text-color', t.ink2); set(r(id), 'text-halo-color', t.land) }
  set(r('cities'), 'circle-stroke-color', t.land)
  set('area-outline', 'line-color', t.ink); set('area-outline-shade', 'line-color', t.ink)
}
