# Pipeline

Command reference. First-time sequence from a fresh clone: `Run it` in the root `README.md`. The world build with timings: `docs/10-world-build.md`. Raw data to live site in one command: `pipeline/publish.sh` (stages: tools, cache, outlines, build, verify, upload, deploy, prune, live; `--dry-run` runs only the read-only checks; credentials in `r2/.env`; runbook `r2/README.md`). Publishing deploys the live site and needs the user's go.

Needs stable Rust (tested with 1.99.0). The build uses no Python, tippecanoe or osmium. Errors print `error: ...` and exit 1. Python 3 (standard library only) is for the dev checks: `check.py`, `top.py`; `measure_views.py` also needs `pip install pmtiles`; `qa/` needs numpy and shapely 2.

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
`$RE outlines --level 2|4|5 [--country CC] [--cache FILE] [--out DIR] [--threads N]` and `$RE outlines --cities WANTED.json`
- `--level 2` countries, `4` provinces and states, `5 --country CN` Chinese prefecture cities (needed by region cards). About 1 s each.
- `--cities` takes `city-outlines-wanted.json` (written by `places`) and writes one outline per city (`docs/14-city-outlines-and-highlights.md`).
- Writes `data/regions/<slug>.geojson` and `data/regions/index.json`. A hand-made outline in `pipeline/regions/<slug>.geojson` wins over a generated one with the same slug.

### cut
`$RE cut <slug>... [--cache FILE] [--regions DIR] [--data DIR] [--threads N] [--strict-exemptions] [--build] [--no-tiles]`
- Cuts each region from the cache by its outline, in parallel, and classifies rail types with `pipeline/config/<slug>.json` (else `_default.json` or its group template). New region: add an outline, add a config only if it needs its own rules, run `cut <slug>`. The planet is not read again.
- Writes `<data>/<slug>/`: `features.geojsonl`, `way_nodes.jsonl`, `relations.jsonl`, `stop_positions.jsonl`, `qa.json`, `airports.geojsonl`, `features.typed.geojsonl`, `lines.json`, `classify_report.json`.
- `way_nodes.jsonl`: OSM node ids per way in `features.geojsonl` (delta-coded integers, in the same order).
- `--strict-exemptions` fails on an exemption that no longer matches anything (`docs/12-config-and-exemptions.md`). `--build` runs `build` straight after the cut.
- Pilots: `$RE cut sf-bay-area paris chengdu` (2 s). World: `$RE cut --regions data/regions --data data/world $(cat data/world/slugs.txt)` (7 s, 7.2 GB).

### build
`$RE build <slug>... [--no-tiles] [--data DIR] [--out DIR] [--threads N]`
- Needs the cut first. Builds the station model, stations under construction, tile input, and side files, per region in parallel.
- Writes side files to `<out>/<slug>/` (hash shards in `stations/`, `lines/`, `tags/`, `name-hl/` plus `shards.json`, `stats.json`, `shapes.json`, `<slug>.boundary.geojson`; `build` clears and rewrites them) and `<data>/<slug>/` (`stations.geojsonl`, `complexes.geojsonl`, `tiles_tmp/`).
- Unless `--no-tiles`, also writes `<out>/<slug>.pmtiles` (MLT, one region at a time; it holds a whole region in memory). It does not run `places`: run `places <slug>...` after it (the `world` command runs it itself).
- Defaults: `--data data`, `--out app/public/data`. Pilots: `$RE build sf-bay-area paris chengdu`. World side files: `$RE build $(cat data/world/kept.txt) --no-tiles --threads 5 --data data/world --out app/public/data/world` (37 s, 12 GB).
- Region stats use `stat_areas` or `stat_auto` from the config (run `outlines --level 4` and `5` first).

### world
`$RE world (--slugs-file FILE | <slug>...) [--threads N] [--data DIR] [--out DIR] [--name NAME] [--deep-zoom N] [--regions DIR]`
- One memory-bounded tile writer run over every slug's `tiles_tmp`. Defaults: `--data data/world`, `--out app/public/data/world`, `--name world`, `--deep-zoom 6` (5 gives a byte-identical file).
- Writes `<out>/world.pmtiles` and `world.parts.json`, then runs `places`: `places.json`, `search.json`, and `<data>/city-outlines-wanted.json`. About 1 min, 10.5 GB peak. Details: `docs/10-world-build.md`.

### world-list
`$RE world-list (--slugs-file FILE | <slug>...) [--data DIR] [--min-km N]`
- Sorts a world cut into countries to build and countries to drop. Kept means at least `--min-km` (default 1) of open track typed anything but freight or yard.
- Writes `<data>/kept.txt` and `<data>/dropped.txt`.

### places
`$RE places <slug>... [--data DIR] [--out DIR] [--cache FILE] [--threads N]` (pilots: `$RE places sf-bay-area paris chengdu`)
- City, town and country labels and the search index, from the cache's place nodes and the stations of each region. No network. Run by `world`; for pilots run it by hand after `build`.
- Writes `<out>/places.json` and `<out>/search.json` (one file for the whole index). Rules: `src/places.rs` header and `docs/00-overview.md` (search index row, 2026-10-16).

