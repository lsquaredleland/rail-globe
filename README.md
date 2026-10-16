# Rail Globe

A static web map of passenger rail built from OpenStreetMap, made for comparing regions at the same real scale (for example the SF Bay Area against Paris, or all of China).

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
| `docs/` | Spec: overview and decisions, features, architecture, data spec, station model, pilot plan, open questions |
| `CLAUDE.md` | Working rules for this repo |

`docs/00-overview.md` (Decisions) wins when documents disagree.

## Run it

From a fresh clone to a working pilot map. Every command runs from the repository root. Tested end to end on a fresh clone (macOS, 14 cores, planet-260928; the planet read itself was skipped, see step 2).

Needs:
- Node 20+ (tested with 22) and npm.
- Rust, stable (edition 2021, no version pinned; tested with 1.99.0). No tippecanoe: tiles are written by the Rust extractor.
- Python 3 with `mapbox-vector-tile` (only `pipeline/places.py` needs it; it pulls in shapely, protobuf and pyclipper). No other step uses Python.
- Disk and memory for the optional world build: see below.
- Network: `places.py` fetches place names from OpenFreeMap; the glyph step downloads about 34 MB of Noto fonts on its first run.
- The planet file, about 80 to 90 GB, only if you have no `.railcache` yet. Memory: the tile step peaks at about 12 GB; cache building at 4.5 GB. Disk: about 4 GB for `data/` and `app/public/data/`, plus the planet.

```
# 1. Build the extractor (52 s the first time)
cargo build --release --manifest-path pipeline/rail-extract/Cargo.toml
RE=pipeline/rail-extract/target/release/rail-extract

# 2. Planet -> rail cache (data/cache/<planet name>.railcache, 1.1 GB; about 1 min). Skip if you already have the .railcache.
#    Download planet-YYMMDD.osm.pbf from https://planet.openstreetmap.org/pbf/ (or planet-latest.osm.pbf) into the repo root;
#    the cache takes its name from the file, and the later steps use the newest .railcache in data/cache/.
$RE cache --planet planet-YYMMDD.osm.pbf

# 3. Region outlines from the planet (2 s). Level 2: countries (de, ch, it); level 4: provinces and states (cn-beijing, and the region cards);
#    level 5 for China: cities (region cards). fr, china, paris, chengdu, sf-bay-area and greater-bay-area come with the repo.
$RE outlines --level 2
$RE outlines --level 4
$RE outlines --level 5 --country CN

# 4. Cut the ten regions the app offers from the cache (2 s)
$RE cut sf-bay-area china de fr ch it paris chengdu greater-bay-area cn-beijing

# 5. Build. The six pilot sources make tiles and, together, pilot.pmtiles (30 s); the app reads only that file, and it is written only when all six are built.
#    The other four are drawn from those tiles and need side files only (1 s).
$RE build sf-bay-area china de fr ch it
$RE build paris chengdu greater-bay-area cn-beijing --no-tiles

# 6. City and town names (app/public/data/places.json). Needs step 5 first; slow because it fetches 500 tiles (15 min here); optional: without it no country, city or town names show (station names still do).
python3 -m pip install mapbox-vector-tile
python3 pipeline/places.py

# 7. Label glyphs (git-ignored; without them every label fails). Run after steps 5 and 6: it reads pilot.pmtiles and places.json. First run 8 min here (font downloads), 40 s once the fonts are cached.
npm --prefix pipeline/glyphs install
node pipeline/glyphs/build-glyphs.mjs

# 8. Run the app
npm --prefix app install
npm --prefix app run dev        # http://127.0.0.1:5173/
```

Check it worked: `app/public/data/pilot.pmtiles` (about 300 MB) and `pilot.parts.json` exist, `app/public/glyphs/` holds two folders (`Noto Sans Regular`, `Noto Sans Bold`), and the app at `#v=11/48.86/2.35` shows Paris rail with station names and a region card, with no console errors. `npm --prefix app run build` type-checks and builds a static site in `app/dist/`.

### Optional: the world build

Every country with passenger rail in one file, `app/public/data/world/world.pmtiles` (1.15 GB, 146 countries). Not part of the pilot; run it only when you want it (disk: about 10 GB for `data/world/` and `app/public/data/world/`; memory: 10.5 GB peak for the tile step, 12 GB for the side files). Needs steps 1 and 2 above (the `.railcache`) and the level 2 and level 4 outlines of step 3. Times on a 14-core Mac: cut 7 s, side files 37 s, tiles 55 to 59 s.

```
mkdir -p data/world && ls data/regions | grep -E '^[a-z]{2}\.geojson$' | sed 's/.geojson//' > data/world/slugs.txt   # the 216 country outlines
$RE cut --regions data/regions --data data/world $(cat data/world/slugs.txt)                         # all countries; no config of their own: pipeline/config/_default.json
$RE world-list --data data/world --slugs-file data/world/slugs.txt                                   # kept.txt (>= 1 km open passenger track: 146) and dropped.txt
$RE build $(cat data/world/kept.txt) --no-tiles --threads 5 --data data/world --out app/public/data/world
$RE world --slugs-file data/world/kept.txt                                                           # app/public/data/world/world.pmtiles + world.parts.json
$RE pminfo app/public/data/world/world.pmtiles                                                       # header, tiles and bytes per zoom
VITE_TILE_SOURCE=world npm --prefix app run dev                                                      # look at it (the app reads only the pilot by default)
```

Not in the repo, so rebuilt by the commands above: `data/`, the planet, `app/public/data/` (including the world folder), `app/public/glyphs/`. City and town names for the world do not exist yet (`places.py`, the current step, only covers the pilot, and a Rust step is planned to replace it). Details and the memory-bounded writer: `docs/10-world-build.md`.

Steps 3 to 5 read only the `.railcache`, not the planet, and took about 40 s in all; steps 6 and 7 are network-bound. The app reads tiles and side files from `app/public/data/`, or from `VITE_DATA_BASE` (for example an R2 bucket) when set; see `app/.env.example`. Details, timings and the other pipeline commands are in `pipeline/README.md`.

## Status

Pilot stage: SF Bay Area, Paris and China (with Chengdu, Beijing and the Greater Bay Area drawn from China's tiles). Phases and their "Done when" tests are in `docs/05-pilot-plan.md`; undecided items are in `docs/06-open-questions.md`. Never run a world build unless asked (see "Optional: the world build" above).
