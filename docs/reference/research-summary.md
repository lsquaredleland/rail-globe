# Research summary (reference only)

Key measurements from the original research (planet of 2026-09-28, measured 2026-10-02). The full notes stay in the Rail Maps doc. Where anything here conflicts with `00-overview.md`, the overview wins.

## Inputs

- Planet extract `planet-260928` (95 GB) and a rail-only slice `railways-world` (671 MB)
- City extracts already built: Seoul, Paris, London, New York, Shanghai (JSONL and small PBFs)

## Build times (one laptop)

| Step | Tool | Wall time | Notes |
| --- | --- | --- | --- |
| Planet → 5 city extracts | osmium extract, one config file | 11.5 min | Reading the planet dominates. One pass serves all regions |
| Rail JSONL, 5 cities | pyosmium, 5 processes | 3 s | |
| World rail stats + GeoJSON export | pyosmium, 1 process | ~4 min | 1.8 GB of GeoJSON lines |
| World tiles z0–14 | tippecanoe | 43 min | Mostly one core busy; low zooms retry while dropping features |
| Merged overview z0–8 | tippecanoe | 7 min | Merged features, few retries |

Speed-ups: split overview and detail and `tile-join` them; per-feature `"tippecanoe": {"minzoom": 13}` on yards, sidings, spurs; trim attributes; develop on city extracts; parallel export by continent.

## World tile sizes

| Variant | File | Paris view z5 | z8 | z11 | z14 |
| --- | --- | --- | --- | --- | --- |
| Full (all track, all attributes) | 1,147 MB | 1,896 KB | 523 KB | 294 KB | 46 KB |
| Lean (main line, metro, light rail) | 661 MB | 1,883 KB | 236 KB | 152 KB | 36 KB |
| Merged overview (z0–8, `--coalesce`) | 22 MB | 668 KB | 78 KB | | |

A "view" is one 1440×900 screen (6–12 tiles at 512 px), compressed bytes.

- Zoomed in (z11–14) is already fast: 30–300 KB per view. Yards and sidings cost file size, not speed.
- Zoomed out (z0–7) is the real problem: OSM splits track into tiny pieces (a Paris z5 tile held up to 44,000 features). Merging pieces with equal attributes cut that to 3 features and the view from 1.9 MB to 668 KB. Fix low zoom by merging, not deleting.
- Request latency: tens of ms from Cloudflare's edge cache, 500 ms or more on a miss back to R2.

## What the rail data contains (world)

| Category | Ways | Vertices | Length |
| --- | --- | --- | --- |
| Active main line | 1,455,018 | 16.9 M | 1.59 M km |
| Metro | 40,656 | 0.65 M | 47 k km |
| Light rail, tram, monorail, funicular | 135,037 | 1.88 M | 41 k km |
| Yard tracks | 615,960 | 5.00 M | 209 k km |
| Spurs | 463,639 | 4.87 M | 210 k km |
| Sidings | 350,257 | 2.94 M | 170 k km |
| Railway land-use areas | 92,065 | 3.59 M | |
| Platforms | 189,314 | 1.52 M | |

Of active main line: `usage=main` 71% of vertices, `usage` present on 96%, `highspeed=yes` 4.7%, maxspeed missing on 41%.

## OSM tag coverage

| Object | Well covered (>85%) | Partly | Rare (<20%) |
| --- | --- | --- | --- |
| Active track | gauge 97%, electrified 96%, usage 88% | operator 72%, name 68%, maxspeed 64%, voltage 61% | number of tracks 7% |
| Stations (97,053) | name 99%, public_transport 88% | wikidata 64%, operator 63%, English name 33% | UIC ref 20%, platform count 4% |
| Metro lines | name, ref, colour 97%, stop list 97% | operator 83% | interval 16% |
| Train services | name 98%, operator 92%, stop list 94% | colour 42%, wikidata 28% | interval 12% |
| Tram lines | name, ref 96%, stop list 94% | colour 55% | interval 9% |

