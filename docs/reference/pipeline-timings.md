# Pipeline timings and the pilot-era command log

Moved from `pipeline/README.md` on 2026-10-20, text unchanged. It is a log, not a reference: the commands, flags and timings below are from 2026-10-12 to 2026-10-15 and include things that no longer exist (`pilot.pmtiles`, `pilot.parts.json`, `build --combined`, the `tiles` command, tippecanoe, `tile-join`, the Python stations steps; removed 2026-10-14 to 2026-10-20). Current commands: `pipeline/README.md`. Current world timings: `docs/10-world-build.md`.

Timings below were measured on 2026-10-12 (14-core Mac, planet-260928).

```
cd rail-extract && cargo build --release && cd ..        # once, and after changing the Rust code (5 s after a change)
rail-extract/target/release/rail-extract cache --planet ../planet-260928.osm.pbf
#   once per planet: 54 s, 4.5 GB peak memory -> ../data/cache/planet-260928.railcache (1.2 GB; blob index cached beside it)
#   also keeps the place=city and place=town nodes (115,309: name, name:en, int_name, population, capital, wikidata; 10 MB, +0.9%) for the places step; place=village (1.65 M nodes) is counted, not kept.
#   A cache from before the places step (format RGRAIL02) is refused with a message: rebuild it.
rail-extract/target/release/rail-extract cut sf-bay-area china de fr ch it paris chengdu greater-bay-area cn-beijing
#   regions cut in parallel from the cache, by their outline regions/<slug>.geojson, and classified (rail types) with the
#   rules in config/<slug>.json: ten regions including all of China in 2.4 s (4.8 GB peak)
#   writes ../data/<slug>/{features.geojsonl, relations.jsonl, stop_positions.jsonl, qa.json, airports.geojsonl,
#                          features.typed.geojsonl, lines.json, classify_report.json}
#   rail type rules: rail-extract/src/classify.rs (ported from classify.py 2026-10-12, same output byte for byte)
rail-extract/target/release/rail-extract build sf-bay-area china de fr ch it
#   per region, in parallel: station model (stations, complexes, size tiers), stations under construction, tile input and side files
#   into ../app/public/data; then the Rust MLT tile writer (tilewriter.rs) for each region in turn -> <slug>.pmtiles (it holds a whole region in memory); skipped for the pilot sources when the pilot join runs (the app reads only pilot.pmtiles)
#   timings 2026-10-14 (earlier run, per-region files plus the pilot join): sf-bay-area, paris, china, de, fr, ch, it together 40.4 s wall, 14.1 GB peak RSS; in the orig-vs-mlt benchmark (pilot file only) the same seven slugs took 27.4 s. Tile step per region (earlier single runs): SF 0.1 s, Paris 0.3 s,
#   China 4.3 s (133.3 MB, 5.3 GB; 4.1 s median of 3 runs in the benchmark, 4.9 s in another single run), Germany 3.8 s, France 2.4 s, Switzerland 0.6 s, Italy 1.0 s. (tippecanoe took China 28 to 37 s)
#   construction and proposed track is written as real dashes, the tile layer `con_dash` (tiles.rs: merged end to end, parallel tracks made one line, cut for 22 half-zoom bands, no yard track): the app's dashes. Upload tiles before deploying the app: it has no fallback, so old tiles show no construction
#   the tile input is tiles_tmp/*.geojsonl; each line's "zoom":{minzoom,maxzoom} member is the feature's zoom range (read by tilewriter.rs)
#   whenever tiles are built and any of sf-bay-area, china, de, fr, ch, it is among the regions, one tile writer run over those six sources' tiles_tmp writes pilot.pmtiles (12.7 s; 298.8 MB now, 310.5 MB with FastPFOR and FSST on),
#   so a partial rebuild refreshes the pilot; a source with no tile input is warned about and the join skipped. It also writes pilot.parts.json (each source's slug and its outline's bounds;
#   bounds of neighbouring countries overlap, so they are not checked). Both are written to a temporary file and renamed.
#   The app draws all rail from pilot.pmtiles (one set of ~46 style layers instead of 3 x 50) and learns the parts from pilot.parts.json
rail-extract/target/release/rail-extract build ch it fr --combined europe-test
#   combined build: per-slug work without tiles, then ONE tile writer run over every slug's tiles_tmp -> ../app/public/data/europe-test.pmtiles
#   plus europe-test.parts.json (slug and outline bounds). Does not write per-region .pmtiles or touch pilot.pmtiles
#   TIMING ONLY (the Americas run used per-country config files, now replaced by config/_default.json; ca, mx and us had hand-written configs that are not committed; the Americas rule work was reverted, docs/00-overview.md 2026-10-15): 27 American countries (ar bb bo br ca cl co cr cu do ec gt hn ht jm kn mx ni pa pe py sv tc tt us uy ve --combined americas): 15.3 s wall, 11.4 GB peak, 207.9 MB
#   (tile writer 7.9 s; tippecanoe took 276 s, 222 MB)
rail-extract/target/release/rail-extract tiles ch it --name x --out ../data/tiles
#   the tile writer alone over existing tiles_tmp (no station model, no side files); slices one region at a time, so do not run several in parallel
rail-extract/target/release/rail-extract build paris chengdu greater-bay-area cn-beijing --no-tiles
#   side files only: the app draws these regions from China's tiles (0.5 s for all three)
#   side files per region are hash shards (stations/, lines/, tags/, name-hl/ plus shards.json), a few to a few hundred files, not one per record; `build` clears and rewrites them.
#   Output from before 2026-10-16 (one file per station and line): node repack_side_files.mjs ../app/public/data converts it in place without a rebuild (30 s for the world output; it skips folders that already have shards.json)
#   also writes ../app/public/data/<slug>/stats.json (region card); areas inside a region: "stat_areas" (a list) or "stat_auto" (all provinces and cities of a country) in config/<slug>.json;
#   run `outlines --level 4` and `--level 5` first; also writes shapes.json (outlines for finding the area under the view)
#   or: rail-extract cut <slugs> --build   (cut, then build straight away; --no-tiles applies to every slug)
rail-extract/target/release/rail-extract places --parts ../app/public/data/pilot.parts.json [--data ../data] [--out ../app/public/data] [--cache FILE]
#   city, town and country labels -> places.json and the search index -> search.json in <out>; no network (0.3 s for the pilot, 0.6 s for the world). It replaced places.py (517 OpenFreeMap tiles, 15 min, five countries).
#   `build` runs it by itself after the pilot join and `world` after the tiles, so run it by hand only after changing the code or the outlines; positional slugs instead of --parts also work.
#   Reads: the stations of each region (<data>/<slug>/complexes.geojsonl: run `build` first), the place=city / town nodes of the cache, and the outlines and index.json in data/regions (`outlines --level 2` and `4`, and 5 for China, first).
#   Rules: a city is kept when a station complex is within 12 km of its node, a town within 5 km (n_stations = how many); countries: each region's country, labelled at the centroid of its largest polygon;
#   rank (1 is the biggest; the style's bands are 0-3, 4-7, 8+) from population (>= 10 M 1, 5 M 2, 3 M 3, 1.5 M 4, 1 M 5, 500 k 6, 250 k 7, 100 k 8, 50 k 9, 20 k 10, 10 k 11, else 12; no tag: city 10, town 12), one better for a national capital, towns never better than 8.
#   Full list in the header of rail-extract/src/places.rs. search.json: [name, name_en, local_name, kind (country|province|city|town), lon, lat, minLon, minLat, maxLon, maxLat, zoom, slug, n_stations]:
#   the countries of the regions and the provinces / states (level 4) that hold a station complex (outline box, fitting zoom, outline slug), and the CITIES: places.json's labels keep every city and town, but search.json has no towns and only cities with a metro or light rail system:
#   a place=city (or a level-5 outline of China) with a station complex within 12 km and open passenger track typed `metro` or `light_rail` (not construction, not proposed; monorail alone does not count) within 12 km of its point (exact distance to the track, not a grid);
#   one entry per metro area: candidates are sorted by rank, then population, then capital flag, then n_stations, and taken greedily, dropping any within 20 km of one already taken (Tokyo, not Shinjuku). A level-5 outline that holds candidate places becomes ONE entry with the outline's box and slug and the best place's name, rank and point (the other places inside it are no entries); level-5 outlines that are districts of a city-province (Hong Kong Island, Kowloon, New Territories) are dropped and level-5 outlines are no longer listed as provinces.
#   City outlines for the point-only cities: `places` writes <data>/city-outlines-wanted.json; `rail-extract outlines --cities <that file>` writes data/regions/<cc>-<city>.geojson with `city: true` in index.json from the cache (the cache keeps the boundaries of admin levels 6 to 9 whose Wikidata id is a place=city node's: rebuild it once after pulling this change); then `build` and `places` again (docs/14-city-outlines-and-highlights.md).
#   A country with under 3,000 km of open passenger rail (<out>/<slug>/stats.json) and no city entry is listed without its provinces (world: 57 countries, 1,370 provinces left).
#   Needs <data>/<slug>/features.typed.geojsonl for the metro test. 2026-10-15 world: 146 countries, 1,674 provinces, 726 cities (63 from level-5 outlines), 255 KB (92 KB gzipped); the 1,674 provinces become 1,370 with the small-country rule; pilot: 5, 116, 194 (63), 32 KB.
python3 check.py                                 # phase 2 "Done when": answer lists in answers/
python3 measure_views.py                         # compressed tile bytes per view vs the 600 KB budget
python3 top.py <slug> [n]                        # dev helper: biggest stations
```

