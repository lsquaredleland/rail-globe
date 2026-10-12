# Architecture

A static site. An offline build turns OSM into one PMTiles file plus small JSON files; the browser draws them with MapLibre. No server, rebuilt monthly.

## Stack

| Part | Choice | Notes |
| --- | --- | --- |
| Source | OSM planet, cut into regions with one osmium pass | The pilot uses three regional extracts |
| Filter and export | Python + pyosmium | Exactly as `03-data-spec.md` says |
| Processing | Python + shapely | PostGIS at build time only if corridor work needs it (phase 5) |
| Tiles | tippecanoe → PMTiles | Overview (z0–8) and detail (z9–14) built in parallel, joined with `tile-join` |
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

1. **Extract** the regions from the planet in one osmium pass (one config file).
2. **Filter** to the data spec: GeoJSON features, the relations side table, the stop positions file.
3. **Classify** every track and line into a rail type (`01-features.md`, section 2).
4. **Build stations** with the station model.
5. **Build lines**: join each route's member ways into one feature per line.
6. **Compute region stats** for each preset.
7. **Tile** with tippecanoe and write side files.
8. **Publish** to R2 and the static host.

Also: a QA report per region per build (broken route relations, orphan stations, stops that map to no station), plus a diff against the previous build.

## Tile layers

| Layer | Geometry | Zooms | Key attributes |
| --- | --- | --- | --- |
| `track_overview` | Merged lines | z0–8 | type, highspeed |
| `track` | Lines | z9–14 | id, type, usage, electrified, voltage, frequency, maxspeed, gauge, tracks, tunnel, bridge, construction, line ids |
| `lines` | One feature per line | z9–14 | line id, type, colour, ref, name |
| `areas` | Station grounds, yards | z12–14 | id, kind |
| `buildings` | Station buildings | z13–14 | id |
| `platforms` | Lines and polygons | z14 | id, ref |
| `airports` | Polygons: aerodrome outline (z8, z10 without an IATA code) and terminals (z12) | z8–14 | id, kind, name, name_en, iata, icao, area |
| `complexes` | Points | z5 and up | id, name, name_en, size tier, terminus, platform count, line count |
| `stations` | Points | z10 and up, z8 for top tiers | id, name, name_en, modes, size tier, terminus, platform count, line count by type, airport (when at a commercial airport) |

Side files: `lines/<id>.json`, `stations/<id>.json`, `regions/<slug>.json`, search shards by name prefix (e.g. `search/pa.json`, queried with MiniSearch or similar), and one line index per region (`regions/<slug>/lines-index.json`: line id → name, colour, type, operator). The line index is loaded once so track tooltips can name the lines on a track without putting names in every track piece. Tooltip fields come from tiles plus the line index; click panels load the per-station or per-line JSON. Tiles carry `name` and an English name for labels and tooltips; other languages live in side files to keep tiles small.

## Zoom rules

| Zoom | Adds |
| --- | --- |
| 0–4 | High-speed and the busiest main lines, merged |
| 5–7 | All main lines, merged; top-tier station complexes |
| 8–9 | Branch, commuter, metro, light rail; lines become clickable |
| 10–11 | Tram; more stations |
| 12 | Station grounds and yard outlines; all stations |
| 13–14 | Yard track, sidings, spurs, crossovers, station buildings; platforms at 14 |

Construction track is shown from the same zoom as its rail type (an under-construction high-speed line shows from z0), not later.

Tiles include each class one zoom earlier than the style shows it, so zoom rules can be tuned in the style without rebuilding. Budget: under 600 KB per view on a mid-range phone.

## Hosting and cost

The pilot regions are a few MB; the full world build measured 1.1 GB. Both fit R2's free 10 GB. At public launch, a custom domain (about $10 a year) adds edge caching and removes the `r2.dev` rate limit. Free-tier limits were checked on 2026-10-02 and need checking again before launch.