### cn12306
`$RE cn12306 [--data DIR] [--slug SLUG] [--input DIR] [--out DIR] [--side DIR] [--threads N]`
- Routes every train of the China 12306 timetable over the world cut's China track (`<data>/<slug>/features.geojsonl`, every `railway=rail` way, service track included at 3x cost), so a station can list the cities its trains go to and the paths they use. Needs the world cut of `cn`, including `way_nodes.jsonl` (step 1 of `docs/10-world-build.md`).
- Inputs: `pipeline/config/cn-12306/timetable.json` (7,253 trains: the trunk day and the C and S intercity day), `stations.tsv`, `meta.json` and `city_en.json` (how they are made and refreshed: `pipeline/config/cn-12306/README.md`). Defaults: `--data data/world`, `--slug cn`, `--input pipeline/config/cn-12306`, `--out data/cn12306`.
- Stage 1, per stop pair: A* with up to three alternatives, the path picked by the minimum run time from way `maxspeed` against the fastest scheduled time, a gap of 10 km or less bridged, else a chord.
- Stage 2, per train leg (`cn12306/corridor.rs`, after the corridor-v3 router of the user's china-rail project; its design and measured results are in that project's corridor-v3 report): candidate corridors per pair (fastest per class group, shortest, penalty-diversified alternatives, the stage-1 path), a run-time model (way `maxspeed`, class cap, braking curve) calibrated on the legs whose corridor is undisputed, the corridor that stopping trains of the same class group demonstrate between the same two stations, and a per-train forward-backward posterior with a reversal penalty. A leg leaves the stage-1 path only when one corridor has posterior 0.9 or more and the stage-1 path 0.05 or less; a pair stage 1 left as a chord is drawn on track at 0.8, or at 0.35 when its gap is under 1 km; a stage-1 path that crosses a gap is replaced by any feasible candidate with a gap smaller by 0.5 km or none (smallest gap first, then posterior), and the smallest-gap bridge starts at a dangling track end, else at any node within 4 km of the station. Candidates may be as long as max(2 chord + 40, 2.5 chord + 5) km. Where corridor-v3 draws the shortest gap-free candidate for every pair's production legs, this build does so only for a pair whose stage-1 path crosses a gap: stage 1 picks by the schedule, and the shortest would move the nonstop Chengdu-Chongqing trains off the Chengyu line. Every chosen path, and the stage-1 paths, are redrawn within 200 m of themselves on the busiest track of the same named line (hops of 30 m, 4 rounds), so double track is one line. Nodes are OSM node ids (`way_nodes.jsonl`, written by `cut` beside the features; the build fails without it): two ways connect only where they share a node. One repair: two different nodes at bit-identical coordinates join when one is a way end and both ways are on the same level (equal bridge, tunnel, layer; OSM's duplicate-node error); each is listed in `report.json` under `graph.dup_node_joins`. In the router's graph a gap between components is repaired by a stitch (250 m at most, one end a dead end), flagged as a stitch. `railway=ferry` is not in the `cn` cut, so a crossing such as the Qiongzhou Strait stays a chord. The parameters are constants (the original read `CV3_*` environment variables).
- Then stretches: maximal runs of ways with the same set of trains, with a stable id (a station's bands count its own trains on a stretch).
- Self-check (`cn12306/selfcheck.rs`, in `report.json` under `corridor.self_check`): hide an intermediate stop of a stopping train and ask which corridor it took. Reports the shares for the stage-1 path, the shortest path, the model's top choice, what is shipped (stage 1 plus the decisive switches) and the precision of the switches.
- Writes `<out>/pairs.json` (stage-1 status, km, minimum minutes, way ids, gaps), `stretches.json` (id, km, way ids, trains as indexes into the timetable), `legs.jsonl` (every leg: decision, chosen and stage-1 corridor with posterior, way ids), `wrong_switches.jsonl` and `report.json`, only with `--out` (the build writes none of them; it keeps the report in `<data>/cn/cn12306_report.json`, below). Identical for any thread count (tested at 5 and 14). About 40 s and 2.4 GB on 14 cores (85 s on 5).
- In the build: `build cn` runs it itself when `pipeline/config/cn-12306/` exists, after the station model and the other side files of `cn`, and writes the app's side files `<out>/cn/cn12306/<rev>/` (1,024 record shards, `ids.json`, `stretches.json`, `ways/<cell>.json`; `shards.json` gains `cn12306`: rev, count, snapshot, cells; `snapshot` is a list of `{ date, classes }`, one entry per timetable day, e.g. `[{"date":"2026-10-21","classes":["G","D","K","Z","T","plain"]},{"date":"2026-10-28","classes":["C","S"]}]`) and `<data>/cn/cn12306_report.json` (stations matched and English names, sizes, twin ways, and the run's `graph` with `dup_node_joins`, `routing`, `corridor` with `decisions` and `self_check`, `stretches`). Nothing else the build writes changes. The stage is a layer on top: if it cannot run (inputs, `way_nodes.jsonl` or outlines missing, a bad file; every failure of the stage is a returned error, never a panic) the build prints a warning, skips it, and removes the `cn12306` folder and the `shards.json` entry, so nothing old stays published. `--side DIR` does the same for the standalone command, also on failure (`DIR/<slug>/shards.json` must exist from a build). The revision `<rev>` is a hash of every file under it and only the current one is kept on disk.

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
- `pipeline/logos/`: one-time operator logo pull; how `app/static/logos/` is made: `pipeline/logos/README.md`.
- `pipeline/archive/`: dead scripts and raw data, see its README.

Timings and the pilot-era command log: `docs/reference/pipeline-timings.md`.
