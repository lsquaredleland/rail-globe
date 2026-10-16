// One-time pull of railway operator logos: operator Wikidata ids -> logo file on Wikimedia Commons -> small PNGs in app/public/logos/.
// Input: operators.json, {"Q123": {"st": n, "ln": n, "name": "..."}}, from the osmium step in README.md (this folder).
// Output: app/public/logos/<qid>.png (96 px wide at most) and app/public/logos/logos.json {qid: {name, file, st, ln}}.
// No licence check; coverage is whatever Wikidata has (P8972 small logo, else P154 logo).
// Usage: node pipeline/logos/pull-logos.mjs <operators.json> [--out app/public/logos]
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs'
import { join } from 'node:path'

const args = process.argv.slice(2)
const input = args.find((a) => !a.startsWith('--'))
const out = args.includes('--out') ? args[args.indexOf('--out') + 1] : 'app/public/logos'
if (!input) { console.error('usage: node pull-logos.mjs <operators.json> [--out dir]'); process.exit(1) }
const UA = 'RailGlobe/0.1 (one-time operator logo pull; lelandlee4@gmail.com)'
const ops = JSON.parse(readFileSync(input, 'utf8'))
mkdirSync(out, { recursive: true })

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
async function get(url, tries = 5) {
  for (let i = 0; i < tries; i++) {
    const r = await fetch(url, { headers: { 'User-Agent': UA } }).catch(() => null)
    if (r && r.ok) return r
    if (r && r.status !== 429 && r.status < 500) return r
    await sleep(1500 * (i + 1))
  }
  return null
}

// 1. the logo file name of each operator
const ids = Object.keys(ops)
const file = {}
for (let i = 0; i < ids.length; i += 50) {
  const batch = ids.slice(i, i + 50)
  const r = await get(`https://www.wikidata.org/w/api.php?action=wbgetentities&format=json&props=claims&ids=${batch.join('|')}`)
  const j = r ? await r.json() : { entities: {} }
  for (const [q, e] of Object.entries(j.entities ?? {})) {
    for (const p of ['P8972', 'P154']) {
      const cl = (e.claims?.[p] ?? []).filter((c) => c.rank !== 'deprecated' && c.mainsnak?.datavalue?.value)
      const best = cl.find((c) => c.rank === 'preferred') ?? cl[0]
      if (best) { file[q] = best.mainsnak.datavalue.value; break }
    }
  }
  console.error(`wikidata ${Math.min(i + 50, ids.length)}/${ids.length}, ${Object.keys(file).length} with a logo`)
  await sleep(300)
}

// 2. download each one as a PNG (Commons renders SVG to PNG at the requested width)
const manifest = {}
const todo = Object.keys(file)
let done = 0, failed = 0
async function worker() {
  while (todo.length) {
    const q = todo.pop()
    const dest = join(out, `${q}.png`)
    let ok = existsSync(dest)
    if (!ok) {
      const r = await get(`https://commons.wikimedia.org/wiki/Special:FilePath/${encodeURIComponent(file[q])}?width=96`)
      const ct = r?.headers.get('content-type') ?? ''
      if (r?.ok && ct.startsWith('image/') && ct !== 'image/svg+xml') {
        writeFileSync(dest, Buffer.from(await r.arrayBuffer())); ok = true
      }
    }
    if (ok) manifest[q] = { name: ops[q].name, file: file[q], st: ops[q].st, ln: ops[q].ln }
    else failed++
    if (++done % 100 === 0) console.error(`logos ${done}/${Object.keys(file).length} (${failed} failed)`)
    await sleep(150)
  }
}
await Promise.all([worker(), worker(), worker(), worker()])
writeFileSync(join(out, 'logos.json'), JSON.stringify(manifest))
console.error(`${ids.length} operators, ${Object.keys(file).length} with a Wikidata logo, ${Object.keys(manifest).length} downloaded, ${failed} failed`)