Map labels need no build step: the app draws all text locally from system fonts (`docs/00-overview.md`, row 2026-10-17), so there are no glyph files.

All six regions, cut to tiles: 42 s (2026-10-13, quiet machine: cut 1.1 s, tiled builds 40.9 s, side-file builds 0.5 s).
With the Python steps it was 106 s. Rust, without tippecanoe, per region (these exclude the tile step):

| Region | Python (stations + con_stations + build_tiles) | Rust build |
| --- | --- | --- |
| SF Bay Area | 0.74 s | 0.15 s |
| Paris | 3.8 s | 1.07 s |
| Chengdu | 0.62 s | 0.14 s |
| Greater Bay Area | 2.3 s | 0.45 s |
| Beijing | 1.09 s | 0.24 s |
| China | 45.5 s | 11.1 s |

History, tippecanoe era (everything below up to the Python timings used tippecanoe and `tile-join`, both removed 2026-10-14; the tippecanoe and `tile-join` figures are no longer reproducible with this code).

Germany (`de`, whole country, added 2026-10-13): cut 7.4 s (335,680 features), build 47.4 s (station model 3.5 s, tile input 9.2 s, tippecanoe 32.5 s -> 92.8 MB), side files 139 MB (app/public/data/de).
`tile-join` of all four sources: 20 to 28 s, pilot.pmtiles 163 MB -> 256 MB. Joining lifts Paris's z4 and z5 view from 13 and 60 KB to 146 and 501 KB, because Germany's low-zoom tiles share those tiles.

