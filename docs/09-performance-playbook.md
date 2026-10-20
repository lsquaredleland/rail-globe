# Performance playbook: what made the map fast (2026-10-14/15)

Written after moving tiles from tippecanoe + MVT to a Rust MLT writer and fixing the zoom and hover jank. Read this before the next tile or rendering performance project. The raw benchmark files and scripts (orig-vs-mlt, mvt-vs-mlt, a progress log) were kept locally only and are not in the repo; the key numbers are in the table below and in the dated sections of `docs/archive/open-questions-2026-10-20.md`.

## Result (final MLT app vs the original commit 913ddff, MVT)

| Metric | Original | Final |
| --- | --- | --- |
| China tile step | 31.5 s | 4.1 s |
| Full 7-region build | 88.4 s | 27.4 s |
| Fast zoom, headed 120 Hz, throttle 4, missed frames (6 runs) | 54 | 14.5 |
| Longest frame while zooming | 98 ms | 32 ms |
| Click latency z9 | 43 ms | 4.6 ms |
| Tile bytes over 30 standard views | 6.0 MB | 4.5 MB |
| Pilot file | 343.7 MB | 298.8 MB |
| Map `load` at 150 ms latency, cold | 1,653 ms | 1,094 ms |

Where the original still wins: CJK glyph bytes (11x on China views), tile-build memory (5.3 GB vs 0.35 GB for China). See "Open problems".

## What actually made it fast, ranked by effect

1. **(Superseded 2026-10-17, the world build made the CJK files 1.3 to 1.8 MB per dense view: the app is now glyph-free, all text is drawn locally and the hitch below is accepted.) Self-hosted subset glyphs with `localIdeographFontFamily: false`.** MapLibre draws CJK glyphs on a canvas on the main thread when it uses the local font. That was a 90 ms stall at the first zoom of every page load and about a third of the JS in bad frames. A 3,288-character subset served as PBF took the quick bench from about 25 to 2 missed frames. This helps MVT too; it is not an MLT gain.
2. **No feature state on MLT sources.** In MapLibre 6.12 (unchanged in 6.14), any feature-state entry on a source makes `Tile.setFeatureState` call `loadVTLayers` on the main thread for every loaded tile. MVT parses lazily (about 0.07 ms per tile). MLT decodes the whole tile eagerly (about 0.66 ms per tile at z9-z11), and the CPU throttle multiplies it. MLT with feature state off matched MVT; removing it for good took the full bench from 69/72 to 41/40 missed frames.
3. **Highlight on its own small source.** Replacing feature state with `setFilter` on a layer of the main source made hover slow: a filter change reloads every tile of the source that layer reads, which throws away decoded tiles, and the next `queryRenderedFeatures` decodes each large overview tile again. The fix is a dedicated vector source for the highlight layer (same PMTiles URL, `minzoom: 9`; called `pilot:hls` when measured, `world:hls` now). z9 Europe hover went from 4.1 ms to 0.6 ms per move (original 0.9).
4. **Rust writer instead of tippecanoe.** China 31.5 s to 4.1 s. The gain comes from the writer, not the format: it would be as fast writing MVT. Cost is memory (see open problems).

## What did not matter (measured, rejected)

- MLT encoder options: FastPFOR, FSST, shared dictionaries, Morton/Hilbert/id sorting changed JS decode time by 4% or less. FastPFOR and FSST made gzipped tiles larger; with both off the pilot file is 3.8% smaller.
- Pre-tessellation: MapLibre runs earcut in the worker and never reads stored triangles; the file grew.
- MLT wire format v2: `@maplibre/mlt` 1.3.0 cannot decode it.
- Storing tiles uncompressed: gzip inflate is 0.03-0.1 ms per tile.
- Dropping 8 tooltip-only `track` columns: 11% smaller file, no speed gain.
- `fadeDuration` 150, `maxTileCacheZoomLevels`, `cancelPendingTileRequestsWhileZooming: false`, `refreshExpiredTiles: false`, `validateStyle: false`: all within noise. `antialias` is already false by default in v6.
- MapLibre 6.12 to 6.14: no measurable difference.
- Capping tile zoom (overzoom above z12): suggested early, but it does not touch a z6-z10 zoom gesture and was never needed.
- The published "MLT decodes about 3x faster than MVT" does not hold on MapLibre's JS path: decode plus geometry plus properties is about 2x slower at z9-z14 because it builds one property object per feature. The MLT gains we saw are size (7-13% smaller files, 15-35% lighter low-zoom views) and build time.

