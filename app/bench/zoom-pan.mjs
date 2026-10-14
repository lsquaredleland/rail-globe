// Zoom/pan benchmark on the production build. Full guide: bench/README.md. Usage:
//   npm run build && npx vite preview --port 4173   (separate shell; or pass --url)
//   node bench/zoom-pan.mjs [--url http://localhost:4173] [--runs 3] [--throttle 6] [--warmup 1] [--label run] [--chrome <path>]
//   --scenario fast   real mouse-wheel zoom bursts (4 levels in ~1 s, 3 in/out cycles) instead of the slow flyTo moves
//   --headed          (fast only) real window; refresh follows the display (ProMotion 120 Hz); frame budget = measured idle control median
//   --delta <n>       (fast only) fixed wheel delta per tick, skips calibration
//   --eval '<js>'     (fast only) JS run in the page after load, for diagnostic toggles; --trace <dir> saves a CDP trace per view
//   --url <external>  non-localhost site: no ?debug, start view via the #view=z/lat/lon hash; the page must expose window.__map
// Primary metric: missed frames = sum of max(0, round(dt / budget) - 1); budget is 16.667 ms (60 Hz) headless, the measured control headed.
// An idle-page control before and after each run flags a halved refresh rate (30 Hz). Results: bench/results/*.json (raw intervals kept).
import puppeteer from 'puppeteer-core'
import { mkdirSync, writeFileSync } from 'node:fs'
import { execSync } from 'node:child_process'
import { loadavg } from 'node:os'

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
const QUERY = arg('query', '')   // extra URL query for local runs, recorded in the result's env.query
const SCENARIO = arg('scenario', 'default')
const EVAL = arg('eval', '')
const TRACE = arg('trace', '')
const HEADED = process.argv.includes('--headed')   // real window on the real display: refresh rate follows the screen (ProMotion = 120 Hz); frame budget = measured control median
const DELTA = arg('delta', '')                      // fast only: fixed wheel delta per tick, skips calibration
const EXT = !/^https?:\/\/(localhost|127\.0\.0\.1)/.test(URL_)   // external site: no ?debug, start view set via the #view= hash
const CHROME = arg('chrome', '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome')
let VSYNC = 1000 / 60   // frame budget; replaced by the measured control median in headed mode
let DROP_MS = 25   // 1.5 vsync: a frame longer than this is dropped (33.3 sits on the 2-vsync rounding boundary)
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

const pct = (a, q) => { if (!a.length) return 0; const s = [...a].sort((x, y) => x - y); return s[Math.min(s.length - 1, Math.ceil(q / 100 * s.length) - 1)] }
// scores a list of frame intervals (ms) and the long tasks (ms durations) inside them
function score(fr, long) {
  return {
    frames: fr.length,
    missed: sum(fr.map((x) => Math.max(0, Math.round(x / VSYNC) - 1))),
    dropped: fr.filter((x) => x > DROP_MS).length,
    max: fr.length ? Math.max(...fr) : 0,
    jank: sum(fr.map((x) => Math.max(0, x - VSYNC))),
    over: fr.filter((x) => x >= 1.5 * VSYNC).length,   // frames that missed at least one vsync (round(dt/budget) >= 2)
    p95: pct(fr, 95), p99: pct(fr, 99),
    longTasks: long.length, longMs: sum(long),
  }
}
const combine = (ss) => ({
  frames: sum(ss.map((s) => s.frames)), missed: sum(ss.map((s) => s.missed)), dropped: sum(ss.map((s) => s.dropped)), over: sum(ss.map((s) => s.over || 0)), p95: 0, p99: 0,
  max: ss.length ? Math.max(...ss.map((s) => s.max)) : 0, jank: sum(ss.map((s) => s.jank)),
  longTasks: sum(ss.map((s) => s.longTasks)), longMs: sum(ss.map((s) => s.longMs)),
})

// runs in the page: resolves at once if already idle, else after the next idle event
const IDLE = `window.__idle = () => new Promise((res, rej) => {
  const m = window.__map
  if (!m.isMoving() && m.loaded()) return res()
  const t = setTimeout(() => { const st = JSON.stringify({ loaded: m.loaded(), moving: m.isMoving(), tiles: m.areTilesLoaded(), style: m.isStyleLoaded(), z: m.getZoom() })
    if (${EXT}) { console.warn('idle timeout ' + st); (window.__idleTimeouts = window.__idleTimeouts || []).push(st); res() } else rej(new Error('idle timeout ' + st)) }, ${EXT ? 20000 : 90000})
  m.once('idle', () => { clearTimeout(t); res() }); m.triggerRepaint()
})`