France, Switzerland, Italy (`fr`, `ch`, `it`, whole countries, added 2026-10-13; Paris now draws from France's tiles and is built with `--no-tiles`). Cut of the three together 1.3 s (cache already read). Build per country, in parallel (total 71 s with the join):

| Step | France | Switzerland | Italy |
| --- | --- | --- | --- |
| read | 0.56 s | 0.55 s | 0.55 s |
| station model | 1.51 s | 0.45 s | 0.69 s |
| tile input and side files | 3.17 s | 1.08 s | 1.74 s |
| tippecanoe | 21.3 s (60.3 MB) | 7.2 s (14.9 MB) | 9.3 s (30.4 MB) |
| total | 27.0 s | 9.5 s | 12.6 s |

`tile-join` of the six sources: 43.9 s, `pilot.pmtiles` 256 MB to 349 MB. Country bounding boxes overlap, so the join no longer checks bounds; about 8 to 40 border-crossing ways per country pair are in both files. The app tells the files apart by outline (`partAt` in `regions.ts`, from each part's boundary file). Heavy views: z5 and z6 laptop views across Europe are now 590 to 1,030 KB (France, Switzerland and Germany tiles share them).

Older per-step Python timings, before classify moved into the cut:

| Region | classify | stations | con_stations | build_tiles |
| --- | --- | --- | --- | --- |
| SF Bay Area | 0.17 s | 0.41 s | 0.14 s | 1.33 s |
| Paris | 0.74 s | 1.90 s | 0.42 s | 5.91 s |
| Chengdu | 0.15 s | 0.35 s | 0.13 s | 1.07 s |
| Greater Bay Area | 0.55 s | 1.21 s | 0.36 s | 3.28 s |