## Method lessons (do these first next time)

1. **Keep the original untouched as a running baseline** (a git worktree of the baseline commit, built and served on its own port). Early on the baseline files were overwritten and "MVT" copies drifted: one scratch MVT copy lacked `places.json` and boundary files and inflated the first MLT-vs-MVT gap; a later one inherited the new highlight code. Always compare the final candidate against the pristine original.
2. **Trace before tuning.** The two biggest wins (glyph canvas drawing, `loadVTLayers`) were found from Chrome traces split by main-thread category, then confirmed by turning the suspect off with `--eval` (no code change). Layer-hiding A/Bs showed the basemap, labels and rail drawing were not the problem.
3. **The bench must reflect real interaction.** The fast-zoom bench never hovers, so it could not see the hover regression. Hover cost needs its own per-mousemove sweep (`hv.mjs` idea: instrument the handler, sweep rail lines at z3-z12, count frames over 25 ms).
4. **Use the right bench settings.** Headed Chrome on the ProMotion display (120 Hz, budget 8.3 ms), production build via `vite preview`, CPU throttle 4, wheel delta 127, one warm-up, alternate A/B/A/B to cancel load drift, note the load average, repeat a condition when repeats differ by more than 25%. Quick preset (2 hardest views, 2 runs) for screening, full five views to confirm. Headless 60 Hz hides 120 Hz overruns.
5. **Add network latency before judging.** On localhost tiles arrive instantly, so every tile's parse and upload lands mid-gesture; latency (50 and 150 ms RTT, about 50 Mbps) moves that work after the gesture and lowers missed frames while raising blank-tile time. Judge smoothness and settle time together. `zoom-pan.mjs --latency/--down/--tilestate` do this.
6. **Check equal work first.** Before blaming the format, compare feature, vertex and property-column counts per tile and per view between the builds (`querySourceFeatures`). Here they matched at z9-z14.
7. **Do not trust one number.** The dev server (`vite dev`) is slower than `vite preview`; `omc ralph verify` counts passing test lines as new failures when the baseline had no tests; the bench's `encodedDataLength` counts cache hits as zero.

## MLT-specific notes for this stack

- MapLibre needs `encoding: 'mlt'` on the vector source; it does not detect the format. pmtiles 4.5 knows `TileType.Mlt` (6) but does not set the encoding.
- `mlt-core` 0.20.4: use `LayerWriter`, then `encode(EncoderConfig)`. Use `.with_fastpfor(false).with_fsst(false)`. Polygon rings are re-wound by the encoder. One type per property per layer: normalise per layer (ints that fit i32 stay i32, int plus float becomes f64, anything beyond i32 becomes f64 to avoid BigInt in the JS decoder, other mixes become strings).
- `geojson-vt-rs` drops any line shorter than its tolerance and any ring below tolerance squared; tippecanoe keeps them. Slice those features in a separate near-zero-tolerance group, or ids go missing (396 ids at z9 in the SF test).
- Tippecanoe's overview merge is not reproduced by slicing alone: join touching lines end to start, otherwise z3-z8 tiles are 35-50% larger.
- Memory: the writer holds a whole region in memory (5.3 GB for China, 9-14 GB for the pilot). Slice one region at a time and cut very large feature groups into runs of about 8,000; 2,000-feature runs blew up to 94 GB.
- Do not set feature state on MLT sources. Drive highlights from a separate source or a GeoJSON overlay.
- PMTiles over HTTP: a second source on the same PMTiles URL issued its byte ranges again (every range twice) because pmtiles does not cache tile bytes. Fixed by `dedupedPmtiles` in `MapView.tsx` (see results).

## Results after the duplicate-fetch fix (2026-10-15)

