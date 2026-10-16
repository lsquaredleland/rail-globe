// Subset map glyphs: writes app/public/glyphs/<fontstack>/<start>-<end>.pbf holding ONLY the characters the map can draw.
//
//   cd pipeline/glyphs && npm install && node build-glyphs.mjs [--data ../../app/public/data] [--tiles pilot,americas] [--out ../../app/public/glyphs] [--allow-empty]
//   --out must end in a folder named 'glyphs' (it is deleted first). Without a tile source in --data it stops with an error;
//   --allow-empty builds ASCII + places.json only.
//   --tiles NAMES: the <name>.pmtiles files of --data to read (default pilot,americas). For the world build:
//       node build-glyphs.mjs --data ../../app/public/data/world --tiles world --out <dir>/glyphs     (reads world.pmtiles and world/places.json)
//
// 1. Character set: every code point in the label properties (n, ne, lb, lbe, rf) of every tile of the --tiles files,
//    the names in places.json (written by `rail-extract places`, which `build` and `world` run last), printable ASCII,
//    and the upper-case form of each (country names are drawn in capitals).
//    Rerun after any rebuild that adds regions or names; the tile glyph files are tied to the tile data.
// 2. Fonts, in priority order per code point: Noto Sans (Latin, Greek, Cyrillic), Noto Sans CJK SC (Han, kana, hangul, CJK and
//    fullwidth punctuation), then, for the few code points neither has (Arabic, Lao, Tibetan, Mongolian, Yi ...), the OpenFreeMap
//    glyph server's Noto Sans, which is what the map drew those with before. Fonts are downloaded once into fonts/ (sha256 checked).
// 3. fontnik renders each 256-code-point range with SDF; the result is filtered to the character set and merged.
import fs from 'node:fs'
import path from 'node:path'
import zlib from 'node:zlib'
import crypto from 'node:crypto'
import { fileURLToPath } from 'node:url'
import fontnik from 'fontnik'
import Pbf from 'pbf'
import { bytesToHeader } from 'pmtiles'
import { buildSync } from 'esbuild'

const here = path.dirname(fileURLToPath(import.meta.url))
const arg = (name, dflt) => { const i = process.argv.indexOf(name); return i > 0 ? path.resolve(process.argv[i + 1]) : dflt }
const DATA = arg('--data', path.resolve(here, '../../app/public/data'))
const OUT = arg('--out', path.resolve(here, '../../app/public/glyphs'))
const FONTS = path.join(here, 'fonts')
const TILES = (process.argv.includes('--tiles') ? process.argv[process.argv.indexOf('--tiles') + 1] : 'pilot,americas').split(',').filter(Boolean)
const ALLOW_EMPTY = process.argv.includes('--allow-empty')
if (path.basename(OUT) !== 'glyphs') { console.error(`refusing --out ${OUT}: its folder name must be 'glyphs' (the folder is deleted before writing)`); process.exit(2) }

// @maplibre/mlt is published for bundlers (extensionless relative imports), which Node cannot load: bundle it once into fonts/
fs.mkdirSync(FONTS, { recursive: true })
buildSync({ entryPoints: [path.join(here, 'node_modules/@maplibre/mlt/dist/index.js')], bundle: true, format: 'esm', platform: 'node', outfile: path.join(FONTS, 'mlt.mjs'), logLevel: 'error' })
const { decodeTile } = await import(path.join(FONTS, 'mlt.mjs'))

const NOTO = 'https://raw.githubusercontent.com/notofonts/notofonts.github.io/main/fonts/NotoSans/hinted/ttf'
const CJK = 'https://raw.githubusercontent.com/notofonts/noto-cjk/main/Sans/OTF/SimplifiedChinese'
const FONT_FILES = {
  'NotoSans-Regular.ttf': [`${NOTO}/NotoSans-Regular.ttf`, '478c558ea716033cd60c03438f628dfa75694dcf6b5f6d505a2f05fd2b4f3823'],
  'NotoSans-Bold.ttf': [`${NOTO}/NotoSans-Bold.ttf`, '1df075a380fc7cb898acf64c1f7b3b4dd780de3caa860178bf929de35817a913'],
  'NotoSansCJKsc-Regular.otf': [`${CJK}/NotoSansCJKsc-Regular.otf`, '2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b'],
  'NotoSansCJKsc-Bold.otf': [`${CJK}/NotoSansCJKsc-Bold.otf`, 'b5f0d1a190a7f9b43c310a8850630af12553df32c4c050543f9059732d9b4c0a'],
}
// the font stacks the style names (app/src/mapStyle.ts) -> font files, then the OpenFreeMap fallback of the same name
const STACKS = {
  'Noto Sans Regular': ['NotoSans-Regular.ttf', 'NotoSansCJKsc-Regular.otf'],
  'Noto Sans Bold': ['NotoSans-Bold.ttf', 'NotoSansCJKsc-Bold.otf'],
}
const OFM_FONTS = 'https://tiles.openfreemap.org/fonts'
const LABEL_KEYS = ['n', 'ne', 'lb', 'lbe', 'rf']

