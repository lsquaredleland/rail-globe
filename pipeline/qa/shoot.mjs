// QA screenshots of map views. Usage:
//   node pipeline/qa/shoot.mjs <pass-number> [--only name,name] [--port 5173] [--no-freight] [--enc mvt|mlt (default: none)] [--keep-ui]
// Output: data/qa/pass-<N>/<name>.png (passenger only), <name>.fr.png (freight shown), index.json
import { createRequire } from 'node:module'
import { spawn } from 'node:child_process'
import { mkdirSync, writeFileSync, readFileSync } from 'node:fs'
import { connect } from 'node:net'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const root = resolve(here, '../..')
const puppeteer = createRequire(join(root, 'app/package.json'))('puppeteer-core')

const args = process.argv.slice(2)
const flag = (n) => args.indexOf('--' + n)
const val = (n, d) => (flag(n) >= 0 ? args[flag(n) + 1] : d)
const pass = args[0]
if (!pass || !/^\d+$/.test(pass)) { console.error('usage: shoot.mjs <pass-number> [--only a,b] [--port 5173] [--no-freight]'); process.exit(1) }
const port = Number(val('port', 5173))
const only = val('only', '')?.split(',').filter(Boolean)
const freight = flag('no-freight') < 0
const enc = val('enc', '')   // optional dev-only app switch (?enc=mvt|mlt) in location.search; unset by default (the app default is used)
if (enc && !['mvt', 'mlt'].includes(enc)) { console.error('--enc must be mvt or mlt'); process.exit(1) }
const keepUi = flag('keep-ui') >= 0
const CHROME = val('chrome', '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome')
const TIMEOUT = 40000
const outDir = join(root, 'data/qa', `pass-${pass}`)
mkdirSync(outDir, { recursive: true })

let views = JSON.parse(readFileSync(join(here, 'views.json'), 'utf8'))
if (only?.length) {
  const miss = only.filter((n) => !views.some((v) => v.name === n))
  if (miss.length) console.error('unknown views: ' + miss.join(', '))
  views = views.filter((v) => only.includes(v.name))
}

const listening = () => new Promise((res) => {
  const s = connect(port, '127.0.0.1'); s.once('connect', () => { s.destroy(); res(true) }); s.once('error', () => res(false))
})
let vite = null
if (!(await listening())) {
  vite = spawn('npx', ['vite', '--port', String(port), '--strictPort', '--host', '127.0.0.1'], { cwd: join(root, 'app'), stdio: 'ignore' })
  for (let i = 0; i < 60 && !(await listening()); i++) await new Promise((r) => setTimeout(r, 500))
  if (!(await listening())) { console.error('vite did not start'); vite.kill(); process.exit(1) }
  console.log('started vite on ' + port)
}

const browser = await puppeteer.launch({ executablePath: CHROME, headless: true, args: ['--no-sandbox', '--enable-unsafe-swiftshader', '--use-angle=swiftshader'] })
const index = { pass: Number(pass), date: new Date().toISOString(), viewport: '1600x1000', views: [] }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function shoot(v, fr) {
  const hash = `r=${v.region}&v=${v.zoom}/${v.lat}/${v.lon}${fr ? '&fr=1' : ''}&con=0`
  const url = `http://127.0.0.1:${port}/${enc ? `?enc=${enc}` : ''}#${hash}`
  const file = `${v.name}${fr ? '.fr' : ''}.png`
  const t0 = Date.now()
  const page = await browser.newPage()
  try {
    await page.setViewport({ width: 1600, height: 1000, deviceScaleFactor: 1 })
    // map errors (tile parse failures etc.) are collected per view; a view with errors is flagged in index.json, not failed
    await page.evaluateOnNewDocument(`window.__qaErrors = []; const t = setInterval(() => { if (window.__map) { clearInterval(t); window.__map.on('error', (e) => window.__qaErrors.push(String(e.error && e.error.message || e.error).slice(0, 200))) } }, 20)`)
    await page.goto(url, { waitUntil: 'load', timeout: TIMEOUT })
    const deadline = Date.now() + TIMEOUT
    // the app adds its rail sources/layers after the map 'load' event (parts file, then places.json); the 'selection' layer comes after the rail sources
    if (!keepUi) await page.addStyleTag({ content: '.left-stack, .zoom, .zoom-debug, .tooltip, .peek { display: none !important }' })   // toolbar + region panel + zoom readout; scale bar and attribution stay
    await page.waitForFunction("window.__map && window.__map.getLayer('selection') && window.__map.isStyleLoaded()", { timeout: TIMEOUT })
    // idle: tiles loaded and not moving, stable for two consecutive polls; settle for late label/source work
    let stable = 0
    while (stable < 3) {
      if (Date.now() > deadline) throw new Error('timeout waiting for idle')
      const ok = await page.evaluate('(() => { const m = window.__map; return !m.isMoving() && m.isStyleLoaded() && m.areTilesLoaded() && Object.entries(m.getStyle().sources).every(([id, s]) => s.type === "geojson" || m.isSourceLoaded(id)) })()')
      stable = ok ? stable + 1 : 0
      await sleep(400)
    }
    await sleep(800)
    const st = await page.evaluate('(() => { const m = window.__map, c = m.getCenter(); return { z: m.getZoom(), lat: c.lat, lon: c.lng } })()')
    await page.screenshot({ path: join(outDir, file) })
    const errors = await page.evaluate('window.__qaErrors')
    return { file, hash, ms: Date.now() - t0, actual: st, ...(errors.length ? { mapErrors: errors.length, firstMapError: errors[0] } : {}) }
  } catch (e) {
    return { file, hash, ms: Date.now() - t0, error: String(e.message ?? e) }
  } finally { await page.close().catch(() => {}) }
}

try {
  for (const v of views) {
    const entry = { ...v, shots: {} }
    for (const fr of freight ? [false, true] : [false]) {
      const r = await shoot(v, fr)
      entry.shots[fr ? 'freight' : 'passenger'] = r
      console.log(`${r.error ? 'FAIL' : 'ok  '} ${r.file} ${(r.ms / 1000).toFixed(1)}s${r.error ? ' ' + r.error : ''}${r.mapErrors ? ` [${r.mapErrors} map errors: ${r.firstMapError.slice(0, 90)}]` : ''}`)
    }
    index.views.push(entry)
    writeFileSync(join(outDir, 'index.json'), JSON.stringify(index, null, 1))
  }
} finally {
  await browser.close()
  if (vite) vite.kill()
}
const failed = index.views.flatMap((e) => Object.values(e.shots).filter((s) => s.error).map((s) => s.file))
const withErr = index.views.flatMap((e) => Object.values(e.shots).filter((s) => s.mapErrors).map((s) => s.file))
if (withErr.length) console.log(`map errors in ${withErr.length} shots: ${withErr.join(', ')}`)
process.exitCode = failed.length || withErr.length ? 1 : 0
if (withErr.length) console.error(`FAIL: tile-parse/map errors in ${withErr.length} shots`)
console.log(`done: ${index.views.length} views, ${failed.length} failures${failed.length ? ': ' + failed.join(', ') : ''}`)