`MapView.tsx` wraps the pmtiles protocol handler (`dedupedPmtiles`): tile requests are shared while in flight (aborted only when every asker aborts) and finished tiles stay in a 32 MB LRU; each caller gets its own copy, so MapLibre transferring the buffer cannot detach the cached one. pmtiles caches directories but not tile bytes, which is why the main source and the highlight source fetched every range twice (measured as `pilot` and `pilot:hls`; they are `world` and `world:hls` now). The highlight source is `world:hls`.

| Check | Previous MLT build | Fixed |
| --- | --- | --- |
| pilot.pmtiles requests, cold load, Chengdu z10 / SF z11 | 26 (12 duplicates) / 20 (9) | 14 (0) / 11 (0), the original's 14 / 11 |
| Last request finished, 0 ms, Chengdu z10 / SF z11 | 2,580 / 2,156 ms | 1,769 / 1,127 ms (original 2,602 / 1,192) |
| Last request finished, 150 ms + 50 Mbps | 6,846 / 5,521 ms | 6,462 / 4,376 ms (original 6,066 / 4,868) |
| Fast zoom, headed 120 Hz, throttle 4, delta 127, Chengdu East + Shenzhen, missed (2 ABAB pairs) | 12.5, 6 | 9, 6.5 |
| pmtiles requests per zoom run (2 views) | 238 / 343 | 72 / 86 |
| Hover ms per move z3 / z9 / z12 (China) | 1.21 / 0.44 / 0.62 | 1.18 / 0.49 / 0.51 |
| Click z6 / z12, screenshots z9 / z12 / z13.5 | | identical |

Chengdu at 150 ms is still 6% slower than the original (6,462 vs 6,066 ms, one run each, glyph files included); SF is 10% faster. Repeat on an idle machine (load average was 5) before calling Chengdu a loss.

## Open problems (do these next)

1. **Glyph download size (closed 2026-10-17: no glyph files at all, see docs/00 row 2026-10-17 and docs/archive/performance-todos-2026-10-17.md; the numbers below are the old pilot set).** Exists because self-hosted glyphs removed the 90 ms main-thread canvas stall; the local CJK font is free to download but costs main-thread time. Measured cold: Chengdu z12 78 files / 1,615 KB, Chengdu z10.2 32 / 982 KB, Beijing z11.5 82 / 1,693 KB, Paris 5 / 78 KB, SF 3 / 73 KB (headless, our /glyphs/ files only). Measured offline: gzip takes 32% off a range file (2.40 MB to 1.63 MB per stack); dropping code points used by one label only saves 21% of disk and loses a glyph in 603 of 117,165 distinct labels (99.96% of characters kept); Bold needs only 17 non-CJK files if its CJK ranges point to the Regular files via `transformRequest`. China views fetch about 1 MB across 32-43 glyph files (both Noto Sans Regular and Bold ranges; gzip saves only about 3%), against 87 KB for the original, adding 18-23% to cold-load bytes. Paris and SF are lighter than the original. Ideas: serve CJK from one weight, prune the 3,288-character subset by frequency (ship the common few hundred first, the rest on demand), immutable cache headers, check `.pbf` compression on the real host (Cloudflare), consider a pre-composed single file per script. Re-measure bytes and settle time after.
2. **Tile-build memory** (5.3 GB for China vs 0.35 GB): per-top-level-tile slicing or a streaming reader before building anything larger than the Americas.
3. **Europe z5/z6 weight** is still over the 600 KB view budget (923 KB worst); needs thinner overview tiles, not a format change. Update 2026-10-17: the writer now dedupes repeated vertices, simplifies joined overview lines (Douglas-Peucker 4 units) and writes z0 to z8 point and line layers with extent 2048, without stations in z0 to z5 tiles; on the 14-country subset z0 to z8 shrank 54% and Europe z5 laptop 1,264 to 515 KB (`16-low-zoom-tile-weight-plan.md` section 9). The world file has to be rebuilt to get it.
4. **Zoom out to the world** was a tie with 120-170% run-to-run spread; repeat on an idle machine. The overview tiles have 1.5-1.7x the vertices of tippecanoe's at z6-z8 because of our line joining.
5. **Headed hover and click sweeps** were one run per app; repeat when the machine is idle.

## Checklist for the next performance project

