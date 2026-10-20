import type { Map as MLMap } from 'maplibre-gl'
import { TYPE_COLOR, TYPE_ORDER } from '../palette'
import { TILES } from '../regions'
import { L, byType, get, nameExpr, vis, type Expr, type MapState } from './shared'
import { SPEED_BANDS, SPEED_NONE, notHiddenSpeed, speedColor } from './speed'
import { INACTIVE, INACTIVE_DARK, INACTIVE_WIDTH, TWINS, activeStation, activeTrack, typeColor, widthExpr } from './trackStyle'
import { CON_BANDS, conColor, conWidth } from './construction'
import { PLACE_LAYERS, placeText } from './places'
import { applyTheme } from './theme'


/** Push toggles / colour mode into the rail layers. Cheap enough to call on every state change. */
export function applyState(map: MLMap, s: MapState) {
  if (map.getLayer(L(TILES, 'track'))) applyRegion(map, TILES, s)
}

function applyRegion(map: MLMap, slug: string, s: MapState) {
  const operator = s.mode === 'operator'
  {
    map.setLayoutProperty(L(slug, 'freight'), 'visibility', vis(s.freight))

    applyTheme(map, s.theme, slug)
    for (const id of PLACE_LAYERS) if (map.getLayer(id)) map.setLayoutProperty(id, 'text-field', placeText(s.lang))
    for (const n of ['lbl1', 'lbl2', 'lbl1t', 'lbl2t', 'lbl3', 'lblcon1', 'lblcon2', 'lblcon3']) map.setLayoutProperty(L(slug, n), 'text-field', nameExpr(s.lang, 'n', 'ne'))
    map.setLayoutProperty(L(slug, 'lbl-track'), 'text-field', nameExpr(s.lang, 'n', 'ne'))
    map.setLayoutProperty(L(slug, 'lbl-lines'), 'text-field', nameExpr(s.lang, 'lb', 'lbe'))
    // paint, layout and filter changes reach a layer and its high-speed twin (if it has one) together
    const names = (n: string) => (TWINS.has(n) ? [n, `${n}-hs`] : n === 'construction' || n === 'proposed' ? CON_BANDS.map((k) => `${n}-b${k}`) : [n])
    const setPaint = (n: string, prop: string, v: unknown) => names(n).forEach((x) => map.setPaintProperty(L(slug, x), prop as never, v as never))
    const setVis = (n: string, v: string) => names(n).forEach((x) => map.setLayoutProperty(L(slug, x), 'visibility', v as never))
    const setFilt = (n: string, f: Expr | null, typeKey = 'type') => {
      if (n === 'construction' || n === 'proposed') {
        CON_BANDS.forEach((k) => map.setFilter(L(slug, `${n}-b${k}`), ['all', ...(f ? [f] : []), ['==', get('b'), k]] as Expr))
        return
      }
      if (!TWINS.has(n)) { map.setFilter(L(slug, n), f); return }
      names(n).forEach((x, i) => {
        const hs: Expr = [i === 0 ? '!=' : '==', get(typeKey), 'high_speed'] as Expr
        map.setFilter(L(slug, x), (f ? ['all', hs, f] : hs) as Expr)
      })
    }
    // construction and proposed track are separate layers (flag `prop`); a switch flips their visibility
    setVis('construction', vis(s.construction)); setVis('proposed', vis(s.proposed))
    setVis('strand', vis(operator)); setVis('strand-casing', vis(operator)); setVis('badge', vis(operator))
    const has = ['has', 'oc'] as Expr
    const tun: Expr = ['step', ['zoom'], 1, 13, ['case', ['has', 'tun'], 0.45, 1]]
    // each colour mode sets its own colour and width; "HSR only" then greys and thins everything that is not high-speed, in any mode
    const greyOut = s.hsrOnly
    const grey = (c: Expr, active: Expr = activeTrack): Expr => (greyOut ? (['case', active, c, s.theme === 'dark' ? INACTIVE_DARK : INACTIVE] as Expr) : c)
    const thin = (m?: Expr): Expr | undefined => (greyOut ? (['*', m ?? 1, ['case', activeTrack, 1, INACTIVE_WIDTH]] as Expr) : m)
    const strandActive: Expr = ['==', get('t'), 'high_speed'] as Expr
    let color: Expr, width: Expr, opacity: Expr = tun
    if (operator) {
      // below z12 each track takes one colour; from z12 tracks with mapped lines give way to the strands
      color = ['coalesce', get('oc'), typeColor] as Expr
      width = widthExpr(thin())
      opacity = ['step', ['zoom'], ['case', has, 1, 0.6], 12, ['case', ['has', 'nl'], 0, 0.6]] as Expr
    } else if (s.mode === 'speed') {
      color = speedColor
      width = widthExpr(thin())
    } else {
      color = typeColor
      width = widthExpr(thin())
    }
    setPaint('track', 'line-color', grey(color))
    setPaint('track', 'line-width', width)
    setPaint('track', 'line-opacity', opacity)
    setPaint('hl', 'line-color', grey(color))
    setPaint('hl-fs', 'line-color', grey(color))
    // under-construction track follows the same colour rule as open track
    const conCol = grey(s.mode === 'speed' ? speedColor : conColor)
    const conW = conWidth(greyOut ? (['case', activeTrack, 1, INACTIVE_WIDTH] as Expr) : undefined)
    for (const n of ['construction', 'proposed']) { setPaint(n, 'line-color', conCol); setPaint(n, 'line-width', conW) }
    setPaint('strand', 'line-color', grey(['coalesce', get('c'), byType((t) => TYPE_COLOR[t], 't', TYPE_COLOR.other)] as Expr, strandActive))
    setPaint('strand-casing', 'line-opacity', greyOut ? (['case', strandActive, 0.28, 0] as Expr) : 0.28)
    const hiddenT = s.hidden.length > 0
    const notHiddenT = (key: string): Expr[] => (hiddenT ? [['!', ['in', get(key), ['literal', s.hidden]]] as Expr] : [])
    const noSpeed = notHiddenSpeed(s.hiddenSpeed)
    setFilt('track', ['all', ['!', ['has', 'con']], ['!=', get('type'), 'yard'], ['!=', get('type'), 'freight'], ...notHiddenT('type'), ...noSpeed] as Expr)
    const conFilt = [...(s.freight ? [] : [['!=', get('type'), 'freight'] as Expr]), ...notHiddenT('type'), ...noSpeed]
    setFilt('construction', ['all', ['!', ['has', 'prop']], ...conFilt] as Expr)
    setFilt('proposed', ['all', ['has', 'prop'], ...conFilt] as Expr)
    map.setLayoutProperty(L(slug, 'casing'), 'visibility', vis(!s.hidden.includes('high_speed')))
    map.setFilter(L(slug, 'casing'), ['all', ['==', get('type'), 'high_speed'], ['!', ['has', 'con']], ...noSpeed] as Expr)
    // city dots: a city shows while one of its systems (metro, tram) is not hidden; "HSR only" has no metro or tram to show
    const cityKinds: Expr[] = [...(s.hidden.includes('metro') ? [] : [['>', get('m'), 0]]), ...(s.hidden.includes('light_rail') ? [] : [['>', get('l'), 0]]), ...(s.hidden.includes('monorail') ? [] : [['>', get('r'), 0]])] as Expr[]
    map.setFilter(L(slug, 'cities'), cityKinds.length ? (['any', ...cityKinds] as Expr) : (['==', get('n'), -1] as Expr))
    map.setLayoutProperty(L(slug, 'cities'), 'visibility', vis(!greyOut))
    // a station disappears when every rail type that serves it is hidden (stations with no known type stay, unless every type is hidden)
    const ty: Expr = ['coalesce', get('ty'), ''] as Expr
    const served: Expr[] = hiddenT
      ? [['any', ...(TYPE_ORDER.every((t) => s.hidden.includes(t)) ? [] : [['==', ty, '']]), ...TYPE_ORDER.filter((t) => !s.hidden.includes(t)).map((t) => ['in', `,${t},`, ty])] as unknown as Expr]
      : []
    // speed bands hidden: a station stays while one of its bands (`sb`, ",index," list: SPEED_BANDS position, 9 = no maxspeed, like `ty`) is visible
    if (s.hiddenSpeed.length) {
      const vis9 = [...SPEED_BANDS.map((b) => b.min), SPEED_NONE].filter((m) => !s.hiddenSpeed.includes(m)).map((m) => (m === SPEED_NONE ? 9 : SPEED_BANDS.findIndex((b) => b.min === m)))
      const sb: Expr = ['coalesce', get('sb'), ',9,'] as Expr
      served.push((vis9.length ? ['any', ...vis9.map((i) => ['in', `,${i},`, sb])] : ['==', get('n'), -1]) as unknown as Expr)
    }
    // greyed-out mode: only stations that high-speed rail serves stay on the map
    if (greyOut) served.push(activeStation(ty))
    ;([['st1', 1], ['st2', 2], ['st3', 3], ['lbl1', 1], ['lbl2', 2], ['lbl1t', 1], ['lbl2t', 2], ['lbl3', 3]] as const).forEach(([n, tier]) =>
      map.getLayer(L(slug, n)) && map.setFilter(L(slug, n), ['all', ['==', get('tier'), tier], ['!', ['has', 'con']], ...served] as Expr))
    ;([['stcon1', 1], ['stcon2', 2], ['stcon3', 3], ['lblcon1', 1], ['lblcon2', 2], ['lblcon3', 3]] as const).forEach(([n, tier]) =>
      map.setFilter(L(slug, n), ['all', ['has', 'con'], ['==', get('tier'), tier], ...served] as Expr))
    map.setFilter(L(slug, 'logo'), ['all', ['has', 'lg'], ['<=', get('tier'), 2], ['!', ['has', 'con']], ...served] as Expr)
    for (const n of ['stcon1', 'stcon2', 'stcon3', 'lblcon1', 'lblcon2', 'lblcon3']) map.setLayoutProperty(L(slug, n), 'visibility', vis(s.construction))
    for (const n of ['strand', 'strand-casing']) setFilt(n, hiddenT ? (['all', ...notHiddenT('t')] as Expr) : null, 't')
    map.setFilter(L(slug, 'badge'), ['all', ['has', 'rf'], ...(greyOut ? [strandActive] : []), ...notHiddenT('t')] as Expr)
    map.setFilter(L(slug, 'lbl-lines'), hiddenT || greyOut ? (['all', ...(greyOut ? [strandActive] : []), ...notHiddenT('t')] as Expr) : null)
    map.setFilter(L(slug, 'lbl-track'), ['all', ...(greyOut ? [activeTrack] : []), ['has', 'n'], ['!', ['has', 'nl']], ['!', ['has', 'con']], ['!=', get('type'), 'yard'], ['!=', get('type'), 'freight'], ...notHiddenT('type'), ...noSpeed] as Expr)
    const hide: Expr[] = [...notHiddenT('type')]
    if (!s.freight) hide.push(['!=', get('type'), 'freight'] as Expr)
    if (greyOut) hide.push(activeTrack)  // greyed-out track cannot be hovered or selected
    const hit = (kind: Expr, extra: Expr[]) => ['all', ['!=', get('type'), 'yard'], kind, ...hide, ...extra] as Expr
    map.setFilter(L(slug, 'hit'), hit(['!', ['has', 'con']] as Expr, noSpeed))
    map.setFilter(L(slug, 'hit-con'), hit(['all', ['has', 'con'], ['!', ['has', 'prop']]] as Expr, noSpeed))
    map.setFilter(L(slug, 'hit-prop'), hit(['has', 'prop'] as Expr, noSpeed))
    map.setLayoutProperty(L(slug, 'hit-con'), 'visibility', vis(s.construction))
    map.setLayoutProperty(L(slug, 'hit-prop'), 'visibility', vis(s.proposed))
  }
}
