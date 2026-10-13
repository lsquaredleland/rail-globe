// Zoom/pan benchmark on the production build. Usage:
//   npm run build && npx vite preview --port 4173   (separate shell; or pass --url)
//   node bench/zoom-pan.mjs [--url http://localhost:4173] [--runs 3] [--throttle 6] [--warmup 1] [--label run] [--chrome <path>]
// Primary metric: missed frames = sum of max(0, round(dt / 16.667) - 1). Intervals are whole 60 Hz frames.
import puppeteer from 'puppeteer-core'
import { mkdirSync, writeFileSync } from 'node:fs'
import { execSync } from 'node:child_process'

const arg = (n, d) => { const i = process.argv.indexOf('--' + n); return i > 0 ? process.argv[i + 1] : d }
const num = (n, d, min) => {
  const v = Number(arg(n, d))
  if (!Number.isFinite(v) || v < min) { console.error(`--${n} must be a number >= ${min}`); process.exit(1) }
  return v
}
const URL_ = arg('url', 'http://localhost:4173')
const RUNS = num('runs', 3, 1)
const THROTTLE = num('throttle', 6, 1)
const WARMUP = num('warmup', 1, 0)
const LABEL = arg('label', 'run')
const CHROME = arg('chrome', '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome')
const VSYNC = 1000 / 60
const DROP_MS = 25   // 1.5 vsync: a frame longer than this is dropped (33.3 sits on the 2-vsync rounding boundary)
const VIEWPORT = { width: 1440, height: 900, deviceScaleFactor: 2 }
const VIEWS = [
  // China only: densest network, and every view draws from the one china.pmtiles
  ['Chengdu East', 104.14, 30.63], ['Shenzhen', 114.06, 22.58], ['Guangzhou', 113.27, 23.13], ['Beijing', 116.40, 39.90], ['Shanghai', 121.47, 31.23],
]
// fixed durations so every run does identical work; essential ignores prefers-reduced-motion
const MOVES = [
  ['zoomIn', (m, c) => m.flyTo({ center: c, zoom: 14, duration: 2500, essential: true })],
  ['pan', (m) => m.panBy([1440, 0], { duration: 1500, essential: true })],
  ['zoomOut', (m) => m.easeTo({ zoom: 8, duration: 2500, essential: true })],
]
const MV = MOVES.map(([k]) => k)

const median = (a) => { if (!a.length) return 0; const s = [...a].sort((x, y) => x - y); const n = s.length; return n % 2 ? s[(n - 1) / 2] : (s[n / 2 - 1] + s[n / 2]) / 2 }
const r1 = (x) => Math.round(x * 10) / 10
const sum = (a) => a.reduce((x, y) => x + y, 0)

// scores a list of frame intervals (ms) and the long tasks (ms durations) inside them
function score(fr, long) {
  return {
    frames: fr.length,
    missed: sum(fr.map((x) => Math.max(0, Math.round(x / VSYNC) - 1))),
    dropped: fr.filter((x) => x > DROP_MS).length,
    max: fr.length ? Math.max(...fr) : 0,
    jank: sum(fr.map((x) => Math.max(0, x - VSYNC))),
    longTasks: long.length, longMs: sum(long),
  }
}
const combine = (ss) => ({
  frames: sum(ss.map((s) => s.frames)), missed: sum(ss.map((s) => s.missed)), dropped: sum(ss.map((s) => s.dropped)),
  max: ss.length ? Math.max(...ss.map((s) => s.max)) : 0, jank: sum(ss.map((s) => s.jank)),
  longTasks: sum(ss.map((s) => s.longTasks)), longMs: sum(ss.map((s) => s.longMs)),
})

// runs in the page: resolves at once if already idle, else after the next idle event
const IDLE = `window.__idle = () => new Promise((res, rej) => {
  const m = window.__map
  if (!m.isMoving() && m.loaded()) return res()
  const t = setTimeout(() => rej(new Error('idle timeout')), 90000)
  m.once('idle', () => { clearTimeout(t); res() }); m.triggerRepaint()
})`

