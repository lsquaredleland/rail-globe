# World build (2026-10-15)

Every country with passenger rail transit in one PMTiles file, built from `data/cache/planet-260928.railcache` (the planet is not read again). Requested explicitly by the user; it overrides the "never run a world build" rule in CLAUDE.md for this one build. Outputs: intermediates in `data/world/`, the app-facing files in `app/public/data/world/` (`world.pmtiles`, `world.parts.json`, and per country `<slug>/` side files and `<slug>.boundary.geojson`). The pilot files are not touched.

## Steps and commands

From a fresh clone with a `.railcache` in `data/cache/` (steps 0 to 2 of `Run it` in the root `README.md`; the planet is not read again). Run from the repository root. All commands are `rail-extract <subcommand>`; `RE=pipeline/rail-extract/target/release/rail-extract`.

```sh
cargo build --release --manifest-path pipeline/rail-extract/Cargo.toml
# 0. outlines: data/regions/<cc>.geojson, one per country (216), and data/regions/index.json
$RE outlines --level 2
$RE outlines --level 4        # the region cards and stat_auto read the level 4 index
mkdir -p data/world && ls data/regions | grep -E '^[a-z]{2}\.geojson$' | sed 's/.geojson//' > data/world/slugs.txt
# 1. cut every country and classify rail types. --regions data/regions: the hand-made pipeline/regions outlines are NOT used.
#    A country without pipeline/config/<cc>.json uses the template pipeline/config/_default.json (one log line says so).
$RE cut --regions data/regions --data data/world $(cat data/world/slugs.txt)
# 2. which countries have passenger rail: >= 1 km of open track typed anything but freight or yard -> data/world/kept.txt, dropped.txt
$RE world-list --data data/world --slugs-file data/world/slugs.txt
# 3. station model, side files and tile input for the kept countries (side files to app/public/data/world/<slug>/)
$RE build $(cat data/world/kept.txt) --no-tiles --threads 5 --data data/world --out app/public/data/world
# 4. the memory-bounded tile writer -> app/public/data/world/world.pmtiles and world.parts.json
$RE world --slugs-file data/world/kept.txt
#    defaults: --data data/world --out app/public/data/world --name world --deep-zoom 6 --regions data/regions
# 5. inspect: header, tiles and bytes per zoom, biggest tiles
$RE pminfo app/public/data/world/world.pmtiles [--tsv FILE] [--top N]
```

Measured 2026-10-15 (14-core Mac, planet-260928): cut of 216 countries 7 s (7.2 GB peak); `build --no-tiles` for 146 countries (142 since 2026-10-16) 37 s (12 GB peak RSS, 18 GB footprint with 5 threads); `world` 55 to 59 s, 10.5 GB peak RSS (7.9 GB footprint), 1,701,830 tiles, 1,150.5 MB. Result: 146 kept, 70 dropped (`data/world/kept.txt`, `dropped.txt`).

`world` writes `world.parts.json` (slug and outline bounds of every country with tile input) from `data/regions` outlines. Boundary files for the app (`<slug>.boundary.geojson`) are written by `build` next to the side files.

Not in the repo (git-ignored or generated): `data/` (cache, outlines, cut, tile input), the planet, `app/public/data/` (tiles and side files, the world folder is 2.8 GB). Everything above is reproducible from the planet with these commands, except as listed under "Choices and limits".

### Default country config

`pipeline/config/_default.json` is the committed template for a region with no `pipeline/config/<slug>.json`: generic rules (no commuter or high-speed network patterns, no overrides, a 200 km/h high-speed floor, `stat_auto` for admin level 4). `build` fills three keys from the region's outline: `stat_name` from `name_en`, `stat_local` from `name`, `stat_auto.country` from `country` (`site.rs`, `fill_default`). This replaced 208 near-identical untracked config files (the Americas template, copied per country): re-running `cut`, `build --no-tiles` and `world` without them gave byte-identical `features.typed.geojsonl`, side files, tile input and `world.pmtiles` (checked 2026-10-15). A config of its own wins; add one only when a country needs rules.

### Viewing it

**Updated 2026-10-16: the world build is the only source the app reads; `VITE_TILE_SOURCE` and the pilot regions were removed (commit 0da274e), and it is deployed (R2 + Vercel, `pipeline/publish.sh`, `r2/README.md`). The block below is the history of the dev switch; plain `npm --prefix app run dev` now shows the world.**

The app used to read only `pilot` by default; `VITE_TILE_SOURCE=world` made it read `world.pmtiles`, `world.parts.json` and the country side files from `app/public/data/world/` (`app/src/config.ts`):

```sh
VITE_TILE_SOURCE=world npm --prefix app run dev      # http://127.0.0.1:5173/ ; zoom to Cairo, Tokyo, Moscow ...
VITE_TILE_SOURCE=world npm --prefix app run build    # a build that shows the world (copies app/public/data into dist)
```

(Historical: the picker listed the pilot regions, whose boundary files are not in `data/world/`; the hand-tuned regions are gone, regions now come from `search.json`.)

### Places and search (`rail-extract places`)

