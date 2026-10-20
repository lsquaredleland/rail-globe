import { defineConfig, type Plugin } from 'vite'
import react from '@vitejs/plugin-react'
import { createReadStream, readFileSync, statSync } from 'node:fs'
import { extname, join, normalize, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const DATA = fileURLToPath(new URL('./public/data', import.meta.url))
const TYPES: Record<string, string> = { '.json': 'application/json', '.geojson': 'application/geo+json', '.pmtiles': 'application/octet-stream' }

/** Dev server: serve public/data straight from disk, with byte ranges (PMTiles reads ranges). Vite only serves public files it
 *  has seen through its watcher, and public/data is not watched (below), so files the pipeline adds would otherwise 404. */
function serveData(): Plugin {
  return {
    name: 'serve-data',
    configureServer(server) {
      server.middlewares.use('/data', (req, res) => {
        const path = normalize(join(DATA, decodeURIComponent((req.url ?? '/').split('?')[0])))
        let size = -1
        try {
          const st = statSync(path)
          if (st.isFile() && path.startsWith(DATA + sep)) size = st.size
        } catch { /* missing: 404 below */ }
        if (size < 0) { res.statusCode = 404; res.end(); return }
        res.setHeader('Content-Type', TYPES[extname(path)] ?? 'application/octet-stream')
        res.setHeader('Accept-Ranges', 'bytes')
        res.setHeader('Cache-Control', 'no-cache')
        const m = /^bytes=(\d*)-(\d*)$/.exec(req.headers.range ?? '')
        if (!m) { res.setHeader('Content-Length', size); createReadStream(path).pipe(res); return }
        const start = m[1] ? Number(m[1]) : Math.max(0, size - Number(m[2]))
        const end = m[1] && m[2] ? Math.min(Number(m[2]), size - 1) : size - 1
        if (start > end || start >= size) { res.statusCode = 416; res.setHeader('Content-Range', `bytes */${size}`); res.end(); return }
        res.statusCode = 206
        res.setHeader('Content-Range', `bytes ${start}-${end}/${size}`)
        res.setHeader('Content-Length', end - start + 1)
        createReadStream(path, { start, end }).pipe(res)
      })
    },
  }
}

/** MapLibre 6 runs its tile worker from two ES module files that import each other by relative name. A bundler emits neither, and
 *  hashing would break the import, so production builds copy both, unhashed, to maplibre/ (MapView.tsx points the worker there). */
function maplibreWorker(): Plugin {
  return {
    name: 'maplibre-worker',
    apply: 'build',
    generateBundle() {
      for (const f of ['maplibre-gl-worker.mjs', 'maplibre-gl-shared.mjs']) {
        this.emitFile({ type: 'asset', fileName: `maplibre/${f}`, source: readFileSync(fileURLToPath(new URL(`./node_modules/maplibre-gl/dist/${f}`, import.meta.url))) })
      }
    },
  }
}

export default defineConfig({
  plugins: [react(), serveData(), maplibreWorker()],
  base: './',
  // static/ (logos) is the only thing copied into dist: public/data (the pipeline's output, GBs) is served by serveData in dev and by R2 in production
  publicDir: 'static',
  // public/data holds the pipeline's tiles and tens of thousands of side files: served by serveData above, but not watched,
  // so a pipeline run does not keep the dev server busy reacting to every file it writes
  test: { setupFiles: ['./src/test-setup.ts'] },
  server: { watch: { ignored: ['**/public/data/**'] }, fs: { allow: ['..'] } },   // fs.allow: ../shared/constants.json lives outside app/
})