// control: ~60 rAF intervals on the loaded page with the map not moving. A median near 33 ms means the display runs at 30 Hz.
const CONTROL_MS = 25
const control = (page) => page.evaluate(`new Promise((res) => {
  const fr = []; let last = performance.now(), n = 0
  const tick = (t) => { fr.push(t - last); last = t; if (++n < 61) requestAnimationFrame(tick); else { fr.shift(); res(fr) } }
  requestAnimationFrame(tick)
})`).then(median)

async function oneRun(browser, ctl) {
  const page = await browser.newPage()
  try {
    await page.setViewport(VIEWPORT)
    const cdp = await page.createCDPSession()
    await cdp.send('Emulation.setCPUThrottlingRate', { rate: THROTTLE })
    await page.emulateMediaFeatures([{ name: 'prefers-reduced-motion', value: 'no-preference' }])   // headless must not skip animations
    await page.evaluateOnNewDocument(`window.__long = [];
      new PerformanceObserver((l) => { for (const e of l.getEntries()) window.__long.push([e.startTime, e.duration]) }).observe({ entryTypes: ['longtask'] })`)
    await page.goto(URL_ + '/?debug' + (QUERY ? '&' + QUERY : ''), { waitUntil: 'load' })
    await page.waitForFunction('window.__map && window.__map.loaded()', { timeout: 120000 })
    await page.evaluate(IDLE)
    ctl.push(await control(page))
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
    ctl.push(await control(page))
    return out
  } finally { await page.close() }
}


// ---- fast scenario: real wheel input through MapLibre's scroll-zoom handler ----
const FAST = { startZoom: 10, span: 4, burstMs: 1000, tickMs: 25, pauseMs: 300, cycles: 3 }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const FAST_IDLE = `window.__idleAfter = () => new Promise((res) => {
  const m = window.__map, t0 = performance.now()
  const chk = () => { if (!m.isMoving() && m.loaded()) res(performance.now()); else m.once('idle', () => res(performance.now())) }
  setTimeout(chk, 50)
})`
const urlKind = (u) => (u.includes('.pmtiles') ? 'pmtiles' : u.includes('openfreemap') ? (u.includes('/fonts/') ? 'ofmGlyph' : 'ofmTile') : null)

async function wheelBurst(page, dir, delta) {
  const n = Math.round(FAST.burstMs / FAST.tickMs), t0 = Date.now()
  for (let i = 0; i < n; i++) {
    await page.mouse.wheel({ deltaY: dir * delta })
    const wait = t0 + (i + 1) * FAST.tickMs - Date.now()
    if (wait > 0) await sleep(wait)
  }
}
const zoomOf = (page) => page.evaluate('window.__map.getZoom()')

// finds the per-tick delta whose burst spans FAST.span zoom levels (zoom-in direction); returns {delta, log}
async function calibrate(page, lon, lat) {
  let delta = 60, lo = 0, hi = Infinity; const log = []
  for (let i = 0; i < 6; i++) {
    await page.evaluate(`window.__map.jumpTo({ center: [${lon}, ${lat}], zoom: ${FAST.startZoom} })`)
    await page.evaluate('window.__idle()')
    await wheelBurst(page, -1, delta)
    await sleep(600)
    const span = (await zoomOf(page)) - FAST.startZoom
    log.push([Math.round(delta), +span.toFixed(2)])
    if (Math.abs(span - FAST.span) < 0.15) break
    if (span < FAST.span) lo = delta; else hi = delta
    delta = Number.isFinite(hi) ? (lo + hi) / 2 : delta * FAST.span / Math.max(span, 0.2)
    await page.evaluate('window.__idle()')
  }
  return { delta, log }
}

