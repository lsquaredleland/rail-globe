# Archive

Things kept for history. Nothing here is run by the pipeline, the app, `publish.sh` or CI. Moved on 2026-10-20 (cleanup). `git log --follow <file>` shows the history.

| File | What it was | Why it is archived |
| --- | --- | --- |
| `names.py` | Python version of the station and line name rules | Ported to Rust (`rail-extract/src/rules.rs`, names section) |
| `build_regions.py` | Unioned the Nominatim dumps in `regions-raw/` into the pilot outlines (`pipeline/regions/<slug>.geojson`) | The outlines are committed; needs shapely; paths are relative to `pipeline/` |
| `outline_cn.py` | Looked up the China, Hong Kong and Macau outlines on Nominatim (47 s) | Same; its outputs are in `regions-raw/cn/` and `pipeline/regions/` |
| `repack_side_files.mjs` | Converted side files from one file per record to hash shards, in place (2026-10-16) | `build` writes shards now |
| `station_viewer.template.html` | Prototype station viewer (phase 1) | Replaced by the app |
| `bench/lowzoom/` | Python scratch tools of the low-zoom tile weight study (`docs/16-low-zoom-tile-weight-plan.md`) | `exp.py` needs a `reenc` binary and `ladder_scan.py` a `scan` module, neither is in the repo; `mv.py` became `pipeline/measure_views.py` |
| `regions-raw/` | Raw Nominatim boundary dumps (`idf.json`, `chengdu.json`, `ba_*.json`, `cn/*.json`) | Only the archived scripts read them. `pipeline/regions/cn/hk.json` stays in place: `proposed_region` in the China configs reads it |
