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