async function ensureFonts() {
  fs.mkdirSync(FONTS, { recursive: true })
  for (const [name, [url, sha]] of Object.entries(FONT_FILES)) {
    const file = path.join(FONTS, name)
    if (!fs.existsSync(file)) {
      console.error(`downloading ${name}`)
      const r = await fetch(url)
      if (!r.ok) throw new Error(`${url}: ${r.status}`)
      fs.writeFileSync(file, Buffer.from(await r.arrayBuffer()))
    }
    const got = crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')
    if (got !== sha) throw new Error(`${name}: sha256 ${got} is not the pinned ${sha}`)
  }
}

/** every directory entry of a PMTiles file, tile bytes decompressed */
function* pmtilesTiles(file) {
  const fd = fs.openSync(file, 'r')
  const rd = (off, len) => { const b = Buffer.alloc(len); fs.readSync(fd, b, 0, len, off); return b }
  const un = (b) => (b[0] === 0x1f && b[1] === 0x8b ? zlib.gunzipSync(b) : b)
  const h = bytesToHeader(new Uint8Array(rd(0, 127)).buffer)
  if (h.tileType !== 6) throw new Error(`${file}: not an MLT tile file (tile type ${h.tileType})`)
  const parse = (b) => {
    let p = 0
    const vi = () => { let r = 0, s = 1; for (;;) { const c = b[p++]; r += (c & 127) * s; if (c < 128) return r; s *= 128 } }
    const n = vi(), es = []
    for (let i = 0, id = 0; i < n; i++) { id += vi(); es.push({ id }) }
    for (let i = 0; i < n; i++) es[i].run = vi()
    for (let i = 0; i < n; i++) es[i].len = vi()
    for (let i = 0; i < n; i++) { const v = vi(); es[i].off = v === 0 && i > 0 ? es[i - 1].off + es[i - 1].len : v - 1 }
    return es
  }
  function* walk(off, len) {
    for (const e of parse(un(rd(off, len)))) {
      if (e.run > 0) yield un(rd(h.tileDataOffset + e.off, e.len))
      else yield* walk(h.leafDirectoryOffset + e.off, e.len)
    }
  }
  yield* walk(h.rootDirectoryOffset, h.rootDirectoryLength)
  fs.closeSync(fd)
}

function collectChars() {
  const cps = new Set()
  const add = (s) => { if (typeof s === 'string') for (const c of s) cps.add(c.codePointAt(0)) }
  for (let c = 0x20; c < 0x7f; c++) cps.add(c)
  const stats = {}
  for (const name of TILES) {
    const file = path.join(DATA, `${name}.pmtiles`)
    if (!fs.existsSync(file)) { console.error(`skip ${name}.pmtiles (missing)`); continue }
    let tiles = 0
    for (const buf of pmtilesTiles(file)) {
      for (const layer of decodeTile(new Uint8Array(buf))) {
        for (const k of LABEL_KEYS) {
          const v = layer.getPropertyVector(k)
          if (v) for (let i = 0; i < v.size; i++) add(v.getValue(i))
        }
      }
      tiles++
    }
    stats[name] = tiles
    console.error(`${name}.pmtiles: ${tiles} tiles, ${cps.size} code points so far`)
  }
  if (!Object.keys(stats).length && !ALLOW_EMPTY) {
    console.error(`no tile source found in ${DATA} (looked for ${TILES.map((t) => t + '.pmtiles').join(', ')}): build the data first, or pass --allow-empty for ASCII-only glyphs`)
    process.exit(1)
  }
  const places = path.join(DATA, 'places.json')
  if (fs.existsSync(places)) for (const r of JSON.parse(fs.readFileSync(places, 'utf8'))) { add(r[4]); add(r[5]) }
  for (const c of [...cps]) { const u = String.fromCodePoint(c).toUpperCase(); if ([...u].length === 1) cps.add(u.codePointAt(0)) }
  cps.delete(0xfe0f) // variation selector: never drawn as a glyph
  return cps
}

