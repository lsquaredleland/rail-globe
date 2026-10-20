// Hover-sweep benchmark: a scripted fast mouse sweep over rail lines, the interaction zoom-pan.mjs never exercises.
//   npm run build && npx vite preview --port 4173   (separate shell; or --url)
//   node bench/hover-sweep.mjs [--url http://localhost:4173] [--views Paris,Chengdu,Europe] [--zooms 8,10,12] [--throttle 4]
//                              [--moves 400] [--interval 10] [--mode jump|sweep] [--runs 2] [--headed] [--label hv] [--chrome <path>]
// Per view and zoom: jump there, wait for idle, pick points that lie on rail (vertices of the rendered hit-layer features), then move the
// mouse through them, one move every --interval ms. `jump` visits the points in random order (each move lands on another line: the worst
// case, a highlight change per frame); `sweep` visits them left to right. The first sweep at a zoom is COLD (shard files and tiles not yet
// loaded), the later ones WARM. Output per sweep:
//   hover ms   duration of the requestAnimationFrame callbacks that ran a hit test (queryRenderedFeatures): the map's mousemove handler
//   query ms   queryRenderedFeatures alone;  setFilter n/ms  calls and time of Map.setFilter;  react renders  DOM mutation batches (App re-renders)
//   frames>25 / >50  rAF intervals over 25 / 50 ms;  long tasks  main-thread tasks over 50 ms;  busy %  CDP TaskDuration / wall time
//   fetches    non-tile requests during the sweep (side-file shards)
// Needs window.__map: the script adds ?debug. Results go to bench/results/hover-<label>-<time>.json.
import puppeteer from 'puppeteer-core'
import { mkdirSync, writeFileSync, mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const arg = (n, d) => {
  const i = process.argv.indexOf('--' + n)
  return i > 0 ? process.argv[i + 1] : d
}
const URL_ = arg('url', 'http://localhost:4173')
const VIEWS_ALL = {
  Paris: [2.35, 48.86],
  Chengdu: [104.07, 30.66],
  Europe: [8.0, 49.5],
  Shenzhen: [114.06, 22.58],
  'SF Bay': [-122.3, 37.8],
  Tokyo: [139.74, 35.68],
  Basel: [7.59, 47.56],
  'HK border': [114.1, 22.45],
}
const VIEWS = arg('views', 'Paris,Chengdu,Europe')
  .split(',')
  .map((n) => {
    if (!VIEWS_ALL[n]) {
      console.error('unknown view', n)
      process.exit(1)
    }
    return [n, ...VIEWS_ALL[n]]
  })
const ZOOMS = arg('zooms', '8,10,12').split(',').map(Number)
const THROTTLE = Number(arg('throttle', 4))
const MOVES = Number(arg('moves', 400))
const INTERVAL = Number(arg('interval', 10))
const MODE = arg('mode', 'jump')
const RUNS = Number(arg('runs', 2))
const LATENCY = Number(arg('latency', 0)),
  DOWN = Number(arg('down', 50)) // network emulation for page and workers (side files and tiles)
const LATE = process.argv.includes('--late') // sweep as soon as the tiles are in, without waiting for the idle prefetch of highlight data (a user who pans and hovers at once)
const DWELL = Number(arg('dwell', 0)) // N > 0: time-to-highlight mode: N hovers, one per line, each from the empty map; waits for the highlight filter to be set
const COLD = process.argv.includes('--cold') // a new browser with an empty profile (no HTTP cache) for every view and zoom
const PREFETCH = process.argv.includes('--prefetch') // measure the idle prefetch of highlight data after each jump: long tasks, shard requests and bytes (no mouse)
const PRE = arg('pre', '') // JS run in the page after the instruments are installed, for A/B toggles (like zoom-pan.mjs --eval)
const FRESH = process.argv.includes('--fresh') // new page per zoom and sweep right after idle (no wait for the highlight prefetch): shard files are cold
const PROFILE = arg('profile', '') // dir: save a CPU profile of the last sweep per view and zoom, and print the top self-time functions (use an unminified build)
const HEADED = process.argv.includes('--headed')
const LABEL = arg('label', 'hv')
const CHROME = arg('chrome', '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome')
const VIEWPORT = { width: 1440, height: 900, deviceScaleFactor: 2 }

const q = (a, p) => {
  const s = [...a].sort((x, y) => x - y)
  return s.length ? s[Math.min(s.length - 1, Math.floor(p * s.length))] : 0
}
const r2 = (x) => Math.round(x * 100) / 100

// installed in the page once: wraps the map and requestAnimationFrame, collects into window.__hv
const INSTALL = `(() => {
  const map = window.__map, hv = window.__hv = { on: false, rafCbs: [], frames: [], long: [], query: [], setFilter: [], mut: 0, last: 0 }
  const oq = map.queryRenderedFeatures.bind(map)
  let inQuery = 0
  map.queryRenderedFeatures = (...a) => { const t = performance.now(); inQuery++; try { return oq(...a) } finally { if (hv.on) hv.query.push(performance.now() - t) } }
  hv.hlSet = []
  const osf = map.setFilter.bind(map)
  map.setFilter = (...a) => { const t = performance.now(); if (/:hl-fs$/.test(a[0]) && Array.isArray(a[1]) && a[1][0] === 'in') hv.hlSet.push(t); try { return osf(...a) } finally { if (hv.on) hv.setFilter.push([a[0], performance.now() - t, JSON.stringify(a[1] ?? null).length]) } }
  hv.tiles = {}
  map.on('data', (e) => { if (hv.on && e.dataType === 'source' && e.tile) hv.tiles[e.sourceId] = (hv.tiles[e.sourceId] ?? 0) + 1 })
  // side-file reads (shards): count, bytes and the main-thread cost of Response.json()
  hv.json = []
  const ofetch = window.fetch.bind(window)
  window.fetch = (...a) => ofetch(...a).then((r) => {
    const u = String(a[0]?.url ?? a[0])
    if (!['/lines/', '/lines-big/', '/name-hl/', '/stations/', '/tags/'].some((k) => u.endsWith('.json') && u.includes(k))) return r
    return r
    const oj = r.json.bind(r)
    r.json = async () => { const t = performance.now(); const v = await oj(); hv.json.push([u.split('/').slice(-3).join('/'), performance.now() - t, Number(r.headers.get('content-length') ?? 0)]); return v }
    return r
  })
  const raf = window.requestAnimationFrame.bind(window)
  window.requestAnimationFrame = (cb) => raf((ts) => {
    const before = hv.query.length, t = performance.now()
    cb(ts)
    if (hv.on && hv.query.length > before) hv.rafCbs.push(performance.now() - t)
  })
  new PerformanceObserver((l) => { if (hv.on) for (const e of l.getEntries()) hv.long.push(e.duration) }).observe({ entryTypes: ['longtask'] })
  new MutationObserver((m) => { if (hv.on) hv.mut++ }).observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true })
  const tick = (t) => { if (hv.on) { if (hv.last) hv.frames.push(t - hv.last); hv.last = t } else hv.last = 0; raf(tick) }
  raf(tick)
})()`

const FIND_POINTS = `(n, mode, onePerLine) => {
  const map = window.__map
  const ids = map.getStyle().layers.map((l) => l.id).filter((id) => /:hit$/.test(id))
  const fs = map.queryRenderedFeatures({ layers: ids })
  const cv = map.getCanvas(), W = cv.clientWidth, H = cv.clientHeight
  const pts = [], seen = new Set()
  const lineSeen = new Set()
  for (const f of fs) {
    if (onePerLine) { const pr = f.properties, ln = Number(String(pr.ln ?? '').split(',')[0]), key = ln > 0 ? 'l' + ln : pr.n ? 'n' + pr.n : pr.nk != null ? 'k' + pr.nk : ''; if (!key || lineSeen.has(key)) continue; lineSeen.add(key) }
    const g = f.geometry, lines = g.type === 'LineString' ? [g.coordinates] : g.type === 'MultiLineString' ? g.coordinates : []
    for (const ln of lines) for (const c of ln) {
      const p = map.project(c)
      if (p.x < 60 || p.x > W - 60 || p.y < 60 || p.y > H - 60) continue
      const k = Math.round(p.x / 14) + ',' + Math.round(p.y / 14)
      if (seen.has(k)) continue
      seen.add(k); pts.push([Math.round(p.x), Math.round(p.y)])
    }
  }
  // random subset (deterministic LCG), then order
  let s = 12345; const rnd = () => (s = (s * 1664525 + 1013904223) >>> 0) / 4294967296
  for (let i = pts.length - 1; i > 0; i--) { const j = Math.floor(rnd() * (i + 1)); [pts[i], pts[j]] = [pts[j], pts[i]] }
  const pick = pts.slice(0, n)
  if (mode === 'sweep') pick.sort((a, b) => a[0] - b[0])
  return { total: pts.length, pts: pick, features: fs.length }
}`

const out = {
  env: {
    url: URL_,
    throttle: THROTTLE,
    latency: LATENCY,
    downMbps: DOWN,
    moves: MOVES,
    interval: INTERVAL,
    mode: MODE,
    runs: RUNS,
    headed: HEADED,
    late: LATE,
    cold: COLD,
    dwell: DWELL,
    time: new Date().toISOString(),
  },
  sweeps: [],
}
let browser, page, cdp
let net = null // while set: the side-file requests seen by CDP, by request id
const SHARD = /\/(lines|lines-big|name-hl)\/[^/]+\.json/
async function launch() {
  if (browser) await browser.close()
  browser = await puppeteer.launch({
    executablePath: CHROME,
    headless: !HEADED,
    userDataDir: mkdtempSync(join(tmpdir(), 'hv-')),
    args: ['--enable-gpu-rasterization', '--ignore-gpu-blocklist', ...(HEADED ? ['--window-size=1440,900', '--window-position=0,0'] : [])],
    ...(HEADED ? { defaultViewport: null } : { defaultViewport: VIEWPORT }),
  })
  page = await browser.newPage()
  cdp = await page.createCDPSession()
  await cdp.send('Performance.enable')
  await cdp.send('Network.enable')
  if (LATENCY)
    await cdp.send('Network.emulateNetworkConditions', { offline: false, latency: LATENCY, downloadThroughput: DOWN * 125000, uploadThroughput: DOWN * 125000 })
  await cdp.send('Emulation.setCPUThrottlingRate', { rate: THROTTLE })
  page.on('pageerror', (e) => console.error('pageerror', e.message))
  const inflight = new Set()
  cdp.on('Network.requestWillBeSent', (e) => {
    if (!net) return
    net.all.set(e.requestId, { url: e.request.url, t0: e.timestamp, shard: SHARD.test(e.request.url) })
    inflight.add(e.requestId)
    net.maxInflight = Math.max(net.maxInflight, inflight.size)
  })
  cdp.on('Network.responseReceived', (e) => {
    const r = net?.all.get(e.requestId)
    if (r) {
      r.proto = e.response.protocol
      r.start = e.response.timing?.requestTime
      r.sendStart = e.response.timing?.sendStart
      r.enc = e.response.encodedDataLength
    }
  })
  cdp.on('Network.loadingFinished', (e) => {
    inflight.delete(e.requestId)
    const r = net?.all.get(e.requestId)
    if (r) {
      r.t1 = e.timestamp
      r.bytes = e.encodedDataLength
    }
  })
  cdp.on('Network.loadingFailed', (e) => inflight.delete(e.requestId))
}
const metric = async (name) => (await cdp.send('Performance.getMetrics')).metrics.find((m) => m.name === name).value
const idle = (ms = 30000) =>
  page.evaluate(
    (ms) =>
      new Promise((res) => {
        const m = window.__map,
          t0 = performance.now()
        const chk = () =>
          m.loaded() && !m.isMoving() && m.areTilesLoaded()
            ? setTimeout(() => (m.loaded() && m.areTilesLoaded() ? res(true) : chk()), 400)
            : performance.now() - t0 > ms
              ? res(false)
              : setTimeout(chk, 100)
        chk()
      }),
    ms
  )
const idle0 = async () => {}
// tiles in, without the settle time and prefetch wait of idle()
const tilesIn = (ms = 30000) =>
  page.evaluate(
    (ms) =>
      new Promise((res) => {
        const m = window.__map,
          t0 = performance.now()
        const chk = () => (m.loaded() && !m.isMoving() && m.areTilesLoaded() ? res(true) : performance.now() - t0 > ms ? res(false) : setTimeout(chk, 50))
        chk()
      }),
    ms
  )
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function saveProfile(name) {
  const { profile } = await cdp.send('Profiler.stop')
  mkdirSync(PROFILE, { recursive: true })
  writeFileSync(join(PROFILE, `${LABEL}-${name}.cpuprofile`), JSON.stringify(profile))
  const self = new Map(),
    dt = profile.timeDeltas
  const byId = new Map(profile.nodes.map((n) => [n.id, n]))
  profile.samples.forEach((id, i) => {
    const n = byId.get(id),
      f = n.callFrame,
      k = `${f.functionName || '(anon)'} ${f.url.split('/').pop()}:${f.lineNumber}`
    self.set(k, (self.get(k) ?? 0) + (dt[i] ?? 0) / 1000)
  })
  const tot = [...self.values()].reduce((a, b) => a + b, 0)
  console.log(`  profile ${name}: ${r2(tot)} ms sampled; top self time:`)
  for (const [k, v] of [...self].sort((a, b) => b[1] - a[1]).slice(0, 14)) console.log(`    ${r2(v).toString().padStart(8)} ms  ${k}`)
}

const netSummary = () => {
  const rs = [...net.all.values()].filter((r) => r.shard)
  const dur = rs.filter((r) => r.t1).map((r) => (r.t1 - r.t0) * 1000)
  const queue = rs.filter((r) => r.start != null && r.sendStart != null).map((r) => Math.max(0, (r.start - r.t0) * 1000))
  return {
    shardReqs: rs.length,
    shardKB: Math.round(rs.reduce((a, r) => a + (r.bytes ?? 0), 0) / 1024),
    shardProto: [...new Set(rs.map((r) => r.proto))].join(','),
    shardDurP50: r2(q(dur, 0.5)),
    shardDurMax: r2(Math.max(0, ...dur)),
    shardQueueP50: r2(q(queue, 0.5)),
    shardQueueMax: r2(Math.max(0, ...queue)),
    shardRegions: [...new Set(rs.map((r) => (r.url.match(/\/world\/([^/]+)\//) ?? [])[1]).filter(Boolean))].join(','),
    allReqs: net.all.size,
    maxInflight: net.maxInflight,
  }
}

if (!COLD) await launch()
for (const [vname, lng, lat] of VIEWS) {
  const open = async () => {
    if (COLD || !browser) await launch()
    const u = new URL(URL_)
    u.searchParams.set('debug', '1')
    await page.goto(u.href, { waitUntil: 'load' })
    await page.waitForFunction(() => window.__map && window.__map.loaded(), { timeout: 90000 })
    await page.evaluate(INSTALL)
    if (PRE) await page.evaluate(PRE)
  }
  if (!FRESH && !COLD) await open()
  for (const z of ZOOMS) {
    if (FRESH || COLD) await open()
    if (PREFETCH) {
      net = { all: new Map(), maxInflight: 0 }
      await page.evaluate(() => {
        const h = window.__hv
        h.long = []
        h.frames = []
        h.json = []
        h.last = 0
        h.on = true
      })
      if (PROFILE) {
        await cdp.send('Profiler.enable')
        await cdp.send('Profiler.setSamplingInterval', { interval: 200 })
      }
      await idle0()
      if (PROFILE) await cdp.send('Profiler.start')
      const t0 = await metric('TaskDuration'),
        w0 = Date.now()
      await page.evaluate((lng, lat, z) => window.__map.jumpTo({ center: [lng, lat], zoom: z }), lng, lat, z)
      await idle()
      await sleep(4000)
      if (PROFILE) await saveProfile(`${vname}-z${z}-prefetch`)
      const wall = Date.now() - w0,
        busy = (await metric('TaskDuration')) - t0
      const h = await page.evaluate(() => {
        window.__hv.on = false
        return { ...window.__hv }
      })
      const ns = netSummary()
      net = null
      const row = {
        view: vname,
        zoom: z,
        mode: 'prefetch',
        longTasks: h.long.length,
        longMs: r2(h.long.reduce((a, b) => a + b, 0)),
        longMax: r2(Math.max(0, ...h.long)),
        maxFrame: r2(Math.max(0, ...h.frames)),
        over25: h.frames.filter((f) => f > 25).length,
        jsonReads: h.json.length,
        jsonMs: r2(h.json.reduce((a, j) => a + j[1], 0)),
        jsonMax: r2(Math.max(0, ...h.json.map((j) => j[1]))),
        busyMs: r2(busy * 1000),
        ...ns,
      }
      out.sweeps.push(row)
      console.log(
        `${vname.padEnd(8)} z${String(z).padEnd(3)} PREFETCH after jump+idle (${Math.round(wall)} ms)  shard reqs ${row.shardReqs} ${row.shardKB} KB ${row.shardProto} dur p50/max ${row.shardDurP50}/${row.shardDurMax} ms | regions ${row.shardRegions} | long tasks ${row.longTasks} (${row.longMs} ms, max ${row.longMax}) | frames>25 ${row.over25} max ${row.maxFrame} | json parse ${row.jsonReads}x ${row.jsonMs} ms (max ${row.jsonMax}) | main-thread busy ${row.busyMs} ms | max in flight ${row.maxInflight} of ${row.allReqs}`
      )
      continue
    }
    await page.evaluate((lng, lat, z) => window.__map.jumpTo({ center: [lng, lat], zoom: z }), lng, lat, z)
    if (LATE) await tilesIn()
    else {
      await idle()
      if (!FRESH) await sleep(1500)
      await idle()
    }
    let fp = null
    for (let tries = 0; tries < 150; tries++) {
      // rail features are queryable once the first tiles are drawn
      fp = await page.evaluate(`(${FIND_POINTS})(${DWELL || MOVES}, ${JSON.stringify(MODE)}, ${DWELL > 0})`)
      if (fp.pts.length >= (DWELL ? 5 : 20)) break
      await sleep(200)
    }
    if (fp.pts.length < (DWELL ? 5 : 20)) {
      console.log(`${vname} z${z}: only ${fp.pts.length} rail points, skipped`)
      continue
    }

    if (DWELL) {
      // time-to-highlight: from the mouse landing on a line to the highlight filter being set (data loaded included)
      net = { all: new Map(), maxInflight: 0 }
      await page.evaluate(() => {
        window.__hv.on = true
      })
      const lags = [],
        waits = []
      let miss = 0
      for (const [x, y] of fp.pts) {
        await page.mouse.move(5, 5)
        await sleep(120)
        const seen0 = new Set(net.all.keys())
        await page.evaluate(() => {
          window.__hv.mark = performance.now()
          window.__hv.hlSet = []
        })
        await page.mouse.move(x, y)
        const lag = await page.evaluate(
          () =>
            new Promise((res) => {
              const t0 = performance.now(),
                chk = () =>
                  window.__hv.hlSet.length ? res(window.__hv.hlSet[0] - window.__hv.mark) : performance.now() - t0 > 2500 ? res(-1) : setTimeout(chk, 5)
              chk()
            })
        )
        if (process.env.HV_DEBUG)
          console.log(
            '   dwell',
            x,
            y,
            Math.round(lag),
            JSON.stringify(await page.evaluate(() => ({ n: window.__hv.hlSet.length, sf: window.__hv.setFilter.length, hov: window.__hv.rafCbs.length })))
          )
        if (lag < 0) miss++
        else lags.push(lag)
        await sleep(150) // let a started fetch finish so its time is counted
        const mine = [...net.all].filter(([id, r]) => !seen0.has(id) && r.shard)
        waits.push(mine.length ? Math.max(...mine.map(([, r]) => ((r.t1 ?? r.t0) - r.t0) * 1000)) : 0)
      }
      await sleep(300)
      const row = {
        view: vname,
        zoom: z,
        mode: 'dwell',
        fetchWaitP50: r2(q(waits, 0.5)),
        fetchWaitP95: r2(q(waits, 0.95)),
        fetchWaitMax: r2(Math.max(0, ...waits)),
        hoversThatFetched: waits.filter((w) => w > 0).length,
        late: LATE,
        hovers: fp.pts.length,
        never: miss,
        tthP50: r2(q(lags, 0.5)),
        tthP95: r2(q(lags, 0.95)),
        tthMax: r2(Math.max(0, ...lags)),
        over100: lags.filter((l) => l > 100).length,
        over300: lags.filter((l) => l > 300).length,
        ...netSummary(),
      }
      net = null
      out.sweeps.push(row)
      console.log(
        `${vname.padEnd(8)} z${String(z).padEnd(3)} DWELL${LATE ? ' late' : ''}  hovers that had to fetch a shard ${row.hoversThatFetched} of ${row.hovers}, fetch wait p50/p95/max ${row.fetchWaitP50}/${row.fetchWaitP95}/${row.fetchWaitMax} ms | highlight set within 2.5 s ${lags.length} (p50/p95/max ${row.tthP50}/${row.tthP95}/${row.tthMax} ms; the rest have no highlight data or none yet) | shard reqs ${row.shardReqs} ${row.shardKB} KB ${row.shardProto} dur p50/max ${row.shardDurP50}/${row.shardDurMax} ms queue p50/max ${row.shardQueueP50}/${row.shardQueueMax} ms | max in flight ${row.maxInflight} of ${row.allReqs} reqs`
      )
      continue
    }

    for (let run = 0; run < (FRESH || COLD ? 1 : RUNS); run++) {
      await page.mouse.move(5, 5)
      await sleep(300)
      await page.evaluate(() => {
        const h = window.__hv
        h.rafCbs = []
        h.json = []
        h.tiles = {}
        h.frames = []
        h.long = []
        h.query = []
        h.setFilter = []
        h.mut = 0
        h.last = 0
        h.on = true
      })
      net = { all: new Map(), maxInflight: 0 }
      const prof = PROFILE && run === RUNS - 1
      if (prof) {
        await cdp.send('Profiler.enable')
        await cdp.send('Profiler.setSamplingInterval', { interval: 200 })
        await cdp.send('Profiler.start')
      }
      const t0 = await metric('TaskDuration'),
        w0 = Date.now()
      for (const [x, y] of fp.pts) {
        await page.mouse.move(x, y)
        await sleep(INTERVAL)
      }
      await sleep(200)
      if (prof) await saveProfile(`${vname}-z${z}`)
      const wall = Date.now() - w0,
        busy = (await metric('TaskDuration')) - t0
      const h = await page.evaluate(() => {
        window.__hv.on = false
        return { ...window.__hv }
      })
      await sleep(500)
      const ns = netSummary()
      net = null
      const sfm = h.setFilter.map((s) => s[1])
      const byLayer = {}
      for (const [l, ms] of h.setFilter) {
        byLayer[l] = byLayer[l] ?? [0, 0]
        byLayer[l][0]++
        byLayer[l][1] += ms
      }
      const filterKB = h.setFilter.map((s) => s[2] / 1024)
      const row = {
        view: vname,
        zoom: z,
        phase: run === 0 ? 'cold' : 'warm',
        late: LATE,
        points: fp.pts.length,
        hitFeatures: fp.features,
        hoverP50: r2(q(h.rafCbs, 0.5)),
        hoverP95: r2(q(h.rafCbs, 0.95)),
        hoverMax: r2(Math.max(0, ...h.rafCbs)),
        hovers: h.rafCbs.length,
        queryP50: r2(q(h.query, 0.5)),
        queryP95: r2(q(h.query, 0.95)),
        setFilterN: sfm.length,
        setFilterMs: r2(sfm.reduce((a, b) => a + b, 0)),
        setFilterMaxMs: r2(Math.max(0, ...sfm)),
        filterKBmax: r2(Math.max(0, ...filterKB)),
        setFilterByLayer: byLayer,
        reactRenders: h.mut,
        frames: h.frames.length,
        over25: h.frames.filter((f) => f > 25).length,
        over50: h.frames.filter((f) => f > 50).length,
        maxFrame: r2(Math.max(0, ...h.frames)),
        longTasks: h.long.length,
        longMs: r2(h.long.reduce((a, b) => a + b, 0)),
        tileLoads: h.tiles,
        jsonReads: h.json.length,
        jsonMs: r2(h.json.reduce((a, j) => a + j[1], 0)),
        jsonMax: r2(Math.max(0, ...h.json.map((j) => j[1]))),
        busyPct: r2((busy / (wall / 1000)) * 100),
        ...ns,
      }
      out.sweeps.push(row)
      console.log(
        `${vname.padEnd(8)} z${String(z).padEnd(3)} ${row.phase}${LATE ? ' late' : ''}  hover p50/p95/max ${row.hoverP50}/${row.hoverP95}/${row.hoverMax} ms (${row.hovers}) | setFilter ${row.setFilterN}x ${row.setFilterMs} ms (max ${row.setFilterMaxMs}) | renders ${row.reactRenders} | frames>25 ${row.over25} >50 ${row.over50} max ${row.maxFrame} | long ${row.longTasks} (${row.longMs} ms) | busy ${row.busyPct}% | shard reqs ${row.shardReqs} ${row.shardKB} KB ${row.shardProto} dur p50/max ${row.shardDurP50}/${row.shardDurMax} queue max ${row.shardQueueMax} | json parse ${row.jsonReads}x ${row.jsonMs} ms (max ${row.jsonMax}) | max in flight ${row.maxInflight}`
      )
    }
  }
}
await browser.close()
mkdirSync(new URL('./results', import.meta.url).pathname, { recursive: true })
const f = new URL(`./results/hover-${LABEL}-${new Date().toISOString().replace(/[:.]/g, '-')}.json`, import.meta.url).pathname
writeFileSync(f, JSON.stringify(out))
console.log('saved', f)
