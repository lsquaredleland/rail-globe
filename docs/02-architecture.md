# Architecture

A static site. An offline build turns OSM into one PMTiles file plus small JSON files; the browser draws them with MapLibre. No server, rebuilt monthly.

## Stack

| Part | Choice | Notes |
| --- | --- | --- |
| Source | OSM planet, read once into a world rail cache (Rust, `pipeline/rail-extract`) | About 1 minute per planet |
| Filter and export | Rust: each region cut from the cache by its outline (countries and provinces / states from the planet's admin boundaries, or hand-made) | Exactly as `03-data-spec.md` says; under a second per region, all of China included |
| Processing | Python + shapely | PostGIS at build time only if corridor work needs it (phase 5) |
| Tiles | tippecanoe → PMTiles | One run: merged track overview (z0–8, `--coalesce`) and full detail (z9–14) in the same layers |
| Hosting | Cloudflare R2 for tiles, a static host for the app | `r2.dev` for the pilot; custom domain at public launch |
| Map | MapLibre GL JS | |
| Basemap | OpenFreeMap, restyled minimal | See `01-features.md` |
| App | Vite + React + TypeScript | |
| Services | OSM route relations only | No GTFS |

## Data layers

Infrastructure is built first and never depends on the other layers. Ranking and zoom come only from infrastructure.

| Layer | Holds | Built from |
| --- | --- | --- |
| Infrastructure | Track, station grounds, yards, platforms, station buildings | OSM ways and areas |
| Service | Lines (`route_master`) and their variants, with type, colour, operator | OSM route relations |
| Link | Which lines use which track; which stop points belong to which station | Route members, `stop_area` |
| Stations | Complexes and stations, ranked by size | All three (see `04-station-model.md`) |
| Region stats | Totals per preset region | Infrastructure and stations |

## Stable ids

Ids survive monthly rebuilds, so links, URL state and side files keep working.

| Object | Id |
| --- | --- |
| Track piece | OSM way id, e.g. `way/123` |
| Merged track (low zoom) | Hash of its sorted source way ids; the list is kept |
| Line | `route_master` id, else the route id |
| Station | `stop_area` id, else the station node id |
| Complex | `stop_area_group` id, else the id of its largest station |
| Region | Preset slug, e.g. `sf-bay-area` |

URL state uses these, e.g. `#compare=sf-bay-area,paris&color=type`.

## Build pipeline

1. **Cache** every rail candidate of the planet, with coordinates, in one read (`rail-extract cache`, once per planet).
2. **Cut and filter** each region from the cache to the data spec: GeoJSON features, the relations side table, the stop positions file, airports (`rail-extract cut <slug>...`).
3. **Classify** every track and line into a rail type (`01-features.md`, section 2).
4. **Build stations** with the station model.
5. **Build lines**: join each route's member ways into one feature per line.
6. **Compute region stats** for each preset.
7. **Tile** with tippecanoe and write side files.
8. **Publish** to R2 and the static host.

Also: a QA report per region per build (broken route relations, orphan stations, stops that map to no station), plus a diff against the previous build.

## Tile layers

The app reads one tile source, `pilot` (`pilot.pmtiles`). Per-region PMTiles files are its input: `rail-extract build` joins them with `tile-join` and writes each part's bounds to `pilot.parts.json`. Tiles carry no region field, so the app finds a feature's region from its position (the part whose bounds hold it) to pick side files. See `00-overview.md`, 2026-10-13.

| Layer | Geometry | Zooms | Key attributes |
| --- | --- | --- | --- |
| `track` (overview) | Merged lines, in the `track` layer (see `00-overview.md`, 2026-10-12) | z0–8 | type, con, prop, oc, kmh band, ln; nk (railway number) on track with no mapped line |
| `track` | Lines | z9–14 | id, type, usage, electrified, voltage, frequency, maxspeed, gauge, tracks, tunnel, bridge, construction, line ids |
| `con_draw` | Construction and proposed track merged end to end, per type, proposed flag and speed band. Draw-only (the dashes): no id, no tags; no yard track | z3–14 in tiles (drawn from z4; a type's own minimum zoom if later) | type, prop, kmh band |
| `lines` | One feature per line | z9–14 | line id, type, colour, ref, name |
| `areas` | Station grounds, yards | z12–14 | id, kind |
| `buildings` | Station buildings | z13–14 | id |
| `platforms` | Lines and polygons | z14 | id, ref |
| `airports` | Polygons: aerodrome outline (z8, z10 without an IATA code) and terminals (z12) | z8–14 | id, kind, name, name_en, iata, icao, area |
| `complexes` | Points | z5 and up | id, name, name_en, size tier, terminus, platform count, line count |
| `stations` | Points | z10 and up, z8 for top tiers | id, name, name_en, modes, size tier, terminus, platform count, line count by type, airport (when at a commercial airport) |

Side files: `lines/<id>.json` (with the line's highlight data: its track pieces' ids and a simplified geometry), `name-hl/<shard>.json` (the same for named railways, sharded by a hash of the name), `railways.json` (every railway name by number `nk`: name, English name, type, km), `stations/<id>.json`, `regions/<slug>.json`, search shards by name prefix (e.g. `search/pa.json`, queried with MiniSearch or similar), and one line index per region (`regions/<slug>/lines-index.json`: line id → name, colour, type, operator). The line index is loaded once so track tooltips can name the lines on a track without putting names in every track piece. Tooltip fields come from tiles plus the line index; click panels load the per-station or per-line JSON. Tiles carry `name` and an English name for labels and tooltips; other languages live in side files to keep tiles small.

## Zoom rules

| Zoom | Adds |
| --- | --- |
| 0–4 | High-speed and the busiest main lines, merged |
| 5–7 | All main lines, merged; top-tier station complexes |
| 8–9 | Branch, commuter, metro, light rail; lines become clickable |
| 10–11 | Tram; more stations |
| 12 | Station grounds and yard outlines; all stations |
| 13–14 | Yard track, sidings, spurs, crossovers, station buildings; platforms at 14 |

Construction track is shown from the same zoom as its rail type (an under-construction high-speed line shows from z0), not later. It is drawn as dashes at every zoom from z4, from `con_draw` (merged lines, so a dash pattern runs along a stretch within a tile). Hover, click and highlight use the per-way `track` features (`hit-con`, `hit-prop`), which the merged overview carries below z9, not `con_draw`.

Tiles include each class one zoom earlier than the style shows it, so zoom rules can be tuned in the style without rebuilding. Budget: under 600 KB per view on a mid-range phone.

## Hosting and cost

The pilot regions are a few MB; the full world build measured 1.1 GB. Both fit R2's free 10 GB. At public launch, a custom domain (about $10 a year) adds edge caching and removes the `r2.dev` rate limit. Free-tier limits were checked on 2026-10-02 and need checking again before launch.

**Deploy (to do later):** the app goes to Vercel from GitHub; the pipeline output (`app/public/data`: tiles and side files, 588 MB for the pilot) is gitignored and too big for git, so it goes to R2.
1. Sync `app/public/data` to the R2 bucket after each pipeline run (for example `rclone sync app/public/data r2:<bucket>`).
2. Add a CORS rule on the bucket: GET and HEAD, `Range` header allowed (PMTiles reads byte ranges).
3. Set `VITE_DATA_BASE` in the Vercel project to the bucket's public URL, with a trailing slash (see `app/.env.example`). Unset, the app reads its own `data/` folder.
4. Code that needs new pipeline output (like `speed` in `stats.json`) needs both the R2 sync and the push.
