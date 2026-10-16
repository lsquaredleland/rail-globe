import type { ColorMode } from './mapStyle'
import { DEFAULT_REGION, regionBySlug, type Region } from './regions'
import type { View } from './MapView'
import type { Theme } from './palette'

export interface UrlState { region: Region; mode: ColorMode; hsrOnly: boolean; construction: boolean; proposed: boolean; freight: boolean; units: 'km' | 'mi'; lang: 'local' | 'en'; hidden: string[]; hiddenSpeed: number[]; inspect: boolean; outline: boolean; theme: Theme; view: View | null }

export function readHash(): UrlState {
  const h = new URLSearchParams(location.hash.replace(/^#/, ''))
  const region = regionBySlug(h.get('r') ?? DEFAULT_REGION.slug)
  let view: View | null = null
  const v = h.get('v')?.split('/').map(Number)
  if (v && v.length === 3 && v.every(Number.isFinite)) view = { zoom: v[0], center: [v[2], v[1]] }
  return {
    region, mode: h.get('c') === 'operator' ? 'operator' : h.get('c') === 'speed' ? 'speed' : 'type', hsrOnly: h.get('hsr') === '1', construction: h.get('con') !== '0', proposed: h.get('prop') === '1', freight: h.get('fr') === '1',
    units: h.get('u') === 'mi' ? 'mi' : 'km', lang: h.get('lg') === 'en' ? 'en' : 'local', hidden: (h.get('h') ?? '').split(',').filter(Boolean), hiddenSpeed: (h.get('hs') ?? '').split(',').filter(Boolean).map(Number).filter(Number.isFinite), inspect: h.get('ins') === '1', outline: h.get('ol') !== '0', theme: h.get('th') === 'dark' ? 'dark' : 'light', view,
  }
}

export function writeHash(s: Omit<UrlState, 'view'> & { view: View | null }) {
  const h = new URLSearchParams()
  h.set('r', s.region.slug)
  h.set('c', s.mode)
  if (s.hsrOnly) h.set('hsr', '1')
  if (!s.construction) h.set('con', '0')
  if (s.proposed) h.set('prop', '1')
  if (s.freight) h.set('fr', '1')
  if (s.units === 'mi') h.set('u', 'mi')
  if (s.lang === 'en') h.set('lg', 'en')
  if (s.hidden.length) h.set('h', s.hidden.join(','))
  if (s.hiddenSpeed.length) h.set('hs', s.hiddenSpeed.join(','))
  if (s.inspect) h.set('ins', '1')
  if (!s.outline) h.set('ol', '0')
  if (s.theme === 'dark') h.set('th', 'dark')
  if (s.view) h.set('v', `${s.view.zoom.toFixed(2)}/${s.view.center[1].toFixed(4)}/${s.view.center[0].toFixed(4)}`)
  history.replaceState(null, '', '#' + h.toString())
}
