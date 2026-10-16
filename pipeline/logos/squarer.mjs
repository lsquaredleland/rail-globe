// Second pass over app/public/logos: a wide wordmark ("Long Island Rail Road" next to the MTA mark) shrinks to nothing on a station chip.
// For each logo wider than 2:1, look for a squarer one among the operator's other logo claims (P8972 small logo, P154 logo, P2910 icon,
// P8392 logo with text variants) and, for a station operator, its parent's (P749, P361) when the parent is also in logos.json; keep the squarest if it is clearly squarer (ratio below 1.8).
// Usage: node pipeline/logos/squarer.mjs [--dir app/public/logos]
import { readFileSync, writeFileSync, existsSync } from 'node:fs'
import { join } from 'node:path'

const dir = process.argv.includes('--dir') ? process.argv[process.argv.indexOf('--dir') + 1] : 'app/public/logos'
const UA = 'RailGlobe/0.1 (one-time operator logo pull; lelandlee4@gmail.com)'
const manifest = JSON.parse(readFileSync(join(dir, 'logos.json'), 'utf8'))
const PARENTS = new Set(['Q1146109' /* MTA */, 'Q9322' /* Deutsche Bahn */, 'Q682520' /* Transport for London */])
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const png = (b) => [b.readUInt32BE(16), b.readUInt32BE(20)]
const ratio = (b) => { const [w, h] = png(b); return w / h }
async function get(url, tries = 5) {
  for (let i = 0; i < tries; i++) {
    const r = await fetch(url, { headers: { 'User-Agent': UA } }).catch(() => null)
    if (r && r.ok) return r
    if (r && r.status !== 429 && r.status < 500) return r
    await sleep(1500 * (i + 1))
  }
  return null
}
async function entity(q) {
  const r = await get(`https://www.wikidata.org/w/api.php?action=wbgetentities&format=json&props=claims&ids=${q}`)
  return (r ? await r.json() : {}).entities?.[q]?.claims ?? {}
}
// SVG or PNG only: a JPEG is usually a photo
const files = (claims) => ['P8972', 'P154', 'P2910'].flatMap((p) => (claims[p] ?? []).filter((c) => c.rank !== 'deprecated').map((c) => c.mainsnak?.datavalue?.value).filter((f) => f && /\.(svg|png)$/i.test(f)))
async function fetchPng(name) {
  const r = await get(`https://commons.wikimedia.org/wiki/Special:FilePath/${encodeURIComponent(name)}?width=96`)
  const ct = r?.headers.get('content-type') ?? ''
  return r?.ok && ct.startsWith('image/') && ct !== 'image/svg+xml' ? Buffer.from(await r.arrayBuffer()) : null
}

const wide = Object.keys(manifest).filter((q) => existsSync(join(dir, `${q}.png`)) && ratio(readFileSync(join(dir, `${q}.png`))) > 2)
console.error(`${wide.length} logos wider than 2:1`)
let swapped = 0
for (const q of wide) {
  const claims = await entity(q)
  const cand = new Set(files(claims))
  for (const p of ['P749', 'P361']) {
    for (const c of claims[p] ?? []) {
      const pq = c.mainsnak?.datavalue?.value?.id
      // a parent's logo only for the big multi-brand groups, whose subsidiaries (LIRR, Metro-North, DB Netz, S-Bahn Hamburg) show the group's mark:
      // other parents turned out to be ministries, banks or rival railways
      if (pq && PARENTS.has(pq) && manifest[pq]) for (const f of files(await entity(pq))) cand.add(f)
    }
  }
  cand.delete(manifest[q].file)
  let best = null, bestR = ratio(readFileSync(join(dir, `${q}.png`))), bestName = null
  for (const name of cand) {
    const b = await fetchPng(name)
    if (b && ratio(b) > 0.4 && ratio(b) < bestR) { best = b; bestR = ratio(b); bestName = name }
    await sleep(150)
  }
  if (best && bestR < 1.8) {
    writeFileSync(join(dir, `${q}.png`), best)
    manifest[q] = { ...manifest[q], file: bestName }
    swapped++
    console.error(`${q}: ${manifest[q].name} -> ${bestName} (${bestR.toFixed(2)}:1)`)
  }
}
writeFileSync(join(dir, 'logos.json'), JSON.stringify(manifest))
console.error(`${swapped} of ${wide.length} wide logos replaced`)