1. Worktree of the baseline on its own port; never edit it.
2. Reproduce with the bench the user feels (add the missing interaction: hover, pinch).
3. Chrome trace, split main-thread time by category; confirm each suspect by disabling it (no code change).
4. Check for cheap main-thread work that scales with tile count (feature state, `queryRenderedFeatures`, glyph canvas).
5. Fix the biggest cause, A/B with the quick preset, confirm with the full bench, add latency.
6. Record what did not work in this file so it is not tried again.

## Hover-lag investigation (2026-10-17)

User report: highlight lags when the mouse sweeps over many lines. Added `app/bench/hover-sweep.mjs` (scripted sweep over rail points, `jump` visits a different line every move; per sweep: handler ms p50/p95/max, `queryRenderedFeatures` ms, `setFilter` count and ms, React DOM batches, frames over 25/50 ms, long tasks, busy %, side-file reads; `--profile` saves a CPU profile, `--fresh --latency` starts with cold shard files, `--pre` runs a toggle in the page). Production builds served from a scratch copy on their own ports, headless, throttle 4, 300 moves at 10 ms.

- Current tree, world source, Paris / Chengdu / Europe at z6, 8, 10, 12 (quiet machine): handler p50 1.4-2.9 ms, p95 2.2-5.9 ms (throttle 4, so about a quarter of that for real), long tasks 0, frames over 25 ms 0-4 of ~300, one `setFilter` per highlight change at 0.8-1 ms (filter literal up to 70 KB of way ids). Commit 0da274e^ (pilot source) gives the same numbers, so the world source and the sharded side files did not change hover cost. The design of docs/06 (2026-10-15) still holds: the highlight filter is on `world:hls`, no feature state, no setFilter on the main source.
- The big swings seen in earlier runs (frames over 25 ms from 0 to 100 for the same build) came from machine load (load average 20-30 during the work); compare only interleaved ABAB runs on an idle machine.
- Where hover time goes (CPU profile, Paris z8, unminified): map `_render` 29%, `receive` (worker replies after a highlight change) 10%, `highlightLine` 5% (almost all `setFilter`: serialising all layers, validating and cloning the 55 KB literal), React `TypeCard` re-render on every hover frame 4%, `hitAt` 4%. Turning off the `world:hl` GeoJSON overlay (`setData` stubbed) lowers busy from 66-83% to 54-60% at z8 and does nothing at z12, so the overlay is a real but z<9-only cost.
- Not reproduced: a cold shard read. In the bench the idle prefetch had loaded the shards before the sweep. On the real host a hover on a line whose shard is not loaded waits for a fetch of up to 218 KB (`lines`) or 465 KB (`name-hl`, uncompressed on r2.dev) before the highlight appears; before the 2026-10-16 repack a line was one small file. That would read as lag when sweeping fast right after a pan. Untested; first check: Network panel on the live site while sweeping, and `hover-sweep.mjs --fresh --latency 50` with the prefetch disabled.
- Guard: run `node bench/hover-sweep.mjs --url http://localhost:4173` after any change to the hover path, the highlight layers, `data.ts` or App state that changes per mouse move; expect handler p95 under 6 ms at throttle 4 and no long tasks.

### Root cause found: the idle prefetch froze the main thread (fixed in 6fe306f and ce2661e)

The first sweep above (no regression) never saw it because it hovered after the prefetch had finished. A sweep that starts as soon as the tiles are in (`hover-sweep.mjs --late --cold`, a user who pans and hovers at once) does:

