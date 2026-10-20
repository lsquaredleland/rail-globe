import { expect, test } from 'vitest'
import type { Map as MLMap } from 'maplibre-gl'
import schema from '../../shared/tile-schema.json'
import { addRailSource } from './mapStyle'

type Layer = { id: string; source?: string; 'source-layer'?: string; [k: string]: unknown }

const layers: Record<string, string[]> = schema.layers

/** Run the real rail style builder against a recording map: the vector layers it adds, with their tile source layer. */
function railLayers(): Layer[] {
  const sources: Record<string, string> = {}
  const added: Layer[] = []
  const map = {
    addSource: (id: string, spec: { type: string }) => (sources[id] = spec.type),
    addLayer: (l: Layer) => added.push(l),
  }
  addRailSource(map as unknown as MLMap, 'world')
  return added.filter((l) => l.source && sources[l.source] === 'vector' && l['source-layer'])
}

/** Every property key an expression reads: ['get', k], ['has', k] */
function readKeys(x: unknown, out = new Set<string>()): Set<string> {
  if (!Array.isArray(x)) return out
  if ((x[0] === 'get' || x[0] === 'has') && typeof x[1] === 'string' && x.length === 2) out.add(x[1])
  for (const e of x) readKeys(e, out)
  return out
}

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

test('the recording map found the rail layers', () => {
  const used = new Set(railLayers().map((l) => l['source-layer']))
  for (const sl of ['track', 'lines', 'stations', 'complexes', 'areas', 'airports', 'cities']) expect(used).toContain(sl)
})
