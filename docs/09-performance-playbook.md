# Performance playbook: what made the map fast (2026-10-14/15)

Written after moving tiles from tippecanoe + MVT to a Rust MLT writer and fixing the zoom and hover jank. Read this before the next tile or rendering performance project. The raw benchmark files and scripts (orig-vs-mlt, mvt-vs-mlt, a progress log) were kept locally only and are not in the repo; the key numbers are in the table below and in the dated sections of `docs/06-open-questions.md`.

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

1. **Self-hosted subset glyphs with `localIdeographFontFamily: false`.** MapLibre draws CJK glyphs on a canvas on the main thread when it uses the local font. That was a 90 ms stall at the first zoom of every page load and about a third of the JS in bad frames. A 3,288-character subset served as PBF took the quick bench from about 25 to 2 missed frames. This helps MVT too; it is not an MLT gain.
2. **No feature state on MLT sources.** In MapLibre 6.12 (unchanged in 6.14), any feature-state entry on a source makes `Tile.setFeatureState` call `loadVTLayers` on the main thread for every loaded tile. MVT parses lazily (about 0.07 ms per tile). MLT decodes the whole tile eagerly (about 0.66 ms per tile at z9-z11), and the CPU throttle multiplies it. MLT with feature state off matched MVT; removing it for good took the full bench from 69/72 to 41/40 missed frames.
3. **Highlight on its own small source.** Replacing feature state with `setFilter` on a layer of the main source made hover slow: a filter change reloads every tile of the source that layer reads, which throws away decoded tiles, and the next `queryRenderedFeatures` decodes each large overview tile again. The fix is a dedicated vector source for the highlight layer (`pilot:hls`, same PMTiles URL, `minzoom: 9`). z9 Europe hover went from 4.1 ms to 0.6 ms per move (original 0.9).
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

`MapView.tsx` wraps the pmtiles protocol handler (`dedupedPmtiles`): tile requests are shared while in flight (aborted only when every asker aborts) and finished tiles stay in a 32 MB LRU; each caller gets its own copy, so MapLibre transferring the buffer cannot detach the cached one. pmtiles caches directories but not tile bytes, which is why `pilot` and `pilot:hls` fetched every range twice. `pilot:hls` is unchanged.

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

1. **Glyph download size (deferred by the user 2026-10-15, tracked as a must fix in docs/06).** Exists because self-hosted glyphs removed the 90 ms main-thread canvas stall; the local CJK font is free to download but costs main-thread time. Measured cold: Chengdu z12 78 files / 1,615 KB, Chengdu z10.2 32 / 982 KB, Beijing z11.5 82 / 1,693 KB, Paris 5 / 78 KB, SF 3 / 73 KB (headless, our /glyphs/ files only). Measured offline: gzip takes 32% off a range file (2.40 MB to 1.63 MB per stack); dropping code points used by one label only saves 21% of disk and loses a glyph in 603 of 117,165 distinct labels (99.96% of characters kept); Bold needs only 17 non-CJK files if its CJK ranges point to the Regular files via `transformRequest`. China views fetch about 1 MB across 32-43 glyph files (both Noto Sans Regular and Bold ranges; gzip saves only about 3%), against 87 KB for the original, adding 18-23% to cold-load bytes. Paris and SF are lighter than the original. Ideas: serve CJK from one weight, prune the 3,288-character subset by frequency (ship the common few hundred first, the rest on demand), immutable cache headers, check `.pbf` compression on the real host (Cloudflare), consider a pre-composed single file per script. Re-measure bytes and settle time after.
2. **Tile-build memory** (5.3 GB for China vs 0.35 GB): per-top-level-tile slicing or a streaming reader before building anything larger than the Americas.
3. **Europe z5/z6 weight** is still over the 600 KB view budget (923 KB worst); needs thinner overview tiles, not a format change.
4. **Zoom out to the world** was a tie with 120-170% run-to-run spread; repeat on an idle machine. The overview tiles have 1.5-1.7x the vertices of tippecanoe's at z6-z8 because of our line joining.
5. **Headed hover and click sweeps** were one run per app; repeat when the machine is idle.

## Checklist for the next performance project

1. Worktree of the baseline on its own port; never edit it.
2. Reproduce with the bench the user feels (add the missing interaction: hover, pinch).
3. Chrome trace, split main-thread time by category; confirm each suspect by disabling it (no code change).
4. Check for cheap main-thread work that scales with tile count (feature state, `queryRenderedFeatures`, glyph canvas).
5. Fix the biggest cause, A/B with the quick preset, confirm with the full bench, add latency.
6. Record what did not work in this file so it is not tried again.
