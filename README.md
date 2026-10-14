# Rail Globe

A static web map of passenger rail built from OpenStreetMap, made for comparing regions at the same real scale (for example the SF Bay Area against Paris, or all of China).

- Rail types (high-speed, intercity, regional, metro, light rail / tram, monorail, funicular, other, freight), plus colour by operator or by speed
- Stations ranked by physical size, never by line counts; station and yard outlines show the footprint of the railway
- Track under construction (faded solid base with dashes from z9), optional proposed track, and an "HSR only" switch
- Region cards with route-km totals per province and city
- No server, no GTFS, no live data: the app and its tiles are static files

Map data © OpenStreetMap contributors, [ODbL](https://www.openstreetmap.org/copyright).

## Layout

| Path | What it is |
| --- | --- |
| `app/` | Vite + React + TypeScript app, MapLibre GL JS 6, PMTiles |
| `pipeline/` | Rust extractor (`rail-extract`: planet to rail cache to region cuts, classification, station model, tile input), tippecanoe, config per region. See `pipeline/README.md` |
| `docs/` | Spec: overview and decisions, features, architecture, data spec, station model, pilot plan, open questions |
| `CLAUDE.md` | Working rules for this repo |

`docs/00-overview.md` (Decisions) wins when documents disagree.

## Run it

Needs Node 20+, Rust, Python 3 (shapely, numpy) and [tippecanoe](https://github.com/felt/tippecanoe).

```
# 1. Build the data (not committed: *.osm.pbf, data/ and app/public/data/ are git-ignored)
cd pipeline/rail-extract && cargo build --release && cd ..
rail-extract/target/release/rail-extract cache --planet ../planet-260928.osm.pbf
rail-extract/target/release/rail-extract cut sf-bay-area paris china --build

# 2. Run the app
cd ../app && npm install && npm run dev
```

The app reads tiles and side files from its own `public/data/`, or from `VITE_DATA_BASE` (for example an R2 bucket) when set; see `app/.env.example`. `npm run build` type-checks and builds for a static host. Details, timings and the other pipeline commands are in `pipeline/README.md`.

## Status

Pilot stage: SF Bay Area, Paris and China (with Chengdu, Beijing and the Greater Bay Area drawn from China's tiles). Phases and their "Done when" tests are in `docs/05-pilot-plan.md`; undecided items are in `docs/06-open-questions.md`. Never run a world build unless asked.