Relations: 18,344 `route=train`, 2,606 subway, 5,116 tram, 1,054 light rail, 9,450 `route_master`, 16,438 `route=railway` (infrastructure), 18,157 `stop_area`.

China: 26,104 stations and 2,514 physical lines, but only 530 `route=train` relations (65 with a train number). Intercity services are thin; metro is well mapped.

## Hosting free tiers (checked 2026-10-02)

| Service | Free limit | Notes |
| --- | --- | --- |
| Cloudflare R2 | 10 GB storage, 1M Class A, 10M Class B ops, free egress | `r2.dev` URLs are rate-limited, for development |
| Cloudflare Pages | 25 MiB per file, 20,000 files | Too small for a world PMTiles file |
| GitHub Pages | 1 GB site, 100 MiB per file | Fine for the app shell |
| OpenFreeMap | No stated limit | No key; commercial use allowed |

## Zoom and pan benchmark baseline

Measured 2026-10-13 on the production build, headless Chrome 155 at 1440×900, device scale 2, commit 96bc40f (working tree not clean: bench files only). Step 1 of `.omc/plans/zoom-pan-performance.md`: numbers only, no app change.

Full bench guide (all flags, scenarios, pitfalls): `app/bench/README.md`.

Run it:

```
cd app && npm run build && npx vite preview --port 4173   # separate shell
node bench/zoom-pan.mjs --runs 3 --throttle 6 --label baseline-t6
```

Flags: `--url` (default `http://localhost:4173`), `--runs` (default 3), `--throttle` (CPU slowdown, default 6, 1 = none), `--warmup` (unmeasured runs first, default 1), `--label`, `--chrome` (path to Chrome). Raw results go to `app/bench/results/` (git-ignored). The JSON holds every frame interval and long task, plus the environment, so runs can be re-scored later.

What it does:

- Views are China only: Chengdu East, Shenzhen, Guangzhou, Beijing, Shanghai. China has the densest network, and all five draw from the single `china.pmtiles`, so tile source cost is the same for every view.
- Each view starts at z9, then flies to z14 (2.5 s), pans one screen east (1.5 s), eases out to z8 (2.5 s). Durations are fixed so runs do identical work.
- One warm-up run comes first and is not measured. The basemap comes from the internet, so the warm-up fills the HTTP cache. Settle still includes some network time.
- CPU throttle slows only the main thread, not MapLibre's workers. Tile-side changes (worker parse, tile size) show mainly in settle time, not in frame times.

How to read the numbers:

- Frame intervals are whole 60 Hz frames (16.7 ms, 33.3 ms, and so on). That is why percentiles are not used: p50 and p95 are always one frame.
- Missed frames is the main metric: for each interval, round(ms / 16.67) − 1, summed. A 50 ms interval counts as 2.
- Dropped is the count of intervals over 25 ms (1.5 frames). Max frame is the longest interval. Jank ms is the sum of time over 16.67 ms per interval.
- Settle is the time from the end of a move until the map is idle (0 if it already is).
- Numbers are the median of 3 runs. Spread is (max − min) / median of the per-run totals.

Throttle 6x (about 179 s, 1,874 frames):

| View | Missed in / pan / out | Missed total | Dropped >25 ms | Max frame ms | Jank ms | Settle in / pan / out ms |
| --- | --- | --- | --- | --- | --- | --- |
| Chengdu East | 22 / 0 / 5 | 26 | 14 | 100.1 | 442 | 142 / 0 / 0 |
| Shenzhen | 16 / 0 / 4 | 19 | 14 | 66.6 | 326 | 196 / 0 / 0 |
| Guangzhou | 20 / 0 / 4 | 25 | 19 | 66.7 | 425 | 208 / 0 / 0 |
| Beijing | 12 / 0 / 2 | 14 | 12 | 50.0 | 243 | 154 / 0 / 0 |
| Shanghai | 14 / 0 / 3 | 17 | 14 | 50.1 | 293 | 197 / 0 / 0 |
| Total | | 103 | 76 | 100.1 | 1,761 | 892 summed |

