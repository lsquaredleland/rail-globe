// Checks that the rail style only reads tile layers and properties the pipeline writes (shared/tile-schema.json).
// Covered: every layer addRailSource adds (filter, paint, layout), and every filter, paint and layout expression that applyState (several
// states: colour modes, hidden types and speed bands, HSR only, languages, themes), highlightLine, setFocus and setStationFocus set afterwards.
// Not covered: TypeScript reads of feature properties in components (hover and click handlers, tooltips, the selection code); those are
// plain `properties.x` reads the style code never shows to this mock.
import { expect, test, vi } from 'vitest'
import type { Map as MLMap } from 'maplibre-gl'
import schema from '../../shared/tile-schema.json'
import { addRailSource, applyState, highlightLine, setFocus, setStationFocus } from './mapStyle'
import type { MapState } from './mapStyle/shared'

vi.mock('./data', async (orig) => ({
  ...(await orig<typeof import('./data')>()),
  loadLineHighlight: async () => ({ ways: ['way/1'], geom: null, type: 'intercity', oc: '#123456' }),
  loadNameHighlight: async () => ({ ways: ['way/2'], geom: null, type: 'regional' }),
}))

type Layer = { id: string; source?: string; 'source-layer'?: string; filter?: unknown; paint?: Record<string, unknown>; layout?: Record<string, unknown> }
/** Reads of a property the tile layer lacks that are known and harmless: the label filter's HSR only test `hsr == 1` falls back to `ty` on `complexes`. */
const KNOWN_MISSING = new Set(['complexes.hsr'])
type Setting = { layer: string; what: string; keys: Set<string> }

const layers: Record<string, string[]> = schema.layers

/** Every property key an expression reads: ['get', k], ['has', k] */
function readKeys(x: unknown, out = new Set<string>()): Set<string> {
  if (!Array.isArray(x)) return out
  if ((x[0] === 'get' || x[0] === 'has') && typeof x[1] === 'string' && x.length === 2) out.add(x[1])
  for (const e of x) readKeys(e, out)
  return out
}

/** A recording map: the layers addRailSource adds, and every filter / paint / layout value set on them later. */
function recordingMap() {
  const sources: Record<string, string> = {}
  const added = new Map<string, Layer>()
  const later: Setting[] = []
  const record = (layer: string, what: string, v: unknown) => later.push({ layer, what, keys: readKeys(v) })
  const map = {
    addSource: (id: string, spec: { type: string }) => (sources[id] = spec.type),
    addLayer: (l: Layer) => added.set(l.id, l),
    getLayer: (id: string) => added.get(id),
    getSource: () => ({ setData: () => undefined }),
    getPaintProperty: (id: string, p: string) => added.get(id)?.paint?.[p],
    getZoom: () => 10,
    isMoving: () => false,
    queryRenderedFeatures: () => [],
    setFilter: (id: string, f: unknown) => record(id, 'filter', f),
    setPaintProperty: (id: string, p: string, v: unknown) => record(id, `paint ${p}`, v),
    setLayoutProperty: (id: string, p: string, v: unknown) => record(id, `layout ${p}`, v),
  }
  addRailSource(map as unknown as MLMap, 'world')
  return { map: map as unknown as MLMap, added, later, sources }
}

/** The vector layers the style builder added, with their tile source layer. */
function railLayers(): Layer[] {
  const { added, sources } = recordingMap()
  return [...added.values()].filter((l) => l.source && sources[l.source] === 'vector' && l['source-layer'])
}

const base: MapState = {
  mode: 'type',
  construction: true,
  proposed: false,
  freight: false,
  hidden: [],
  hiddenSpeed: [],
  hsrOnly: false,
  lang: 'local',
  theme: 'light',
}
const states: MapState[] = [
  base,
  { ...base, mode: 'operator', freight: true, proposed: true, lang: 'en' },
  { ...base, mode: 'speed', hiddenSpeed: [-1, 300], theme: 'dark' },
  { ...base, mode: 'speed', hidden: ['metro', 'tram', 'light_rail', 'monorail', 'high_speed'], hiddenSpeed: [200] },
  { ...base, hsrOnly: true, hidden: ['regional', 'freight'], theme: 'dark' },
  { ...base, mode: 'operator', hsrOnly: true, hidden: ['intercity'], lang: 'en' },
]

test('rail layers only use tile layers and properties the pipeline writes', () => {
  const problems: string[] = []
  for (const l of railLayers()) {
    const sl = l['source-layer']!
    const props = layers[sl]
    if (!props) {
      problems.push(`layer ${l.id}: source-layer '${sl}' is not in shared/tile-schema.json`)
      continue
    }
    const keys = new Set<string>()
    for (const section of [l.filter, l.paint, l.layout]) {
      if (section && typeof section === 'object' && !Array.isArray(section)) for (const v of Object.values(section)) readKeys(v, keys)
      else readKeys(section, keys)
    }
    for (const k of keys) if (!props.includes(k)) problems.push(`layer ${l.id}: reads '${k}' but source-layer '${sl}' has only [${props.join(', ')}]`)
  }
  expect(problems).toEqual([])
})

test('filters and paint set at run time (state, highlight, focus) only read properties the pipeline writes', async () => {
  const { map, added, later } = recordingMap()
  later.length = 0
  for (const s of states) applyState(map, s)
  highlightLine(map, 'world', 5)
  await vi.waitFor(() => expect(later.some((x) => x.layer === 'world:hl-fs' && x.what === 'filter')).toBe(true))
  highlightLine(map, 'world', null, 'Some Railway')
  highlightLine(map, 'world', null)
  setFocus(map, 'world')
  setStationFocus(map, ['node/1', 'node/2'])
  setStationFocus(map, null)

  expect(later.length).toBeGreaterThan(200) // the mock really saw applyState and the others
  const problems = new Set<string>()
  let checked = 0
  for (const { layer, what, keys } of later) {
    const sl = added.get(layer)?.['source-layer']
    if (!sl) continue // a layer with no tile source layer (the geojson highlight, basemap layers) has no tile properties
    checked++
    const props = layers[sl]
    if (!props) problems.add(`layer ${layer}: source-layer '${sl}' is not in shared/tile-schema.json`)
    else
      for (const k of keys)
        if (!props.includes(k) && !KNOWN_MISSING.has(`${sl}.${k}`))
          problems.add(`layer ${layer} (${what}): reads '${k}' but source-layer '${sl}' has only [${props.join(', ')}]`)
  }
  expect(checked).toBeGreaterThan(100)
  expect([...problems]).toEqual([])
})

test('the recording map found the rail layers', () => {
  const used = new Set(railLayers().map((l) => l['source-layer']))
  for (const sl of ['track', 'lines', 'stations', 'complexes', 'areas', 'airports', 'cities']) expect(used).toContain(sl)
})
