# Rail Globe

A static web map of passenger rail built from OpenStreetMap, made for comparing two regions side by side at the same real scale (e.g. SF Bay Area vs Paris).

## Where things are

| File | What it holds |
| --- | --- |
| `docs/00-overview.md` | Goal, scope, every decision made so far (dated) |
| `docs/01-features.md` | What the user sees: colour modes, rail types, basemap, labels, outlines, compare view |
| `docs/02-architecture.md` | Stack, data layers, stable ids, pipeline, tile layers, zoom rules |
| `docs/03-data-spec.md` | Exactly what the OSM filter keeps and drops, plus its implementation checklist |
| `docs/04-station-model.md` | How stations are grouped (complex / station / stop point) and ranked by size |
| `docs/05-pilot-plan.md` | Pilot regions and phases 0 to 5, each with a "Done when" test |
| `docs/06-open-questions.md` | Provisional choices the code made where the docs were silent |
| `docs/07-freight-detection.md` | How freight is told from passenger track in OSM: the research, the ordered rules, the settings, the known failures |
| `docs/08-americas-passenger-qa.md` | Americas passenger-rail QA: what was checked, what was fixed, what is left |
| `docs/09-performance-playbook.md` | What made the map fast (MLT writer, feature state, hover), what did not, how to benchmark, and the open problems (duplicate range requests) |
| `docs/10-world-build.md` | The world build: commands, measured times and sizes, `search.json` rules, memory-bounded writer |
| `docs/12-config-and-exemptions.md` | Per-region configs: `extends` layering, the `exemptions` list (schema, stale detection) |
| `docs/13-freight-north-america.md` | Study: freight or passenger in the US, Canada and Mexico against NARN, the rule that came out of it (proposal) |
| `docs/14-city-outlines-and-highlights.md` | City outlines scope, why some lines have no highlight, small countries as country level only |
| `docs/15-argentina-africa-freight.md` | Study and rule: passenger needs evidence in Argentina and Africa (routes mode, stations mode, `_groups.json`) |
| `docs/16-low-zoom-tile-weight-plan.md` | Measured plan to bring z2 to z8 views under 600 KB, ordered by payoff (scratch scripts in `pipeline/archive/bench/lowzoom/`) |
| `docs/17-action-items.md` | The one to-do list: open action items, decisions that are the user's call, performance and hosting items, parked features |
| `docs/archive/` | Superseded decisions, closed open questions, closed performance to-dos (evidence, not rules) |
| `docs/reference/pipeline-timings.md` | Pilot-era command log and timings (history) |
| `CHANGELOG.md` | What changed, newest first |
| `pipeline/README.md` | Command reference for `rail-extract` and the dev checks |
| `pipeline/archive/` | Dead scripts and raw data, with a README |
| `docs/qa/` | Per-region QA notes (west Europe, east Europe, east Asia and Oceania, Americas, Africa and Asia) |
| `docs/reference/research-summary.md` | Key measurements from the original research (tile sizes, build times, OSM coverage) |

## How to work

- Work phase by phase from `docs/05-pilot-plan.md`. A phase is done only when its "Done when" column passes.
- Phases 0 to 2 are done, phase 3 and 5 are not met, phase 4 (Compare) was skipped: work on the open phases and on `docs/17-action-items.md` (the one to-do list).
- `docs/00-overview.md` (Decisions) wins over every other doc if they disagree. Flag the conflict instead of guessing.
- Anything not covered by the docs: check `docs/17-action-items.md` and `docs/06-open-questions.md`; if it is not there, ask before deciding.
- Do not add GTFS, live data, or any server component. v1 is fully static.
- After any change that can alter the data, run the data checker and report the result before saying it is done: see "Automatic data check" in `docs/05-pilot-plan.md` (answer lists, major stations, random city spot checks, aggregates, diff against the last build).
- The app reads only the world build, so after a change that can alter the data, rebuild the world (default, since 2026-10-17); use the three pilot extracts (SF Bay Area, Chengdu, Paris) for specific testing. Publishing (`pipeline/publish.sh`) deploys the live site and still needs the user's go.

## Stack (summary)

- Pipeline: Rust extractor (`pipeline/rail-extract`: planet -> world rail cache -> region cuts, including rail type classification; `build`: station model, tile input, side files, MLT tiles through the Rust writer `tilewriter.rs`), PMTiles; no tippecanoe
- App: Vite + React + TypeScript, MapLibre GL JS, OpenFreeMap basemap restyled minimal
- Hosting: Cloudflare R2 for tiles and side files (live on the `r2.dev` public URL, custom domain still to do), Vercel for the app (`rail-globe.vercel.app`); the app reads only the world build; `pipeline/publish.sh` runs raw data to live site

## Data rules that are easy to get wrong

- Keep the OSM id on every feature (`osm_id`, e.g. `way/123`). Ids must survive monthly rebuilds.
- Station rank and zoom come from physical size only. Never from line counts.
- Infrastructure is built first and never depends on the service layer.
- Abandoned and disused track is not in the data; proposed track is kept for Hong Kong only (a test, off by default). Construction is kept (drawn dashed at every zoom from z4, from merged ways).
- OSM data is ODbL: attribution on the map; side files are offered under ODbL.
