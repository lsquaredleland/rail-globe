# Pipeline

Run order, per region (`sf-bay-area`, `chengdu`, `paris`). Needs Python 3 with osmium, shapely, numpy.

```
python3 build_regions.py                         # boundaries -> extracts.json (needs regions/*.json from Nominatim)
osmium extract -c extracts.json -s smart --overwrite ../planet-260928.osm.pbf   # phase 0, ~7 min
# faster outline for a new region: one Nominatim search per county (polygon_geojson=1, about 5 s each), union with shapely,
# write regions/<slug>.geojson and an extracts-<slug>.json with just that region (see `outline_cn.py` and `extracts-gba.json`), then run osmium on it alone (~5 min)
# China (timings 2026-10-11, logs in data/timing-*.log): python3 outline_cn.py (47 s), then
#   osmium extract -c extracts-china.json -s smart ../planet-260928.osm.pbf   (368 s, 1.72 GB: PRC + Hong Kong + Macao)
#   osmium extract -c extracts-gba.json -s smart ../data/china.osm.pbf        (7 s, 142 MB: 9 Pearl River Delta cities + HK + Macao)
python3 filter.py <slug>                         # phase 1: prefilter (osmium tags-filter) then C++ filter (cpp/filter.cpp); about 2 s per region
#   needs: brew install libosmium protozero  (the binary compiles itself on first run). filter_py.py is the old Python version, kept to compare
python3 classify.py <slug>                       # phase 2: rail types, lines.json
python3 stations.py <slug>                       # phase 2: stations, complexes, size tiers
python3 con_stations.py <slug>                    # phase 3: adds under-construction stations (run after stations.py)
python3 check.py                                 # phase 2 "Done when": answer lists in answers/
python3 top.py <slug> [n]                        # dev helper: biggest stations
python3 airports.py <slug>                        # phase 3: airport outlines and terminals from the extract
python3 build_tiles.py <slug>                    # phase 3: PMTiles + side files into ../app/public/data
python3 measure_views.py                         # phase 3: compressed tile bytes per view vs the 600 KB budget
```

App (phase 3): `cd ../app && npm install && npm run dev`, then open http://127.0.0.1:5173/ . `npm run build` makes a static site in `dist/`.
Share links keep state in the URL hash: `#r=paris&c=operator&con=0&fr=1&u=mi&v=12.6/48.8785/2.3553` (region, colour mode, toggles, units, view).

Per-region rules live in `config/<slug>.json`: `high_speed_networks_re`, `type_overrides`, `high_speed_track_overrides` (Chinese 200 km/h lines that count as high-speed), `proposed_region` (a `regions/` boundary file inside which `railway=proposed` track is kept; none by default). `commuter_networks` is kept but unused while Commuter is merged into Regional.

Generated files (`data/`, `app/public/data/`, `*.osm.pbf`, the compiled `cpp/rail_filter`) are git-ignored; rebuild them with the commands above.
Answer lists in `answers/` are hand-made drafts; weights and tier cut-offs are at the top of `stations.py`.