- Cause: `prefetchHighlights` (`mapStyle/highlight.ts`) ran on every map idle at z6 and closer and called `regionOf(f)` for every rendered track piece (thousands). On the world build the parts' bounds overlap (China and its neighbours), so `partAt` falls to `inRing` over country outlines with tens of thousands of vertices. CPU profile, Chengdu z10, unminified, throttle 4: `prefetchHighlights` 1,892 ms of 3,959 ms busy, `regionOf` 1,795 ms, `inRing` 1,705 ms self. JSON parse of the shards did not show in the profile.
- Numbers, throttle 4, cold browser, local server, ABAB twice: idle prefetch after a jump, Chengdu z10 longest long task 2,056 / 2,090 ms with prefetch, 241 / 249 ms with it disabled; main-thread busy 3.36 / 3.41 s against 1.17 / 1.21 s. Paris z10: 677 / 691 ms against 237 / 254 ms. Hover sweep started at once after the jump: Chengdu longest task 2,003 / 2,054 ms, Paris 553 / 593 ms (max frame 583 ms).
- Fix: `regionOf` once per ~5 km cell (not per piece), queued files keyed by region plus line id or name (ids are numbered per region), the view is marked prefetched only when the queue finished (a move mid-prefetch lets the next idle redo it), a generation counter stops old runners, prefetch two at a time and dropped when the map moves. After: Chengdu longest task 264 / 264 ms, Paris 229 / 244 ms in the idle test (prefetch disabled: 240 / 250); in the hover sweep 207 / 196 ms and 80 / 81 ms (disabled: 0).
- Still open: the prefetch loads 28 shards, 5.8 MB (Paris) to 6.2 MB (Chengdu), per view, and a hover sweep over many lines loads 20 to 23 shards (4.5 to 5 MB) on demand. On the live site (r2.dev, HTTP/1.1, no compression) a shard takes 270 ms median, 570 ms max at 50 ms RTT / 20 Mbit, and 28 of them share six connections with the tiles. Fix needs the pipeline: smaller shards or the highlight ways split from the line record. Until then the cost is bandwidth, not main-thread time.
- Guard: `node bench/hover-sweep.mjs --url <preview> --views Paris,Chengdu --zooms 10 --moves 250 --late --cold` (sweep right after a jump) and `--prefetch --cold` (the idle phase alone); the longest task should stay within 50 ms of a run with `prefetchHighlights` disabled.
- Border check (`--prefetch --cold`, Basel z10): shards queued for de, fr and ch, as before the fix; longest task 1,663 ms before, 233 ms after. Final numbers, `--late --cold` sweep, throttle 4, ABAB twice: Chengdu z10 longest task 180 / 189 ms, Paris z10 73 / 75 ms.

## maplibre-gl is pinned to 6.14.0

`app/package.json` pins `maplibre-gl` to the exact version 6.14.0 (no caret): label drawing from system fonts (the `text-font` stacks, first family carries the weight) is MapLibre's own code, so look at labels in a few scripts (Latin, Hangul, Arabic) before changing the pin.

## Glyph files against glyph-free (2026-10-17)

Decision: the app is glyph-free (`docs/00-overview.md`, 2026-10-17). Measured on production builds, headless Chrome 60 Hz, CPU throttle 4, 2 runs after 1 warm-up, load average 2.5 to 3.2. A = glyph files for non-CJK scripts plus local CJK; B = no `glyphs` property, all text local.

| | A (glyph files + local CJK) | B (glyph-free) |
| --- | --- | --- |
| Glyph bytes, world z2 | 21 files, 745 KB raw / 409 KB gzipped | 0 |
| Glyph bytes, Paris z11 / Cairo z9 (after the world view) | 43 KB / 102 KB gzipped | 0 |
| Chengdu East, zoom bench | missed 4, max frame 75 ms, 1 long task (82 ms) | missed 8.5, max frame 125 ms, 2 long tasks (185 ms) |
| Shenzhen, zoom bench | missed 1, max frame 33 ms, no long task | missed 3, max frame 66.6 ms, 1 long task (70 ms) |

Earlier history on the pilot set: local CJK font against the server's glyphs: quick bench 27.2 missed frames to 2.8 with server glyphs, Chengdu East 18 to 1.5, max frame 91 to 17 ms (`docs/06`); glyph files for everything against local CJK (A): 0 missed frames / 16.8 ms max frame before, 3 / 66.7 ms after.

Caveats: two runs, one noisy pair (the 125 ms in B), headless 60 Hz only, macOS fonts only (Helvetica, PingFang SC, Hiragino Sans, Apple SD Gothic Neo); Windows, Android and a headed 120 Hz run were not measured. Visual check: Latin (Paris), Hangul (Seoul) and shaped Arabic (Cairo) look the same in both; about five Paris labels collide differently. Re-measure the first-zoom hitch on real devices (`docs/11`).
