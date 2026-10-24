# Rail Globe

A static web map of passenger rail built from OpenStreetMap. Its goal is comparing regions at the same real scale (for example the SF Bay Area against Paris, or all of China); the side-by-side compare view is planned, and today the app is a single map.

- Rail types (high-speed, intercity, regional, metro, light rail / tram, monorail, funicular, other, freight), plus colour by operator or by speed
- Stations ranked by physical size, never by line counts; station and yard outlines show the footprint of the railway
- Track under construction (dashed), optional proposed track, and an "HSR only" switch
- Region cards with route-km totals per province and city
- No server, no GTFS, no live data: the app and its tiles are static files

Map data © OpenStreetMap contributors, [ODbL](https://www.openstreetmap.org/copyright).

## Layout

| Path | What it is |
| --- | --- |
| `app/` | Vite + React + TypeScript app, MapLibre GL JS 6, PMTiles |
| `pipeline/` | Rust extractor (`rail-extract`: planet to rail cache to region cuts, classification, station model, tile input, MLT tile writer), config per region. See `pipeline/README.md` |
| `docs/` | Spec: overview and decisions, features, architecture, data spec, station model, pilot plan, action items. Index in `CLAUDE.md` |
| `CLAUDE.md` | Working rules for this repo |

`docs/00-overview.md` (Decisions) wins when documents disagree.

## Run it

From a fresh clone to the data checks (pilot regions); the app needs the world build below. The steps up to 5 give the pilot data. Every command runs from the repository root. Tested end to end on a fresh clone (macOS, 14 cores, planet-260928; the planet read itself was skipped, see step 2).

Needs:
- Node 20+ (tested with 22) and npm.
- Rust, stable (edition 2021, no version pinned; tested with 1.99.0). No tippecanoe: tiles are written by the Rust extractor.
- No Python in the build. Python 3 (standard library only) is only for the data checks, `pipeline/check.py` and `pipeline/top.py`.
- Disk and memory for the optional world build: see below.
- Network: none needed. City, town and country names come from the planet (the `.railcache`), with no network.
- The planet file, about 80 to 90 GB, only if you have no `.railcache` yet. Memory: the tile step peaks at about 12 GB; cache building at 4.5 GB. Disk: about 4 GB for `data/` and `app/public/data/`, plus the planet.

```
# 1. Build the extractor (52 s the first time)
cargo build --release --manifest-path pipeline/rail-extract/Cargo.toml
RE=pipeline/rail-extract/target/release/rail-extract

# 2. Planet -> rail cache (data/cache/<planet name>.railcache, 1.2 GB; about 1 min). Skip if you already have the .railcache.
#    Download planet-YYMMDD.osm.pbf from https://planet.openstreetmap.org/pbf/ (or planet-latest.osm.pbf) into the repo root;
#    the cache takes its name from the file, and the later steps use the newest .railcache in data/cache/.
$RE cache --planet planet-YYMMDD.osm.pbf

# 3. Region outlines from the planet (2 s). Level 2: countries (de, ch, it); level 4: provinces and states (cn-beijing, and the region cards);
#    level 5 for China: cities (region cards). fr, china, paris, chengdu, sf-bay-area and greater-bay-area come with the repo.
$RE outlines --level 2
$RE outlines --level 4
$RE outlines --level 5 --country CN

# 4. Cut the three pilot regions from the cache. Hand-made outlines for sf-bay-area, paris and chengdu come with the repo.
$RE cut sf-bay-area paris chengdu

# 5. Build them: station model, side files into app/public/data/<slug>/ and one <slug>.pmtiles each.
#    Add --no-tiles for side files only. Then `$RE places sf-bay-area paris chengdu` writes places.json and search.json (names and the search index).
$RE build sf-bay-area paris chengdu

# 6. Run the data checker on the pilots (answer lists)
python3 pipeline/check.py

# 7. Run the app (it reads the world build, see below; without it the map has no data)
npm --prefix app install
npm --prefix app run dev        # http://127.0.0.1:5173/
```

Check it worked: `app/public/data/paris.pmtiles` and the other two pilot files exist, and `python3 pipeline/check.py` passes (one failure is known: `成都` tier 3). The app itself needs the world build below. `npm --prefix app run build` type-checks and builds a static site in `app/dist/`.

### The world build (the app's only data source)

The app's only data source (deployed 2026-10-16): every country with passenger rail in one file, `app/public/data/world/world.pmtiles` (1.15 GB, 142 countries). Run it when data changed (it is the default after data-changing work; disk: about 10 GB for `data/world/` and `app/public/data/world/`; memory: 10.5 GB peak for the tile step, 12 GB for the side files). Needs steps 1 and 2 above (the `.railcache`) and the level 2 and level 4 outlines of step 3. Times on a 14-core Mac: cut 7 s, side files 37 s, tiles 55 to 59 s.

```
mkdir -p data/world && ls data/regions | grep -E '^[a-z]{2}\.geojson$' | sed 's/.geojson//' > data/world/slugs.txt   # the 216 country outlines
$RE cut --regions data/regions --data data/world $(cat data/world/slugs.txt)                         # all countries; a country with no config of its own uses pipeline/config/_default.json, or its group template (_groups.json: Argentina and Africa)
$RE world-list --data data/world --slugs-file data/world/slugs.txt                                   # kept.txt (>= 1 km open passenger track: 142) and dropped.txt
$RE build $(cat data/world/kept.txt) --no-tiles --threads 5 --data data/world --out app/public/data/world
$RE world --slugs-file data/world/kept.txt                                                           # first pass: world.pmtiles + world.parts.json, and places + search + data/world/city-outlines-wanted.json (the point cities)
$RE outlines --cities data/world/city-outlines-wanted.json                                           # city outlines from the cache (admin levels 6 to 9 of the place=city nodes), data/regions/<cc>-<city>.geojson
$RE build $(cat data/world/kept.txt) --no-tiles --threads 5 --data data/world --out app/public/data/world   # again: the statistics and shapes now have the cities
$RE world --slugs-file data/world/kept.txt                                                           # second pass: the same, with the city outlines in the search index (skip it if you only changed outlines: `$RE places ...` is enough)
$RE pminfo app/public/data/world/world.pmtiles                                                       # header, tiles and bytes per zoom
npm --prefix app run dev                                                                              # look at it
```

Not in the repo, so rebuilt by the commands above: `data/`, the planet, `app/public/data/` (including the world folder). The world build also writes `world/places.json` and `world/search.json` (the `world` command runs `rail-extract places` last). Details and the memory-bounded writer: `docs/10-world-build.md`.

Steps 3 to 5 read only the `.railcache`, not the planet, and are quick (seconds for the pilots). The app reads tiles and side files from `app/public/data/`, or from `VITE_DATA_BASE` (for example an R2 bucket) when set; see `app/.env.example`. Details, timings and the other pipeline commands are in `pipeline/README.md`.

## Ideas and tools to evaluate

Research notes (line number badges, interlining, station markers) are in `docs/17-action-items.md` section 7.

## Status

World build deployed 2026-10-16 (R2 r2.dev + Vercel, `pipeline/publish.sh`); data last published 2026-10-20; custom domain and the low-zoom tile budget (`docs/16-low-zoom-tile-weight-plan.md`) are open. The pilot regions remain the data-check regions. Phases and their "Done when" tests are in `docs/05-pilot-plan.md`; open items are in `docs/17-action-items.md`. The world build is the default after data-changing work (since 2026-10-17); use the pilot extracts for specific testing; publishing still needs the user's go.
