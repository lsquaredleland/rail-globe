// Checks the side-file interfaces in src/data.ts against shared/side-schema.json, which the Rust record structs generate
// (pipeline/rail-extract/src/tiles/records.rs, test `side_schema_is_current`). Every schema property must be on the interface with the same
// optionality (`?` is a key the pipeline may leave out), and every interface property must be in the schema.
import { expect, test } from 'vitest'
import ts from 'typescript'
import dataSource from './data.ts?raw'
import schema from '../../shared/side-schema.json'

type Def = { properties: Record<string, unknown>; required?: string[] }
const definitions: Record<string, Def> = schema.definitions

/** interface in src/data.ts -> definition in the schema */
const MAP: Record<string, string> = {
  StationRecord: 'station',
  LineRecord: 'line',
  LineIndexEntry: 'line_index_entry',
  NamedLine: 'named_line',
  Highlight: 'highlight',
  Shards: 'shards',
  StatsArea: 'stats_area',
  CnRecord: 'china_service_record',
}

/** `Interface.key` the Rust struct for the file does not hold, with the reason */
const EXCEPTIONS = new Set([
  // shards.json is written by the tiles side files; the China build adds "cn12306" to cn/shards.json afterwards (src/cn12306/sidefiles.rs)
  'Shards.cn12306',
])

const source = ts.createSourceFile('data.ts', dataSource, ts.ScriptTarget.Latest, true)
const interfaces = new Map<string, Map<string, boolean>>() // name -> key -> optional
source.forEachChild((n) => {
  if (!ts.isInterfaceDeclaration(n) || !(n.name.text in MAP)) return
  const keys = new Map<string, boolean>()
  for (const m of n.members) {
    if (!ts.isPropertySignature(m)) continue
    const name = ts.isStringLiteral(m.name) || ts.isIdentifier(m.name) ? m.name.text : m.name.getText(source)
    keys.set(name, !!m.questionToken)
  }
  interfaces.set(n.name.text, keys)
})

test('every mapped interface is found in data.ts', () => {
  expect([...interfaces.keys()].sort()).toEqual(Object.keys(MAP).sort())
})

test.each(Object.entries(MAP))('%s matches the %s schema', (name, def) => {
  const keys = interfaces.get(name)!
  const d = definitions[def]
  const required = new Set(d.required ?? [])
  const problems: string[] = []
  for (const k of Object.keys(d.properties)) {
    if (EXCEPTIONS.has(`${name}.${k}`)) continue
    if (!keys.has(k)) problems.push(`${name} lacks "${k}", which the pipeline writes`)
    else if (keys.get(k) === required.has(k))
      problems.push(`${name}.${k} is ${keys.get(k) ? 'optional' : 'required'} but the pipeline ${required.has(k) ? 'always writes' : 'may leave out'} it`)
  }
  for (const k of keys.keys()) if (!(k in d.properties) && !EXCEPTIONS.has(`${name}.${k}`)) problems.push(`${name}.${k} is not in the schema`)
  expect(problems).toEqual([])
})
