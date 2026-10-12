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