Per-run totals: missed 106, 100, 103 (spread 6%); jank 1,812, 1,712, 1,761 ms.

No throttle (about 163 s, 1,974 frames):

| View | Missed in / pan / out | Missed total | Dropped >25 ms | Max frame ms | Jank ms | Settle in / pan / out ms |
| --- | --- | --- | --- | --- | --- | --- |
| Chengdu East | 0 / 0 / 0 | 0 | 0 | 16.8 | 10 | 64 / 0 / 0 |
| Shenzhen | 0 / 0 / 0 | 0 | 0 | 16.8 | 11 | 81 / 0 / 0 |
| Guangzhou | 0 / 0 / 0 | 0 | 0 | 16.8 | 10 | 83 / 0 / 0 |
| Beijing | 0 / 0 / 0 | 0 | 0 | 16.8 | 10 | 82 / 0 / 0 |
| Shanghai | 0 / 0 / 0 | 0 | 0 | 16.8 | 10 | 83 / 0 / 0 |
| Total | | 1 | 1 | 33.3 | 67 | 393 summed |

Per-run totals: missed 1, 1, 0 (one 33 ms frame in two runs); jank 67, 68, 49 ms.

- Unthrottled, the map is at 60 fps apart from one stray frame per run. Use throttle 6 to compare changes.
- Under throttle, nearly all misses are in the zoom-in (z9 to z14). The pan and zoom-out are clean.
- Total missed frames (103, spread 6%) and jank ms are the numbers to compare. Single views are noisier (spread up to 19%).
- Settle in the pan and zoom-out is 0 because the map is already idle when the move ends (tiles are cached). Settle after the zoom-in is the number that reflects tile loading.

### One tile source (step 4)

Measured 2026-10-13, same bench and flags as the baseline above (5 China views, 3 runs, 1 warm-up, throttle 6x), after joining the pilot regions into one source. Machine load average was 2.3-3.4, higher during the after runs, so the gain is if anything understated.

| Throttle 6x | Before | After |
| --- | --- | --- |
| Missed frames (all views) | 128 | 76 (-41%) |
| Jank ms | 2178 | 1314 |
| Max frame ms | 133 | 83 (Chengdu East) |
| Zoom-in settle, summed ms | 952 | 852 |
| Run spread | 30% | 11% |

Missed frames per view, before to after: Chengdu East 36 to 22, Shenzhen 19 to 12, Guangzhou 26 to 17, Beijing 19 to 12, Shanghai 25 to 15. At throttle 1x both runs missed 0 frames.

The plan's target of 50% fewer missed frames (64 or fewer) was not reached. Nearly all remaining misses are in the zoom-in.

The bench's refresh-halving guard is now a control: it takes the median rAF interval of an idle page, rather than testing each phase.

## Fast zoom: profile and comparison (2026-10-13)

How to run the bench and read its output: `app/bench/README.md`. These are the measurements.

### Headless, 60 Hz, fast scenario (ours)