`world` ends by running `rail-extract places --data <data> --out <out> <slugs>` over the kept countries, so `app/public/data/world/places.json` (the app's labels: `[lon, lat, class, rank, name, name_en, n_stations]`) and `world/search.json` (the search bar's index) come with the world build. By hand: `$RE places --data data/world --out app/public/data/world $(cat data/world/kept.txt)` (0.6 s). No network: the place nodes (place=city, town) are in the `.railcache`; the stations come from `data/world/<slug>/complexes.geojsonl`, the country and province outlines from `data/regions`. Rules and formats: header of `pipeline/rail-extract/src/places.rs`, summary in `pipeline/README.md`. It also reads `data/world/<slug>/features.typed.geojsonl` (the metro test below).

`search.json` rules for the cities (decided 2026-10-15; towns are not in it, `places.json` still has them): a city is a place=city, or a China level-5 outline, with a station complex within 12 km and open passenger track typed `metro` or `light_rail` (construction and proposed do not count; monorail alone does not; light rail added 2026-10-15 so cities with only a light rail system keep their own entry) within 12 km of its point, measured to the track itself. Then ONE entry per metro area: candidates sorted by rank (population-derived), then population, then capital flag, then n_stations, taken greedily, each dropped when an accepted entry is within 20 km. A level-5 outline that holds candidate places gives one entry: the outline's box and slug (`cn-chengdu`) with the best place's name, rank and point; the other places inside it are not entries (Tianfu New Area is Chengdu). Level-5 outlines inside a province named like a city (Hong Kong Island, Kowloon, New Territories in `cn-hong-kong`) are dropped, and level-5 outlines are not listed as provinces any more. Result on the 2026-10-15 world: 146 countries (142 since 2026-10-16), 1,370 provinces (1,674 before the small-country rule), 726 cities with metro or light rail (384 with metro only; 63 with an outline), 255 KB raw, 92 KB gzipped (was 3.3 MB with 7,717 cities and 28,102 towns). Small countries (under 3,000 km of passenger rail in `stats.json` and no city entry) are listed without provinces: 1,370 provinces left in the world file (`docs/14-city-outlines-and-highlights.md`). Known limits: a place with the best rank wins even when a neighbour has more stations (New Taipei over Taipei, Yokohama is 28 km from Tokyo so it stays); a mis-typed metro line puts a city in (Kinshasa).


## The memory-bounded writer (`tilewriter::run_world`)

`run` (the per-region path of `build`, unchanged) holds a region's features and all pieces of all tiles in memory. `run_world` instead:

1. Memory-maps every `tiles_tmp/*.geojsonl` and indexes each feature: file, byte range, zoom range, bounding box (Web Mercator unit square, f32). One parallel parse also collects the whole input's column types (MLT allows one type per column), layer zoom ranges and bounds. Nothing but the index stays in memory (about 4 GB resident for 8.5 million features, mostly the mapped files).
2. Makes the tiles in passes. A pass handles zooms zmin..=zmax inside one partition tile: it parses only features whose bounding box (plus the 3% clip buffer) meets the tile and whose zoom range meets zmin..=zmax, slices them with the same `slice_group` (per layer, zoom range, tiny flag, runs of 8,000), keeps tiles inside the partition, encodes (same seq order, coalescing, line joining, MLT, gzip) and appends to a spool file, then frees everything. Passes: A the world z0-3; B each populated z4 tile, z4 to deep-1; C each populated tile at zoom `--deep-zoom` (default 6), deep to z14. Passes run one after another, each parallel inside; biggest first.
3. Sorts the spool index by tile id and writes the PMTiles file (header, directories, dedup by the pmtiles crate).

The input order of a feature in a layer (slug order, file order, line order) decides its draw order within a tile, as in `run`.

Verification (2026-10-15; the `tiles` subcommand and the pilot join were removed later, the numbers stay as evidence):
- `rail-extract tiles` (old path, removed) on the pilot sources after the change: `cmp` identical to the build before the change (298,819,502 bytes).
- `rail-extract world` on the same six pilot sources: `cmp` identical to `run`'s `pilot.pmtiles`, 344,928 tiles; 15 s and 5.3 GB peak against 12.7 s and 9.4 GB.
- World with `--deep-zoom 5` and `6`: `cmp` identical (partition does not matter).
- Known edge: features share a slicer, and a slicer does not descend into a tile with no points, so grouping can in theory change pieces smaller than the simplification tolerance. It did not show in any comparison.

## Choices and limits

- "Has passenger rail transit": at least 1 km of open track typed high_speed, intercity, regional, metro, light_rail, monorail, funicular or other (way length, so double track counts twice; construction and proposed not counted). Freight-only and yard-only countries and countries under 1 km are dropped. The freight detection and labelling are the repo's as they are.
- Generic country configs are `pipeline/config/_default.json` (no commuter networks, high-speed from 200 km/h on the track's own tags or route services); no country-specific type overrides except those in the hand-written configs. The world build also used `ca.json`, `us.json`, `mx.json` (Americas freight rules) and `cn.json` (China's rules), which are hand-written and were not tracked in git when this was written: commit them, or a fresh clone builds those four countries with the template and gets different results.
- Tiles are not cut at country borders: a way that crosses a border is in both countries' tile input and drawn twice, identically (as in the pilot).
- There are no glyph files: the browser draws every label from system fonts (`docs/00-overview.md`, row 2026-10-17), so there is no glyph build step and no script coverage list to maintain.
- `places.json` for the world holds every city and town with a station nearby (35.8 k places), not OpenFreeMap's thinned set; the style's collision handling decides what is drawn. A distance or population thinning would shrink the file (1.9 MB, 0.77 MB gzipped) if the first load matters.
