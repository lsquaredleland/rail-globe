// Dev probe, not a benchmark: tied to macOS Chrome (the executablePath below) and the Chengdu start view (104.14, 30.63).
// node probe.mjs [latencyMs] [url] [dumpStyleGroups]: per zoom 6-14, settle time and loaded/in-view world tiles.
import puppeteer from 'puppeteer-core'
const lat = Number(process.argv[2] || 0), url = process.argv[3] || 'http://localhost:4173'
const b = await puppeteer.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true, defaultViewport: { width: 1440, height: 900, deviceScaleFactor: 2 }, args: ['--enable-gpu-rasterization', '--ignore-gpu-blocklist'] })
const p = await b.newPage(); const c = await p.createCDPSession()
await c.send('Network.enable')
const cond = (cl) => cl.send('Network.emulateNetworkConditions', { offline: false, latency: lat, downloadThroughput: 50 * 125000, uploadThroughput: -1 })
if (lat) { await cond(c); p.on('workercreated', async (w) => { try { await w.client.send('Network.enable'); await cond(w.client) } catch {} }) }
const reqT = {}
c.on('Network.responseReceived', (e) => { if (e.response.url.includes('.pmtiles')) (reqT.p = reqT.p || []).push(Math.round(e.response.timing ? e.response.timing.receiveHeadersEnd : -1)) })
await p.goto(url + '/?debug', { waitUntil: 'load' }); await p.waitForFunction('window.__map && window.__map.loaded()', { timeout: 120000 })
console.log('lat', lat, 'initial pmtiles receiveHeadersEnd ms (first 8):', (reqT.p || []).slice(0, 8))
if (lat === 0 || process.argv[4]) {
  console.log(await p.evaluate(`(() => { const m = window.__map, L = m.getStyle().layers, g = {}; for (const l of L) { const k = (l.source || 'none') + '/' + l.type + (l.id.includes(':') ? '' : ''); (g[k] = g[k] || []).push(l.id) } return JSON.stringify(Object.fromEntries(Object.entries(g).map(([k, v]) => [k, v.length + ' ' + v.slice(0, 4).join(',')])), null, 1) + '\\nsources ' + Object.keys(m.getStyle().sources) })()`))
}
await p.evaluate('window.__idle = () => new Promise((r) => { const m = window.__map; if (!m.isMoving() && m.loaded()) return r(); m.once("idle", r); m.triggerRepaint() })')
const rows = []
for (const z of [6, 7, 8, 9, 10, 11, 12, 13, 14]) {
  const t0 = Date.now()
  await p.evaluate(`window.__map.jumpTo({ center: [104.14, 30.63], zoom: ${z} })`)
  await p.evaluate('window.__idle()')
  const dt = Date.now() - t0
  const s = await p.evaluate(`(() => { const m = window.__map, o = { z: ${z} }; const st = m.style, kk = Object.keys(st).filter(k => /tileManager|sourceCache/i.test(k)); const sc = st[kk[0]] || {};     for (const [id, c] of Object.entries(sc)) { if (id !== 'world' && id !== 'ofm') continue; const tiles = Object.values(c._inViewTiles ? c._inViewTiles.getAllTiles?.() || c._tiles : c._tiles); o[id] = tiles.filter(t => t.state === 'loaded').length + '/' + tiles.length }
    const L = m.getStyle().layers, rail = L.filter(l => l.source && l.source !== 'ofm' && l.type !== 'background').map(l => l.id)
    o.railRendered = m.queryRenderedFeatures({ layers: rail.filter(i => m.getLayer(i)) }).length
    o.ofmRendered = m.queryRenderedFeatures({ layers: L.filter(l => l.source === 'ofm').map(l => l.id) }).length
    return o })()`)
  s.settleMs = dt; rows.push(s)
}
for (const r of rows) console.log(JSON.stringify(r))
await b.close()
