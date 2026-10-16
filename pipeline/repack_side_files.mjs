// Converts side files written before 2026-10-16 (one file per station and per line, 256 tag and name-hl shards) into the shard layout
// that pipeline/rail-extract/src/tiles.rs writes now, without a rebuild: stations/, lines/, tags/ and name-hl/ become a few hash shards
// plus shards.json (app/src/data.ts reads it). Same rules as tiles.rs: key hashed with fnv1a, count a power of two for ~256 KB a shard,
// lines over 64 KB stay their own file in lines-big/. Usage: node pipeline/repack_side_files.mjs <data root> [--dry-run]
// Walks <data root> for region folders (a folder holding stations/ and no shards.json), so it covers world/<slug> too. Safe to re-run.
import fs from 'node:fs'
import path from 'node:path'

const [root, flag] = process.argv.slice(2)
if (!root) { console.error('usage: node pipeline/repack_side_files.mjs <data root> [--dry-run]'); process.exit(1) }
const dry = flag === '--dry-run'
const TARGET = 256 * 1024, BIG_LINE = 64 * 1024

const fnv1a = (s) => { let h = 0x811c9dc5; for (const b of Buffer.from(s, 'utf8')) h = Math.imul(h ^ b, 0x01000193) >>> 0; return h >>> 0 }
const shardCount = (bytes) => { let n = 1; while (n * TARGET < bytes && n < 1024) n *= 2; return n }
const rd = (p) => JSON.parse(fs.readFileSync(p, 'utf8'))
const files = (d) => fs.readdirSync(d).filter((f) => f.endsWith('.json'))

/** recs: [key, JSON text][]. Replaces dir's contents with shards; returns the count. */
function writeShards(dir, recs) {
  const n = shardCount(recs.reduce((a, r) => a + Buffer.byteLength(r[1]), 0))
  const out = Array.from({ length: n }, () => [])
  for (const [k, v] of recs) out[fnv1a(k) % n].push(`${JSON.stringify(k)}:${v}`)
  for (let i = 0; i < n; i++) fs.writeFileSync(path.join(dir, `${i}.json`), `{${out[i].join(',')}}`)
  return n
}
const clear = (dir, keep = () => false) => { for (const f of fs.readdirSync(dir)) if (!keep(f)) fs.rmSync(path.join(dir, f)) }

function repack(region) {
  const stations = [], lines = [], tags = [], hl = [], big = []
  for (const f of files(`${region}/stations`)) { const t = fs.readFileSync(`${region}/stations/${f}`, 'utf8'); stations.push([JSON.parse(t).id, t]) }
  for (const f of files(`${region}/lines`)) {
    const t = fs.readFileSync(`${region}/lines/${f}`, 'utf8'), id = f.replace('.json', '')
    if (Buffer.byteLength(t) > BIG_LINE) big.push(Number(id)); else lines.push([id, t])
  }
  for (const [kind, into] of [['tags', tags], ['name-hl', hl]]) for (const f of files(`${region}/${kind}`)) for (const [k, v] of Object.entries(rd(`${region}/${kind}/${f}`))) into.push([k, JSON.stringify(v)])
  if (dry) return { stations: stations.length, lines: lines.length + big.length, tags: tags.length, hl: hl.length }
  const m = { stations: 0, lines: 0, lines_big: big.sort((a, b) => a - b), tags: 0, 'name-hl': 0 }
  clear(`${region}/stations`); m.stations = writeShards(`${region}/stations`, stations)
  fs.mkdirSync(`${region}/lines-big`, { recursive: true })
  for (const id of big) fs.renameSync(`${region}/lines/${id}.json`, `${region}/lines-big/${id}.json`)
  clear(`${region}/lines`); m.lines = writeShards(`${region}/lines`, lines)
  clear(`${region}/tags`); m.tags = writeShards(`${region}/tags`, tags)
  clear(`${region}/name-hl`); m['name-hl'] = writeShards(`${region}/name-hl`, hl)
  fs.writeFileSync(`${region}/shards.json`, JSON.stringify(m))
  return m
}

function walk(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    if (!e.isDirectory()) continue
    const p = path.join(dir, e.name)
    if (fs.existsSync(`${p}/stations`) && fs.existsSync(`${p}/lines`)) {
      if (fs.existsSync(`${p}/shards.json`)) continue
      const before = fs.readdirSync(p, { recursive: true }).length
      const r = repack(p)
      console.log(dry ? 'would repack' : 'repacked', p, JSON.stringify(r), `(${before} entries before)`)
    } else if (!['stations', 'lines', 'tags', 'name-hl'].includes(e.name)) walk(p)
  }
}
walk(root)
