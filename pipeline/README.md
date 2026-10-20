# Pipeline

Command reference. First-time sequence from a fresh clone: `Run it` in the root `README.md`. The world build with timings: `docs/10-world-build.md`. Raw data to live site in one command: `pipeline/publish.sh` (stages: tools, cache, outlines, build, verify, upload, deploy, prune, live; `--dry-run` runs only the read-only checks; credentials in `r2/.env`; runbook `r2/README.md`). Publishing deploys the live site and needs the user's go.

Needs stable Rust (tested with 1.99.0). The build uses no Python, tippecanoe or osmium. Python 3 (standard library only) is for the dev checks: `check.py`, `top.py`; `measure_views.py` also needs `pip install pmtiles`; `qa/` needs numpy and shapely 2.

Run every command from the repository root. `RE` below is the extractor:

```
cargo build --release --manifest-path pipeline/rail-extract/Cargo.toml
RE=pipeline/rail-extract/target/release/rail-extract
```

`data/`, `app/public/data/`, `*.osm.pbf` and `pipeline/rail-extract/target/` are git-ignored. The app reads only the world build. The three pilots (`sf-bay-area`, `chengdu`, `paris`) are for data checks and specific tests.

## rail-extract

### cache
`$RE cache --planet planet-YYMMDD.osm.pbf [--out FILE] [--threads N]`
- Reads the planet once (about 1 min, 4.5 GB peak) and keeps the rail data, the place nodes, and admin boundaries (levels 2, 4, 5, and 6 to 9 for cities).
- Writes `data/cache/<planet name>.railcache` (1.2 GB). Later commands use the newest `.railcache` in `data/cache/` (`--cache FILE` overrides).
- Change which tags are kept (`src/rules.rs`, `cache_key`) and you need a new cache. A cache from an older format is refused with a message.

### outlines
`$RE outlines --level 2|4|5 [--country CC] [--cache FILE] [--out DIR]` and `$RE outlines --cities WANTED.json`
- `--level 2` countries, `4` provinces and states, `5 --country CN` Chinese prefecture cities (needed by region cards). About 1 s each.
- `--cities` takes `city-outlines-wanted.json` (written by `places`) and writes one outline per city (`docs/14-city-outlines-and-highlights.md`).
- Writes `data/regions/<slug>.geojson` and `data/regions/index.json`. A hand-made outline in `pipeline/regions/<slug>.geojson` wins over a generated one with the same slug.

### cut
`$RE cut <slug>... [--cache FILE] [--regions DIR] [--data DIR] [--threads N] [--strict-exemptions] [--build] [--no-tiles]`
- Cuts each region from the cache by its outline, in parallel, and classifies rail types with `pipeline/config/<slug>.json` (else `_default.json` or its group template). New region: add an outline, add a config only if it needs its own rules, run `cut <slug>`. The planet is not read again.
- Writes `<data>/<slug>/`: `features.geojsonl`, `relations.jsonl`, `stop_positions.jsonl`, `qa.json`, `airports.geojsonl`, `features.typed.geojsonl`, `lines.json`, `classify_report.json`.
- `--strict-exemptions` fails on an exemption that no longer matches anything (`docs/12-config-and-exemptions.md`). `--build` runs `build` straight after the cut.
- Pilots: `$RE cut sf-bay-area paris chengdu` (2 s). World: `$RE cut --regions data/regions --data data/world $(cat data/world/slugs.txt)` (7 s, 7.2 GB).

### build
`$RE build <slug>... [--no-tiles] [--data DIR] [--out DIR] [--threads N]`
- Needs the cut first. Builds the station model, stations under construction, tile input, and side files, per region in parallel.
- Writes side files to `<out>/<slug>/` (hash shards in `stations/`, `lines/`, `tags/`, `name-hl/` plus `shards.json`, `stats.json`, `shapes.json`, `<slug>.boundary.geojson`; `build` clears and rewrites them) and `<data>/<slug>/` (`stations.geojsonl`, `complexes.geojsonl`, `tiles_tmp/`).
- Unless `--no-tiles`, also writes `<out>/<slug>.pmtiles` (MLT, one region at a time; it holds a whole region in memory). It then runs `places` for the slugs built.
- Defaults: `--data data`, `--out app/public/data`. Pilots: `$RE build sf-bay-area paris chengdu`. World side files: `$RE build $(cat data/world/kept.txt) --no-tiles --threads 5 --data data/world --out app/public/data/world` (37 s, 12 GB).
- Region stats use `stat_areas` or `stat_auto` from the config (run `outlines --level 4` and `5` first).