async function oneRunFast(browser, ctl, runIdx) {
  const page = await browser.newPage()
  const net = { on: false, cur: null }
  const bump = (kind, key) => { if (net.on && kind) net.cur[kind][key]++ }
  const ids = new Map()
  try {
    if (!HEADED) await page.setViewport(VIEWPORT)
    else await page.bringToFront()
    const cdp = await page.createCDPSession()
    await cdp.send('Emulation.setCPUThrottlingRate', { rate: THROTTLE })
    await page.emulateMediaFeatures([{ name: 'prefers-reduced-motion', value: 'no-preference' }])
    await page.evaluateOnNewDocument(`window.__long = [];
      new PerformanceObserver((l) => { for (const e of l.getEntries()) window.__long.push([e.startTime, e.duration]) }).observe({ entryTypes: ['longtask'] })
      addEventListener('wheel', () => { window.__lastWheel = performance.now() }, { capture: true, passive: true })`)
    const track = (c) => {
      c.on('Network.requestWillBeSent', (e) => { const k = urlKind(e.request.url); if (k) { ids.set(e.requestId, k); bump(k, 'requested') } })
      c.on('Network.loadingFinished', (e) => { const k = ids.get(e.requestId); if (k) { bump(k, 'finished'); ids.delete(e.requestId) } })
      c.on('Network.loadingFailed', (e) => { const k = ids.get(e.requestId); if (k) { bump(k, e.canceled ? 'aborted' : 'failed'); ids.delete(e.requestId) } })
    }
    track(cdp); await cdp.send('Network.enable')
    page.on('workercreated', async (w) => { try { track(w.client); await w.client.send('Network.enable') } catch {} })
    await page.goto(EXT ? `${URL_.replace(/\/$/, '')}/#view=${FAST.startZoom}/${VIEWS[0][2]}/${VIEWS[0][1]}` : URL_ + '/?debug' + (QUERY ? '&' + QUERY : ''), { waitUntil: 'load' })
    await page.waitForFunction('window.__map && window.__map.loaded()', { timeout: 120000 })
    await page.evaluate(IDLE); await page.evaluate(FAST_IDLE)
    const win = await page.evaluate('({ dpr: devicePixelRatio, iw: innerWidth, ih: innerHeight, sw: screen.width, sh: screen.height, vis: document.visibilityState, focus: document.hasFocus(), layers: window.__map.getStyle().layers.length })')
    env.win = win
    if (HEADED && (win.vis !== 'visible' || !win.focus)) console.error(`!!! window not foreground: visibility ${win.vis}, focus ${win.focus}; rAF may be throttled`)
    if (EVAL) await page.evaluate(EVAL)
    await page.evaluate('window.__idle()')
    ctl.push(await control(page))
    await page.mouse.move((HEADED ? win.iw : VIEWPORT.width) / 2, (HEADED ? win.ih : VIEWPORT.height) / 2)
    const cal = DELTA ? { delta: Number(DELTA), log: 'fixed' } : await calibrate(page, VIEWS[0][1], VIEWS[0][2])
    const out = { _cal: cal, _win: win, _ctl: ctl.slice() }
    for (const [vi, [name, lon, lat]] of VIEWS.entries()) {
      await page.evaluate(`window.__map.jumpTo({ center: [${lon}, ${lat}], zoom: ${FAST.startZoom} })`)
      await page.evaluate('window.__idle()')
      await sleep(300)
      net.cur = { pmtiles: { requested: 0, finished: 0, aborted: 0, failed: 0 }, ofmTile: { requested: 0, finished: 0, aborted: 0, failed: 0 }, ofmGlyph: { requested: 0, finished: 0, aborted: 0, failed: 0 } }
      const tracing = TRACE && runIdx === 0
      if (tracing) await page.tracing.start({ categories: ['devtools.timeline', 'disabled-by-default-devtools.timeline', 'disabled-by-default-devtools.timeline.frame', 'v8.execute', 'disabled-by-default-v8.cpu_profiler', 'gpu', 'toplevel', 'blink.user_timing'] })
      await page.evaluate(`window.__fr = []; window.__on = true; window.__zl = [];
        { let last = performance.now(); const tick = (t) => { window.__fr.push(t - last); last = t; if (window.__on) requestAnimationFrame(tick) }; requestAnimationFrame(tick) }
        window.__t0 = performance.now()`)
      net.on = true
      const zooms = []
      for (let c = 0; c < FAST.cycles; c++) {
        await wheelBurst(page, -1, cal.delta); zooms.push(+(await zoomOf(page)).toFixed(2))
        await sleep(FAST.pauseMs)
        await wheelBurst(page, 1, cal.delta); zooms.push(+(await zoomOf(page)).toFixed(2))
        await sleep(FAST.pauseMs)
      }
      if (EXT) { const to = await page.evaluate('window.__idleTimeouts || []'); if (to.length) { out._idleTimeouts = to; console.error('  idle timeouts: ' + to.join(' | ')) } }
      const tIdle = await page.evaluate('window.__idleAfter()')
      const lastWheel = await page.evaluate('window.__lastWheel')
      net.on = false
      await page.evaluate('window.__on = false')
      await sleep(150)
      if (tracing) { const buf = await page.tracing.stop(); mkdirSync(TRACE, { recursive: true }); writeFileSync(`${TRACE}/${LABEL}-${name.replace(/ /g, '_')}.json`, buf) }
      const res = await page.evaluate('({ fr: window.__fr.slice(1), t0: window.__t0, t1: performance.now(), long: window.__long.slice() })')
      const long = res.long.filter(([s]) => s >= res.t0 && s <= res.t1).map(([, d]) => d)
      out[name] = { fr: res.fr, long, net: net.cur, idleAfterLastWheelMs: tIdle - lastWheel, zooms }
      const s = score(res.fr, long)
      process.stderr.write(`  ${name}: missed ${s.missed}, dropped ${s.dropped}/${s.frames}, max ${r1(s.max)} ms, idle ${r1(tIdle - lastWheel)} ms, zooms ${zooms.join(' ')}\n`)
    }
    ctl.push(await control(page))
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

// Per-phase warning only (never used to exclude a run): a phase whose frames are all ~33.3 ms looks like a 30 Hz display, but at
// 60 Hz genuinely slow frames also show as 33.3 ms. The control (idle page, before and after a run) decides whether a run is flagged.
const halved = (fr) => fr.length >= 10 && (Math.min(...fr) >= 30 || (Math.abs(median(fr) - 2 * VSYNC) < 1.5 && fr.filter((x) => x < 20).length <= fr.length * 0.02))
const phaseWarn = (run) => VIEWS.flatMap(([n]) => MV.filter((k) => halved(run[n][k].fr)).map((k) => `${n}/${k}`))
const flagged = (c) => c.some((x) => x > CONTROL_MS)

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
const browser = await puppeteer.launch({ executablePath: CHROME, headless: !HEADED, ...(HEADED ? { defaultViewport: null } : {}),
  args: ['--enable-gpu-rasterization', '--ignore-gpu-blocklist', ...(HEADED ? ['--window-size=1440,900', '--window-position=0,0'] : [])] })
const env = {
  query: QUERY,
  chrome: await browser.version(), commit: git('rev-parse HEAD'), dirty: !!git('status --porcelain'),
  viewport: VIEWPORT, headed: HEADED, ext: EXT, url: URL_, throttle: THROTTLE, runs: RUNS, warmup: WARMUP, vsyncMs: VSYNC, dropMs: DROP_MS, date: new Date().toISOString(),
  loadAvg: loadavg(), loadAvgEnd: null, controlMs: [], refreshHalved: [], controlLimitMs: CONTROL_MS,
}
if (env.loadAvg[0] > 2) console.error(`WARNING: 1-min load average ${env.loadAvg[0].toFixed(1)} > 2: the machine is busy, results may be noisy`)

if (SCENARIO === 'fast') {
  mkdirSync(new URL('./results/', import.meta.url), { recursive: true })
  const ffile = new URL(`./results/${LABEL}-fast-${new Date().toISOString().replace(/[:.]/g, '-')}.json`, import.meta.url)
  const fruns = []
  try {
    for (let i = 0; i < WARMUP; i++) { console.error(`warmup ${i + 1}/${WARMUP}`); await oneRunFast(browser, [], -1) }
    for (let i = 0; i < RUNS; i++) {
      console.error(`run ${i + 1}/${RUNS}`)
      const ctl = []
      fruns.push(await oneRunFast(browser, ctl, i))
      env.controlMs.push(ctl.map(r1)); env.refreshHalved.push(flagged(ctl))
      if (flagged(ctl)) console.error(`!!! run ${i + 1} REFRESH HALVED: control ${ctl.map(r1).join(' / ')} ms`)
      writeFileSync(ffile, JSON.stringify({ label: LABEL, scenario: 'fast', fast: FAST, eval: EVAL, env, raw: fruns }))
    }
  } catch (e) { console.error('aborted: ' + e.stack); process.exitCode = 1 } finally { await browser.close(); env.loadAvgEnd = loadavg() }
  writeFileSync(ffile, JSON.stringify({ label: LABEL, scenario: 'fast', fast: FAST, eval: EVAL, env, raw: fruns }))
  const p = (v, n) => String(v).padStart(n)
  if (HEADED && fruns.length) {
    VSYNC = median(env.controlMs.flat()); DROP_MS = 1.5 * VSYNC
    console.log(`\nHEADED: window ${JSON.stringify(env.win)}; control median per run ${JSON.stringify(env.controlMs)} ms -> frame budget ${VSYNC.toFixed(2)} ms (${(1000 / VSYNC).toFixed(0)} Hz); style layers ${env.win?.layers}`)
    console.log(`budget-relative, median across ${fruns.length} runs ("over" = frames with dt >= 1.5 budget)`)
    console.log('view          missed  over/frames  over%   p95    p99    max   jank ms  zooms(run1)')
    const row = (n, f) => {
      const sc = fruns.map((r) => score(f(r), n === 'TOTAL' ? [] : r[n].long)), m = (k) => median(sc.map((x) => x[k]))
      console.log(`${n.padEnd(13)} ${p(m('missed'), 6)} ${p(m('over') + '/' + m('frames'), 12)} ${p((100 * m('over') / m('frames')).toFixed(1), 6)} ${p(r1(m('p95')), 6)} ${p(r1(m('p99')), 6)} ${p(r1(m('max')), 6)} ${p(Math.round(m('jank')), 8)}  ${n === 'TOTAL' ? '' : fruns[0][n].zooms.join(' ')}`)
    }
    for (const [name] of VIEWS) row(name, (r) => r[name].fr)
    row('TOTAL', (r) => VIEWS.flatMap(([n]) => r[n].fr))
  }
  console.log(`\n${LABEL} fast: ${fruns.length} runs, throttle ${THROTTLE}x, median across runs; calibrated delta ${fruns.map((r) => Math.round(r._cal.delta)).join('/')} (cal log ${JSON.stringify(fruns[0]?._cal.log)})`)
  console.log('view          missed dropped/frames  max ms  jank ms longTasks longMs idleAfter(ms) | pmtiles req/abort/fail | ofmTile req/abort/fail | glyph req/abort')
  const tot = { missed: [], dropped: [], jank: [], max: [] }
  for (const [name] of VIEWS) {
    const sc = fruns.map((r) => ({ s: score(r[name].fr, r[name].long), r: r[name] }))
    const m = (f) => median(sc.map(f))
    const nm = (k, f) => m((x) => x.r.net[k][f])
    console.log(`${name.padEnd(13)} ${p(m((x) => x.s.missed), 6)} ${p(m((x) => x.s.dropped) + '/' + m((x) => x.s.frames), 14)} ${p(r1(m((x) => x.s.max)), 7)} ${p(Math.round(m((x) => x.s.jank)), 8)} ${p(m((x) => x.s.longTasks), 9)} ${p(Math.round(m((x) => x.s.longMs)), 6)} ${p(Math.round(m((x) => x.r.idleAfterLastWheelMs)), 14)} | ${p(nm('pmtiles', 'requested') + '/' + nm('pmtiles', 'aborted') + '/' + nm('pmtiles', 'failed'), 12)} | ${p(nm('ofmTile', 'requested') + '/' + nm('ofmTile', 'aborted') + '/' + nm('ofmTile', 'failed'), 12)} | ${nm('ofmGlyph', 'requested')}/${nm('ofmGlyph', 'aborted')}`)
    tot.missed.push(m((x) => x.s.missed)); tot.dropped.push(m((x) => x.s.dropped)); tot.jank.push(m((x) => x.s.jank)); tot.max.push(m((x) => x.s.max))
  }
  console.log(`TOTAL missed ${sum(tot.missed)}, dropped ${sum(tot.dropped)}, jank ${Math.round(sum(tot.jank))} ms, max ${r1(Math.max(...tot.max))} ms; zooms at end of each half-burst (run 1, last view): ${fruns[0]?.Shanghai.zooms.join(' ')}`)
  console.log('refresh-halved control medians ms: ' + env.controlMs.map((c) => c.join('/')).join('; ') + `; load avg start ${env.loadAvg.map((x) => x.toFixed(2)).join(' ')} end ${env.loadAvgEnd.map((x) => x.toFixed(2)).join(' ')}`)
  console.error('wrote ' + ffile.pathname)
  process.exit(process.exitCode || 0)
}
mkdirSync(new URL('./results/', import.meta.url), { recursive: true })
const file = new URL(`./results/${LABEL}-${new Date().toISOString().replace(/[:.]/g, '-')}.json`, import.meta.url)
const runs = []
const save = () => writeFileSync(file, JSON.stringify({ label: LABEL, url: URL_, env, completedRuns: runs.length, summary: runs.length ? summarize(runs) : null,
  summaryCleanRuns: runs.length ? cleanSummary() : null, raw: runs }))
const cleanIdx = () => runs.map((_, i) => i).filter((i) => !flagged(env.controlMs[i] || []))
const cleanSummary = () => { const ix = cleanIdx(); return ix.length >= 2 ? { runs: ix.length, ...summarize(ix.map((i) => runs[i])) } : null }
try {
  for (let i = 0; i < WARMUP; i++) { console.error(`warmup ${i + 1}/${WARMUP} (not measured)`); await oneRun(browser, []) }
  for (let i = 0; i < RUNS; i++) {
    console.error(`run ${i + 1}/${RUNS}`)
    const ctl = []
    runs.push(await oneRun(browser, ctl))
    env.controlMs.push(ctl.map(r1))
    const pw = phaseWarn(runs[runs.length - 1]), fl = flagged(ctl)
    env.refreshHalved.push(fl)
    if (fl) console.error(`!!! WARNING: run ${i + 1} looks REFRESH HALVED (30 Hz): idle-page control median ${ctl.map(r1).join(' / ')} ms (limit ${CONTROL_MS}). Environmental, not code. Run kept in the all-runs summary.`)
    if (pw.length) console.error(`warning (info only, run kept): run ${i + 1} phases with all frames ~33 ms: ${pw.join(', ')}`)
    save()
  }
} catch (e) {
  console.error('aborted: ' + e.message)
  process.exitCode = 1
} finally { await browser.close(); env.loadAvgEnd = loadavg(); if (runs.length) save() }

if (runs.length) {
  const p = (v, n) => String(v).padStart(n)
  const print = (title, rs) => {
    const { views, total: T } = summarize(rs)
    console.log(`\n${title}: ${rs.length} runs of ${RUNS} after ${WARMUP} warmup, throttle ${THROTTLE}x, median across runs, ${Math.round((Date.now() - t0) / 1000)} s`)
    console.log('view          missed(in/pan/out)  total spread dropped  max ms  jank ms longTasks longMs  settle in/pan/out (ms)')
    for (const [n, v] of Object.entries(views)) {
      const m = v.missed
      console.log(`${n.padEnd(13)} ${(m.zoomIn + '/' + m.pan + '/' + m.zoomOut).padStart(18)} ${p(m.total, 6)} ${p(Math.round(v.missedSpread * 100) + '%', 6)} ${p(v.dropped, 7)} ${p(r1(v.max), 7)} ${p(Math.round(v.jank), 8)} ${p(v.longTasks, 9)} ${p(Math.round(v.longMs), 6)}  ${MV.map((k) => Math.round(v.settle[k])).join('/')}`)
    }
    console.log(`${'TOTAL'.padEnd(13)} ${' '.repeat(18)} ${p(T.missed, 6)} ${p(Math.round(T.missedSpread * 100) + '%', 6)} ${p(T.dropped, 7)} ${p(r1(T.max), 7)} ${p(Math.round(T.jank), 8)} ${p(T.longTasks, 9)} ${p(Math.round(T.longMs), 6)}  ${Math.round(T.settle)} summed`)
    console.log(`frames ${T.frames}; jank spread ${Math.round(T.jankSpread * 100)}%`)
  }
  print(`${LABEL} (ALL ${runs.length} runs)`, runs)
  const ix = cleanIdx()
  if (ix.length < runs.length) {
    console.log(`\n!!! REFRESH HALVED (30 Hz) control flagged run(s) ${runs.map((_, i) => i).filter((i) => !ix.includes(i)).map((i) => i + 1).join(', ')} (control medians ms: ${env.controlMs.map((c) => c.join('/')).join('; ')})`)
    if (ix.length >= 2) print(`${LABEL} (CLEAN runs only: ${ix.length})`, ix.map((i) => runs[i]))
    else console.log(`only ${ix.length} clean run(s): no clean-only summary`)
  } else console.log('refresh-halved control: none flagged (control medians ms ' + env.controlMs.map((c) => c.join('/')).join('; ') + ')')
  const f = (a) => a.map((x) => x.toFixed(2)).join(' ')
  console.log(`load avg at start ${f(env.loadAvg)}; at end ${f(env.loadAvgEnd)}`)
  console.error('wrote ' + file.pathname)
}