async function oneRun(browser) {
  const page = await browser.newPage()
  try {
    await page.setViewport(VIEWPORT)
    const cdp = await page.createCDPSession()
    await cdp.send('Emulation.setCPUThrottlingRate', { rate: THROTTLE })
    await page.emulateMediaFeatures([{ name: 'prefers-reduced-motion', value: 'no-preference' }])   // headless must not skip animations
    await page.evaluateOnNewDocument(`window.__long = [];
      new PerformanceObserver((l) => { for (const e of l.getEntries()) window.__long.push([e.startTime, e.duration]) }).observe({ entryTypes: ['longtask'] })`)
    await page.goto(URL_ + '/?debug', { waitUntil: 'load' })
    await page.waitForFunction('window.__map && window.__map.loaded()', { timeout: 120000 })
    await page.evaluate(IDLE)
    const out = {}
    for (const [name, lon, lat] of VIEWS) {
      await page.evaluate(`window.__map.jumpTo({ center: [${lon}, ${lat}], zoom: 9 })`)
      await page.evaluate('window.__idle()')
      const moves = {}
      for (const [mv, fn] of MOVES) {
        // frame loop and move start in one task. The first interval is dropped (slice(1)): it runs from this task's
        // start, not from a previous frame. moveend stops the loop.
        const res = await page.evaluate(`(async () => {
          const m = window.__map, fr = [], c = [${lon}, ${lat}]
          let last = performance.now(), on = true
          const tick = (t) => { fr.push(t - last); last = t; if (on) requestAnimationFrame(tick) }
          requestAnimationFrame(tick)
          const done = new Promise((r) => m.once('moveend', r))
          const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches
          const t0 = performance.now()
          ;(${fn.toString()})(m, c)
          await done
          const t1 = performance.now()
          on = false
          await window.__idle()
          const settle = performance.now() - t1
          // long tasks are filtered by startTime in the node side; take them after idle so delivery has happened
          await new Promise((r) => setTimeout(r, 100))
          return { fr: fr.slice(1), t0, t1, long: window.__long.slice(), moveMs: t1 - t0, settle, reduced }
        })()`)
        if (res.reduced) console.error('warning: prefers-reduced-motion is on')
        moves[mv] = {
          fr: res.fr, long: res.long.filter(([s]) => s >= res.t0 && s <= res.t1).map(([s, d]) => ({ s, d })),
          moveMs: res.moveMs, settle: res.settle,
        }
        await new Promise((r) => setTimeout(r, 300))
      }
      out[name] = moves
      const s = combine(MV.map((k) => score(moves[k].fr, moves[k].long.map((x) => x.d))))
      process.stderr.write(`  ${name}: missed ${s.missed}, dropped ${s.dropped}/${s.frames}, max ${r1(s.max)} ms\n`)
    }
    return out
  } finally { await page.close() }
}

// per-run scores from raw frames
function scoreRun(run) {
  const views = {}
  for (const [name] of VIEWS) {
    const per = Object.fromEntries(MV.map((k) => [k, score(run[name][k].fr, run[name][k].long.map((x) => x.d))]))
    views[name] = { moves: per, total: combine(Object.values(per)), settle: Object.fromEntries(MV.map((k) => [k, run[name][k].settle])) }
  }
  const total = combine(Object.values(views).map((v) => v.total))
  total.settle = sum(Object.values(views).map((v) => sum(Object.values(v.settle))))
  return { views, total }
}