### world
`$RE world (--slugs-file FILE | <slug>...) [--data DIR] [--out DIR] [--name NAME] [--deep-zoom N] [--regions DIR]`
- One memory-bounded tile writer run over every slug's `tiles_tmp`. Defaults: `--data data/world`, `--out app/public/data/world`, `--name world`, `--deep-zoom 6` (5 gives a byte-identical file).
- Writes `<out>/world.pmtiles` and `world.parts.json`, then runs `places`: `places.json`, `search.json`, and `<data>/city-outlines-wanted.json`. About 1 min, 10.5 GB peak. Details: `docs/10-world-build.md`.

### world-list
`$RE world-list (--slugs-file FILE | <slug>...) [--data DIR] [--min-km N]`
- Sorts a world cut into countries to build and countries to drop. Kept means at least `--min-km` (default 1) of open track typed anything but freight or yard.
- Writes `<data>/kept.txt` and `<data>/dropped.txt`.

### places
`$RE places (--parts FILE | <slug>...) [--data DIR] [--out DIR] [--cache FILE]`
- City, town and country labels and the search index, from the cache's place nodes and the stations of each region. No network. Run by `build` and `world`; run by hand only after changing code or outlines.
- Writes `<out>/places.json` and `<out>/search.json` (one file for the whole index). Rules: `src/places.rs` header and `docs/00-overview.md` (search index row, 2026-10-16).

### pminfo
`$RE pminfo FILE [--tsv FILE] [--top N]`
- Prints the PMTiles header, tiles and bytes per zoom, and the biggest tiles. `--tsv` writes every tile's size. Writes nothing else.

## Config

Per-region rules are in `pipeline/config/<slug>.json`. A region with no file uses `_default.json`, or its group template in `_groups.json`. China shares rules through `_china.json` (`cn` extends it; `china`, `chengdu`, `greater-bay-area` and `cn-beijing` extend `_china` or `cn`). A config can `extends` another and list justified `exemptions`: `docs/12-config-and-exemptions.md`. Keys most used: `high_speed_networks_re`, `type_overrides`, `high_speed_track_overrides`, `proposed_region` (an outline under `pipeline/regions/` inside which proposed track is kept; today only `cn/hk.json`), `stat_areas`, `stat_auto`.

Outlines in `pipeline/regions/` (`sf-bay-area`, `paris`, `chengdu`, `greater-bay-area`, `china`, `fr`, and `cn/hk.json`) are hand-made or came from Nominatim; the raw dumps and the scripts that built them are in `pipeline/archive/`.

## Checks and dev tools (Python, standard library)

- `python3 pipeline/check.py [slug...] [--exemptions] [--data DIR]`: answer lists in `pipeline/answers/` (major stations and tiers, lines, rail types). Without `--data`, pilots read `data/<slug>` and world slugs `data/world/<slug>`. Reads only; exit code 1 on a failure. One failure is known: `成都` tier 3.
- `python3 pipeline/top.py <slug> [n]`: the biggest stations of a built region by size score.
- `python3 pipeline/measure_views.py [world.pmtiles]`: compressed tile bytes per laptop and phone view at z2 to z8 against the 600 KB budget. Default file `app/public/data/world/world.pmtiles`.
- `pipeline/qa/`: `fetch_narn.py`, `study_ways.py`, `eval_rules.py`, the freight study against the US NARN data (`docs/13-freight-north-america.md`). `fetch_passenger_stations.py`: the 12306 station list for China (`config/cn-passenger-stations.txt`).
- `pipeline/logos/`: one-time operator logo pull; how `app/public/logos/` is made: `pipeline/logos/README.md`.
- `pipeline/archive/`: dead scripts and raw data, see its README.

Timings and the pilot-era command log: `docs/reference/pipeline-timings.md`.