- Throttle 1x: 0 missed frames in every view.
- Throttle 6x: 18 missed frames in total (Chengdu East 4, Shenzhen 2, Guangzhou 4, Beijing 5, Shanghai 3), max frame 66.7 ms, jank 379 ms. 88% of the misses are in the zoom-in half.
- The map is idle about 420 ms after the last wheel tick. 0 failed tile requests.
- At 6x, 5-20% of pmtiles requests and up to 35% of OpenFreeMap requests are aborted (MapLibre's `cancelPendingTileRequestsWhileZooming`, on by default).

### Profile at 6x (Guangzhou, unminified build, traced)

- Main thread busy 8.3 s of 11.4 s. Shares of that: `_render` 61%, GL driver / native (program) 27.5%, `Map._updatePlacement` 24.3%, painter render 22%, `renderLayer` 16%, `_updateSources` 5.9%, GC 2.1%, paint recalculate 1.7%.
- Worker busy 6%, no backlog. GPU process about 5%.
- So the main thread is the limit when zooming warm, not tiles.

### Toggles at 6x (missed frames, from 18)

| Change | Missed |
| --- | --- |
| `fadeDuration` 150 | 14 (placement 24% to 8% of busy time; traced misses 42 to 17) |
| `fadeDuration` 300 | 14 |
| Hide `hl-fs` layers | 15.5 |
| `cancelPendingTileRequestsWhileZooming` off + `maxTileCacheZoomLevels` 8 | 14.5 |
| The three above combined | 11 |
| Hide the basemap | 6.5 |
| Hide rail labels | 14 |
| Only `pilot:track*` visible | 0 |
| `setPixelRatio(1)` | 14 |

- `maxTileCacheSize` in MapLibre 6.12 can only lower the cache (`Math.min` with a view-dependent size). That explains the earlier no-gain result. `maxTileCacheZoomLevels` is the knob that raises it. MapLibre has no `prefetchZoomDelta`.
- Our `fadeDuration: 0` (`MapView.tsx`, since the initial commit, never a recorded decision) forces full, unpaused symbol placement every frame in MapLibre 6.12 (`maplibre-gl-dev.mjs` `forceFullPlacement` at line 15655; placement never pauses at line 9248).

### Headed, 120 Hz, against trains.theojaffee.net

Fast scenario on a ProMotion display: measured control 8.3 ms, DPR 2, viewport 1440×757. Ours is the production build with tiles from localhost. Results are in `app/bench/results/ours-120-*` and `ref-120-*` (git-ignored).

| | Throttle 1x ours | Throttle 1x ref | Throttle 4x ours | Throttle 4x ref |
| --- | --- | --- | --- | --- |
| Missed frames | 5 | 23 | 52 | 250 |
| Max frame ms | 17 | 50 | 59 | 132 |
| p95 ms | 9.5 | 9.0 | | |
| p99 ms | 10.2 | 10.2 | 10.4 | 17.9 |
| Jank ms | 739 | 744 | 2253 | 3923 |
| Long tasks | | | 0 | 14 (~1.2 s) |

Missed per view at 4x, ours / reference: Chengdu East 19 / 86, Shenzhen 8 / 43, Guangzhou 6 / 54, Beijing 10 / 32, Shanghai 9 / 35.

Fairness caveats:

- The reference clamps at z17 and ends about 0.55 levels higher.
- Its rail tiles come over the network; ours come from localhost.
- Its 4x run had no warm-up.
- 66 style layers on ours against 57 on the reference.

### Reference site teardown

From its published files:

- MapLibre 5.24, self-hosted, on GitHub Pages. 3 sources, `fadeDuration` 150.
- Rail is gzipped MVT packed into chunk files, read by HTTP Range through `addProtocol` (several zoom levels per chunk, LRU caches).
- Rail tiles z0-12, overzoomed above. Sampled tiles at most 36 KB. About 110.6 MB gzipped for the world.
- OpenFreeMap vector basemap with heavy `minzoom`.
- Latin glyph ranges self-hosted; CJK drawn with `localIdeographFontFamily`.

### Our tile sizes

Re-gzipped samples from `pilot.pmtiles`, dense Chinese cities. Largest tile per zoom: z3 261 KB, z4 218, z5 183, z6 94, z7 63, z8 165, z9 174, z10 159, z11 157, z12 89, z13 46, z14 36 KB. Medians are 2-39 KB.

### Tile and style zoom mismatches

Data sits in tiles a zoom before the style draws it (`track_minzoom` and `feat` calls in `pipeline/rail-extract/src/tiles.rs` against `minzoom` in `app/src/mapStyle.ts`):

| Layer | In tiles from | Drawn from |
| --- | --- | --- |
| Strands | z11 | z12 |
| Station grounds, yard areas | z10 | z12 |
| Buildings | z12 | z13 |
| Platforms | z13 | z14 |
| Yard track | z12 | z13 |

### Not measured yet

- The user's own testing was on localhost, probably the dev server on port 5173, which is slower than the production build measured here.
- First visit with a cold cache and a throttled network.
- Trackpad pinch input (the bench sends wheel ticks).