const spread = (a) => (median(a) ? (Math.max(...a) - Math.min(...a)) / median(a) : 0)   // unrounded
function summarize(runs) {
  const sc = runs.map(scoreRun)
  const med = (f) => median(sc.map(f))
  const views = {}
  for (const [name] of VIEWS) {
    views[name] = {
      missed: Object.fromEntries([...MV, 'total'].map((k) => [k, med((r) => (k === 'total' ? r.views[name].total : r.views[name].moves[k]).missed)])),
      dropped: med((r) => r.views[name].total.dropped), max: med((r) => r.views[name].total.max), jank: med((r) => r.views[name].total.jank),
      longTasks: med((r) => r.views[name].total.longTasks), longMs: med((r) => r.views[name].total.longMs), frames: med((r) => r.views[name].total.frames),
      settle: Object.fromEntries(MV.map((k) => [k, med((r) => r.views[name].settle[k])])),
      missedSpread: spread(sc.map((r) => r.views[name].total.missed)),
    }
  }
  const T = (k) => med((r) => r.total[k])
  const total = {
    frames: T('frames'), missed: T('missed'), dropped: T('dropped'), max: T('max'), jank: T('jank'), longTasks: T('longTasks'), longMs: T('longMs'), settle: T('settle'),
    missedSpread: spread(sc.map((r) => r.total.missed)), jankSpread: spread(sc.map((r) => r.total.jank)),
  }
  return { views, total }
}

const git = (c) => { try { return execSync('git ' + c, { encoding: 'utf8' }).trim() } catch { return null } }
const t0 = Date.now()
const browser = await puppeteer.launch({ executablePath: CHROME, headless: true, args: ['--enable-gpu-rasterization', '--ignore-gpu-blocklist'] })
const env = {
  chrome: await browser.version(), commit: git('rev-parse HEAD'), dirty: !!git('status --porcelain'),
  viewport: VIEWPORT, throttle: THROTTLE, runs: RUNS, warmup: WARMUP, vsyncMs: VSYNC, dropMs: DROP_MS, date: new Date().toISOString(),
}
mkdirSync(new URL('./results/', import.meta.url), { recursive: true })
const file = new URL(`./results/${LABEL}-${new Date().toISOString().replace(/[:.]/g, '-')}.json`, import.meta.url)
const runs = []
const save = () => writeFileSync(file, JSON.stringify({ label: LABEL, url: URL_, env, completedRuns: runs.length, summary: runs.length ? summarize(runs) : null, raw: runs }))
try {
  for (let i = 0; i < WARMUP; i++) { console.error(`warmup ${i + 1}/${WARMUP} (not measured)`); await oneRun(browser) }
  for (let i = 0; i < RUNS; i++) { console.error(`run ${i + 1}/${RUNS}`); runs.push(await oneRun(browser)); save() }
} catch (e) {
  console.error('aborted: ' + e.message)
  process.exitCode = 1
} finally { await browser.close(); if (runs.length) save() }

if (runs.length) {
  const { views, total: T } = summarize(runs)
  const p = (v, n) => String(v).padStart(n)
  console.log(`\n${LABEL}: ${runs.length}/${RUNS} runs after ${WARMUP} warmup, throttle ${THROTTLE}x, median across runs, ${Math.round((Date.now() - t0) / 1000)} s`)
  console.log('view          missed(in/pan/out)  total spread dropped  max ms  jank ms longTasks longMs  settle in/pan/out (ms)')
  for (const [n, v] of Object.entries(views)) {
    const m = v.missed
    console.log(`${n.padEnd(13)} ${(m.zoomIn + '/' + m.pan + '/' + m.zoomOut).padStart(18)} ${p(m.total, 6)} ${p(Math.round(v.missedSpread * 100) + '%', 6)} ${p(v.dropped, 7)} ${p(r1(v.max), 7)} ${p(Math.round(v.jank), 8)} ${p(v.longTasks, 9)} ${p(Math.round(v.longMs), 6)}  ${MV.map((k) => Math.round(v.settle[k])).join('/')}`)
  }
  console.log(`${'TOTAL'.padEnd(13)} ${' '.repeat(18)} ${p(T.missed, 6)} ${p(Math.round(T.missedSpread * 100) + '%', 6)} ${p(T.dropped, 7)} ${p(r1(T.max), 7)} ${p(Math.round(T.jank), 8)} ${p(T.longTasks, 9)} ${p(Math.round(T.longMs), 6)}  ${Math.round(T.settle)} summed`)
  console.log(`frames ${T.frames}; jank spread ${Math.round(T.jankSpread * 100)}%`)
  console.error('wrote ' + file.pathname)
}
