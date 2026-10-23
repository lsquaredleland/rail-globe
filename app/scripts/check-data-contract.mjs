// node scripts/check-data-contract.mjs [data-dir]
// Reads one station, one line record and stats.json of one country in public/data/world and checks their keys against the TS interfaces
// in src/data.ts (StationRecord, LineRecord, LineIndexEntry, StatsArea). The key lists below are kept by hand: change them with the interfaces.
// Exit 0 with a note when the data is absent; exit 1 on a missing required key or a key the app does not know.
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = process.argv[2] ?? join(dirname(fileURLToPath(import.meta.url)), '..', 'public', 'data', 'world')
if (!existsSync(root)) {
  console.log(`check-data-contract: no data at ${root}, nothing to check`)
  process.exit(0)
}

/** [required, optional] keys per record kind */
const CONTRACT = {
  station: [
    [
      'id',
      'name',
      'name_en',
      'tier',
      'terminus',
      'hsr',
      'modes',
      'platforms',
      'platform_count',
      'platform_refs',
      'platform_refs_other',
      'line_count',
      'lines',
      'complex',
      'osm',
      'wikidata',
      'pt',
      'tags',
      'model',
    ],
    ['construction', 'rail_lines', 'airport', 'logo'],
  ],
  line: [
    ['id', 'name', 'tags', 'route_ids', 'ref', 'type', 'colour', 'operator', 'network', 'from', 'to', 'km', 'osm', 'routes', 'station_names'],
    ['name_en', 'hl'],
  ],
  'line index entry': [['n', 'r', 'c', 't', 'o', 'f', 'to', 'km'], ['ne']],
  // China only (public/data/world/cn/cn12306/<rev>/<n>.json): a station's 12306 services. src/data.ts has no interface for it yet; these lists are the contract
  'china service record': [['name', 'name_en', 'stops', 'dep', 'arr', 'first', 'last', 'hd', 'ha', 'dest', 'dst', 'st', 'pts'], []],
  'stats area': [
    ['slug', 'name', 'local', 'kind', 'bbox', 'km', 'lines', 'stations'],
    ['speed', 'speed_con', 'speed_prop', 'hs_min_kmh'],
  ],
}

const problems = []
const notes = []
function check(kind, rec, where) {
  const [req, opt] = CONTRACT[kind]
  for (const k of req) if (!(k in rec)) problems.push(`${where}: ${kind} lacks "${k}"`)
  for (const k of Object.keys(rec)) if (!req.includes(k) && !opt.includes(k)) problems.push(`${where}: ${kind} has "${k}", which data.ts does not know`)
}
const read = (p) => JSON.parse(readFileSync(p, 'utf8'))
const first = (o) => o[Object.keys(o)[0]]

const slugs = readdirSync(root, { withFileTypes: true })
  .filter((d) => d.isDirectory() && existsSync(join(root, d.name, 'shards.json')))
  .map((d) => d.name)
if (!slugs.length) {
  console.log(`check-data-contract: no country with side files in ${root}, nothing to check`)
  process.exit(0)
}
const slug = slugs.includes('de') ? 'de' : slugs[0]
const dir = join(root, slug)

check('station', first(read(join(dir, 'stations', '0.json'))), `${slug}/stations/0.json`)
check('line', first(read(join(dir, 'lines', '0.json'))), `${slug}/lines/0.json`)
check('line index entry', first(read(join(dir, 'lines-index.json'))), `${slug}/lines-index.json`)
const { areas } = read(join(dir, 'stats.json'))
check('stats area', areas.find((a) => a.kind === 'region') ?? areas[0], `${slug}/stats.json (region row)`)
if (areas.find((a) => a.kind === 'region')?.hs_min_kmh === undefined)
  notes.push('stats.json region row has no hs_min_kmh yet (the app falls back to 250 for China, 200 elsewhere)')

const cn = join(root, 'cn')
if (existsSync(join(cn, 'cn12306'))) {
  // cn/shards.json "cn12306": { rev, count, snapshot: [{ date, classes }], cells }; everything of one revision is under cn12306/<rev>/
  const sh = read(join(cn, 'shards.json')).cn12306
  const day = (d) => d && typeof d.date === 'string' && Array.isArray(d.classes) && d.classes.every((c) => typeof c === 'string')
  if (
    !sh ||
    typeof sh.rev !== 'string' ||
    !Number.isInteger(sh.count) ||
    !Array.isArray(sh.snapshot) ||
    !sh.snapshot.length ||
    !sh.snapshot.every(day) ||
    !Array.isArray(sh.cells)
  )
    problems.push('cn/shards.json "cn12306" lacks rev, count, snapshot ([{ date, classes }]) or cells')
  else {
    const rd = join(cn, 'cn12306', sh.rev)
    const rec = first(read(join(rd, '0.json')) ?? {}) ?? first(read(join(rd, '1.json')))
    if (rec) check('china service record', rec, `cn/cn12306/${sh.rev}/0.json`)
    for (const f of ['stretches.json', 'ids.json', `${sh.count - 1}.json`]) if (!existsSync(join(rd, f))) problems.push(`cn/cn12306/${sh.rev}/${f} is missing`)
    for (const c of sh.cells.slice(0, 3)) if (!existsSync(join(rd, 'ways', `${c}.json`))) problems.push(`cn/cn12306/${sh.rev}/ways/${c}.json is missing`)
    const st = existsSync(join(rd, 'stretches.json')) ? read(join(rd, 'stretches.json')) : null
    if (st && (typeof st.q !== 'number' || !Array.isArray(st.s) || !('t' in st.s[0]) || !('c' in st.s[0])))
      problems.push('stretches.json is not { q, s: [{ t, c }] }')
    notes.push(`cn/cn12306 (revision ${sh.rev}, snapshot ${sh.snapshot.map((d) => d.date).join(' + ')}) matches the contract above`)
  }
}

for (const n of notes) console.log(`note: ${n}`)
if (problems.length) {
  for (const p of problems) console.error(`FAIL ${p}`)
  process.exit(1)
}
console.log(`check-data-contract: ${slug} matches the interfaces in src/data.ts`)
