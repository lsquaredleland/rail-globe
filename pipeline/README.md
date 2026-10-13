# Pipeline

Run from `pipeline/`. Needs Rust 1.99 (extraction), Python 3 with shapely and numpy, and tippecanoe.
Timings below were measured on 2026-10-12 (14-core Mac, planet-260928).

```
cd rail-extract && cargo build --release && cd ..        # once, and after changing the Rust code (5 s after a change)
rail-extract/target/release/rail-extract cache --planet ../planet-260928.osm.pbf
#   once per planet: 51 s, 4.5 GB peak memory -> ../data/cache/planet-260928.railcache (1.1 GB; blob index cached beside it)
rail-extract/target/release/rail-extract cut sf-bay-area paris chengdu greater-bay-area
#   regions cut in parallel from the cache, by their outline regions/<slug>.geojson, and classified (rail types) with the
#   rules in config/<slug>.json: all six regions including all of China in 1.6 s
#   writes ../data/<slug>/{features.geojsonl, relations.jsonl, stop_positions.jsonl, qa.json, airports.geojsonl,
#                          features.typed.geojsonl, lines.json, classify_report.json}
#   rail type rules: rail-extract/src/classify.rs (ported from classify.py 2026-10-12, same output byte for byte)
rail-extract/target/release/rail-extract build sf-bay-area paris china
#   per region, in parallel: station model (stations, complexes, size tiers), stations under construction, tile input,
#   tippecanoe and side files into ../app/public/data (was stations.py, con_stations.py, build_tiles.py; ported 2026-10-13)
#   China: 11.9 s before tippecanoe (station model 6.4 s), tippecanoe 28.5 s
rail-extract/target/release/rail-extract build greater-bay-area chengdu cn-beijing --no-tiles
#   side files only: the app draws these regions from China's tiles (0.5 s for all three)
#   also writes ../app/public/data/<slug>/stats.json (region card); areas inside a region: "stat_areas" (a list) or "stat_auto" (all provinces and cities of a country) in config/<slug>.json;
#   run `outlines --level 4` and `--level 5` first; also writes shapes.json (outlines for finding the area under the view)
#   or: rail-extract cut <slugs> --build   (cut, then build straight away; --no-tiles applies to every slug)
python3 check.py                                 # phase 2 "Done when": answer lists in answers/
python3 measure_views.py                         # compressed tile bytes per view vs the 600 KB budget
python3 top.py <slug> [n]                        # dev helper: biggest stations
```

All six regions, cut to tiles: 42 s (2026-10-13, quiet machine: cut 1.1 s, tiled builds 40.9 s, side-file builds 0.5 s).
With the Python steps it was 106 s. Rust, without tippecanoe, per region:

| Region | Python (stations + con_stations + build_tiles) | Rust build |
| --- | --- | --- |
| SF Bay Area | 0.74 s | 0.15 s |
| Paris | 3.8 s | 1.07 s |
| Chengdu | 0.62 s | 0.14 s |
| Greater Bay Area | 2.3 s | 0.45 s |
| Beijing | 1.09 s | 0.24 s |
| China | 45.5 s | 11.1 s |

Older per-step Python timings, before classify moved into the cut:

| Region | classify | stations | con_stations | build_tiles |
| --- | --- | --- | --- | --- |
| SF Bay Area | 0.17 s | 0.41 s | 0.14 s | 1.33 s |
| Paris | 0.74 s | 1.90 s | 0.42 s | 5.91 s |
| Chengdu | 0.15 s | 0.35 s | 0.13 s | 1.07 s |
| Greater Bay Area | 0.55 s | 1.21 s | 0.36 s | 3.28 s |

Region outlines. Countries and provinces / states come from the planet (admin levels 2 and 4, kept in the cache, simplified
to about 20 m):

```
rail-extract/target/release/rail-extract outlines --level 2                # 218 countries: ../data/regions/cn.geojson, us, fr ... (0.4 s)
rail-extract/target/release/rail-extract outlines --level 4                # 3,062 provinces / states: cn-sichuan, us-california ... (0.8 s)
rail-extract/target/release/rail-extract outlines --level 4 --country CN   # just one country's
rail-extract/target/release/rail-extract outlines --level 5 --country CN   # 342 Chinese prefecture-level cities (cn-chengdu ...); needed by the region cards
#   (levels 2, 4 and 5 are in the cache; adding a level means rebuilding the cache, 47 s)
rail-extract/target/release/rail-extract cut us-california cn-sichuan      # then cut any of them (about 1 s)
```

`../data/regions/index.json` lists every outline (name, English name, level, country, ISO code, OSM relation). Hand-made outlines
in `regions/` win over generated ones with the same slug: `sf-bay-area` (9 counties), `paris`, `chengdu`, `greater-bay-area`
and `china` (mainland + Hong Kong + Macao) came from Nominatim (`build_regions.py` unions the boundary dumps in `regions/*.json`;
`outline_cn.py` looks up the China ones, 47 s). Cities and counties (levels 5 to 8) are not in the cache yet.

New region: an outline (generated, or a hand-made GeoJSON Feature in `regions/<slug>.geojson`) and `config/<slug>.json`
(classify needs it), then `cut <slug>`; the planet is not read again.
Changing which tags are kept (`rail-extract/src/rules.rs`, `cache_key`) needs a new cache; changing the rules for what is cut
only needs a rebuild of the binary and a new cut.

App (phase 3): `cd ../app && npm install && npm run dev`, then open http://127.0.0.1:5173/ . `npm run build` makes a static site in `dist/`.
Share links keep state in the URL hash: `#r=paris&c=operator&con=0&fr=1&u=mi&v=12.6/48.8785/2.3553` (region, colour mode, toggles, units, view).

Per-region rules live in `config/<slug>.json`: `high_speed_networks_re`, `type_overrides`, `high_speed_track_overrides` (Chinese 200 km/h lines that count as high-speed), `proposed_region` (a `regions/` boundary file inside which `railway=proposed` track is kept; none by default). `commuter_networks` is kept but unused while Commuter is merged into Regional.

Generated files (`data/`, `app/public/data/`, `*.osm.pbf`, `rail-extract/target/`) are git-ignored; rebuild them with the commands above.
Answer lists in `answers/` are hand-made drafts; weights and tier cut-offs are at the top of `rail-extract/src/stations.rs`.