// glyph PBF (https://github.com/mapbox/glyph-pbf-composite/blob/master/glyphs.proto): stacks(1){ name 1, range 2, glyphs 3{ id 1, bitmap 2, width 3, height 4, left 5, top 6, advance 7 } }
function readGlyphs(buf) {
  const out = new Map()
  const readGlyph = (tag, g, pbf) => {
    if (tag === 1) g.id = pbf.readVarint()
    else if (tag === 2) g.bitmap = Buffer.from(pbf.readBytes())
    else if (tag === 3) g.width = pbf.readVarint()
    else if (tag === 4) g.height = pbf.readVarint()
    else if (tag === 5) g.left = pbf.readSVarint()
    else if (tag === 6) g.top = pbf.readSVarint()
    else if (tag === 7) g.advance = pbf.readVarint()
  }
  new Pbf(buf).readFields((tag, _, pbf) => {
    if (tag === 1) pbf.readMessage((t, __, p) => { if (t === 3) { const g = {}; p.readMessage(readGlyph, g); out.set(g.id, g) } }, {})
  }, {})
  return out
}
function writeGlyphs(name, range, glyphs) {
  const pbf = new Pbf()
  pbf.writeMessage(1, (_, p) => {
    p.writeStringField(1, name)
    p.writeStringField(2, range)
    for (const g of glyphs) {
      p.writeMessage(3, (gg, q) => {
        q.writeVarintField(1, gg.id)
        if (gg.bitmap) q.writeBytesField(2, gg.bitmap)
        q.writeVarintField(3, gg.width)
        q.writeVarintField(4, gg.height)
        q.writeSVarintField(5, gg.left)
        q.writeSVarintField(6, gg.top)
        q.writeVarintField(7, gg.advance)
      }, g)
    }
  }, null)
  return Buffer.from(pbf.finish())
}

const fontnikRange = (font, start, end) => new Promise((res, rej) => fontnik.range({ font, start, end }, (e, d) => (e ? rej(e) : res(d))))
async function ofmRange(stack, range) {
  const cache = path.join(FONTS, 'ofm', stack, `${range}.pbf`)
  if (!fs.existsSync(cache)) {
    const r = await fetch(`${OFM_FONTS}/${encodeURIComponent(stack)}/${range}.pbf`)
    if (!r.ok) return new Map()
    fs.mkdirSync(path.dirname(cache), { recursive: true })
    fs.writeFileSync(cache, Buffer.from(await r.arrayBuffer()))
  }
  return readGlyphs(fs.readFileSync(cache))
}

await ensureFonts()
const cps = collectChars()
const bmp = [...cps].filter((c) => c <= 0xffff).sort((a, b) => a - b)
const astral = [...cps].filter((c) => c > 0xffff)
console.error(`${cps.size} code points (${astral.length} above U+FFFF, not in 16-bit glyph ranges: skipped)`)

fs.rmSync(OUT, { recursive: true, force: true })
const ranges = [...new Set(bmp.map((c) => c >> 8))]
const report = {}
for (const [stack, files] of Object.entries(STACKS)) {
  const fonts = files.map((f) => fs.readFileSync(path.join(FONTS, f)))
  fs.mkdirSync(path.join(OUT, stack), { recursive: true })
  const src = { 'Noto Sans': 0, 'Noto Sans CJK SC': 0, OpenFreeMap: 0 }
  const missing = []
  let bytes = 0
  for (const r of ranges) {
    const start = r * 256, end = start + 255, name = `${start}-${end}`
    const need = new Set(bmp.filter((c) => c >> 8 === r))
    const got = new Map()
    const take = (map, label) => { for (const [id, g] of map) if (need.has(id) && !got.has(id)) { got.set(id, g); src[label]++ } }
    take(readGlyphs(await fontnikRange(fonts[0], start, end)), 'Noto Sans')
    if (got.size < need.size) take(readGlyphs(await fontnikRange(fonts[1], start, end)), 'Noto Sans CJK SC')
    if (got.size < need.size) take(await ofmRange(stack, name), 'OpenFreeMap')
    for (const id of need) if (!got.has(id)) missing.push(id)
    const buf = writeGlyphs(stack, name, [...got.values()].sort((a, b) => a.id - b.id))
    fs.writeFileSync(path.join(OUT, stack, `${name}.pbf`), buf)
    bytes += buf.length
  }
  report[stack] = { files: ranges.length, bytes, gzipBytes: 0, sources: src, missing: missing.map((c) => 'U+' + c.toString(16).toUpperCase().padStart(4, '0') + ' ' + String.fromCodePoint(c)) }
  for (const r of ranges) report[stack].gzipBytes += zlib.gzipSync(fs.readFileSync(path.join(OUT, stack, `${r * 256}-${r * 256 + 255}.pbf`))).length
}
console.log(JSON.stringify({ codePoints: cps.size, bmp: bmp.length, astralSkipped: astral.map((c) => String.fromCodePoint(c)), report }, null, 1))
