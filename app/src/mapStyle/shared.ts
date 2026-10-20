import type { ExpressionSpecification, LayerSpecification, Map as MLMap } from 'maplibre-gl'
import { TYPE_ORDER, type RailType, type Theme } from '../palette'

// No `glyphs` URL in the style: MapLibre draws all text locally from system fonts. The first family carries the weight (it is sniffed from the name).
export const BOLD_STACK = ['Noto Sans Bold', 'Noto Sans', 'Helvetica Neue', 'Arial', 'sans-serif']
export const REG_STACK = ['Noto Sans Regular', 'Noto Sans', 'Helvetica Neue', 'Arial', 'sans-serif']

export type ColorMode = 'type' | 'operator' | 'speed'
export interface MapState {
  mode: ColorMode
  construction: boolean
  proposed: boolean
  freight: boolean
  hidden: string[]
  hiddenSpeed: number[]
  hsrOnly: boolean
  lang: 'local' | 'en'
  theme: Theme
}

export type Expr = ExpressionSpecification
export const get = (k: string): Expr => ['get', k]
export const ALL_TYPES = [...TYPE_ORDER, 'freight', 'yard'] as RailType[]

/** match on the track `type` property */
export function byType(f: (t: RailType) => number | string, key = 'type', fallback: number | string = 0): Expr {
  const out: unknown[] = ['match', get(key)]
  for (const t of ALL_TYPES) out.push(t, f(t))
  out.push(fallback)
  return out as Expr
}

/** label text: the English field when the language is English and the tile has one, else the local field */
export const nameExpr = (lang: 'local' | 'en', local: string, en: string): Expr =>
  (lang === 'en' ? ['case', ['all', ['has', en], ['!=', get(en), '']], get(en), get(local)] : get(local)) as Expr

export const L = (slug: string, name: string) => `${slug}:${name}`
export const vis = (on: boolean) => (on ? 'visible' : 'none')
export const roundJoin = { 'line-cap': 'round', 'line-join': 'round' } as const

export const strandLayer = (slug: string) => L(slug, 'strand')
export const hitLayers = (slug: string) => ['hit', 'hit-con', 'hit-prop'].map((n) => L(slug, n))
export const yardHitLayer = (slug: string) => L(slug, 'hit-yard')

/** What the rail layer builders (addRailSource) share: the map, the tile source, and the layers that are added after the others */
export interface RailCtx {
  map: MLMap
  source: string
  add: (l: Record<string, unknown>) => void
  src: { readonly source: string }
  /** high-speed twins and the casing: added above every other rail layer */
  top: LayerSpecification[]
  /** highlight layers: added last (after the veil) */
  late: LayerSpecification[]
  trackLayer: (name: string, spec: Record<string, unknown>, typeKey?: string) => void
  hlLayer: (spec: Record<string, unknown>) => void
}
