// Label coverage: share of label strings (per country) with at least one code point the glyph folder cannot draw.
//   node coverage.mjs --labels labels.json --old ../../app/public/glyphs --new ../../app/public/glyphs-v2 [--top 20]
// labels.json is written by `build-glyphs.mjs --dump labels.json` (label strings per country; the country is the smallest search.json bbox holding the tile centre, z>=6 tiles).
import fs from 'node:fs'
import path from 'node:path'
import Pbf from 'pbf'

const arg = (n, d) => { const i = process.argv.indexOf(n); return i > 0 ? process.argv[i + 1] : d }
const labels = JSON.parse(fs.readFileSync(arg('--labels'), 'utf8'))
const TOP = Number(arg('--top', 20))

function glyphIds(dir) {
  const ids = new Set()
  for (const stack of fs.readdirSync(dir)) {
    for (const f of fs.readdirSync(path.join(dir, stack))) {
      new Pbf(fs.readFileSync(path.join(dir, stack, f))).readFields((tag, _, pbf) => {
        if (tag === 1) pbf.readMessage((t, __, p) => { if (t === 3) p.readMessage((tt, ___, q) => { if (tt === 1) ids.add(q.readVarint()) }, {}) }, {})
      }, {})
    }
  }
  return ids
}
const sets = { old: glyphIds(path.resolve(arg('--old'))), new: glyphIds(path.resolve(arg('--new'))) }
// code points MapLibre draws with the system font (localIdeographFontFamily), same set as LOCAL_DRAWN in build-glyphs.mjs: counted as drawn for the new folder
const LOCAL = /[\u02EA\u02EB\u1100-\u11FF\u2E80-\u2FDF\u3000-\u30FF\u3105-\u312F\u3131-\u318E\u31A0-\u4DBF\u4E00-\uA48C\uA490-\uA4C6\uA960-\uA97C\uAC00-\uD7C6\uD7CB-\uD7FB\uF900-\uFA6D\uFA70-\uFAD9\uFE10-\uFE1F\uFE30-\uFE4F\uFF00-\uFFEF]/
const ignore = (c) => c < 0x20 || c === 0xfe0f || c === 0x200d || c === 0x200b || c === 0x200e || c === 0x200f
const rows = []
const lost = { old: new Map(), new: new Map() }
const total = { n: 0, old: 0, new: 0 }
for (const [cc, strs] of Object.entries(labels)) {
  const r = { cc, n: strs.length, old: 0, new: 0 }
  for (const s of strs) {
    for (const k of ['old', 'new']) {
      let bad = false
      for (const ch of s) { const c = ch.codePointAt(0); if (!ignore(c) && !sets[k].has(c) && !(k === 'new' && LOCAL.test(ch))) { bad = true; lost[k].set(c, (lost[k].get(c) || 0) + 1) } }
      if (bad) r[k]++
    }
  }
  total.n += r.n; total.old += r.old; total.new += r.new
  rows.push(r)
}
const pct = (a, n) => (n ? (100 * a / n).toFixed(1) : '0.0') + '%'
rows.sort((a, b) => b.old / b.n - a.old / a.n)
console.log(`country  labels   missing-before  missing-after`)
for (const r of rows.slice(0, TOP)) console.log(`${r.cc.padEnd(8)} ${String(r.n).padStart(7)}  ${pct(r.old, r.n).padStart(8)}  ${pct(r.new, r.n).padStart(8)}`)
console.log(`ALL      ${String(total.n).padStart(7)}  ${pct(total.old, total.n).padStart(8)}  ${pct(total.new, total.n).padStart(8)}`)
const worstAfter = [...rows].sort((a, b) => b.new / b.n - a.new / a.n).slice(0, 10).map((r) => `${r.cc} ${pct(r.new, r.n)}`)
console.log('worst after:', worstAfter.join(', '))
const hex = (c) => 'U+' + c.toString(16).toUpperCase().padStart(4, '0')
console.log(`remaining unrenderable code points: ${lost.new.size} (strings affected per code point, top 80)`)
console.log([...lost.new].sort((a, b) => b[1] - a[1]).slice(0, 80).map(([c, n]) => `${hex(c)} ${String.fromCodePoint(c)} ${n}`).join('\n'))
