# Americas passenger-rail QA

> **Historical log: the commands below (`build --combined`, `pilot.pmtiles`) are removed and are not current instructions.** **Status 2026-10-15: code reverted, docs kept.** The user said the point was missed: the job is correct freight labelling (too many lines drawn as passenger). All code from this work was reverted on branch `americas-qa` (commits "Revert ..." on top of `9ca7bdb`) and in the working tree of the main checkout: the classification rules and Americas configs, `build --combined`, `tiles-rs`, the Americas tile source in the app, and `pipeline/qa` (`shoot.mjs`, `views.json`, `fetch_narn.py`, `narn_check.py`). The scripts and options named below no longer exist in the tree; they are in git history at `76f3740` (classification), `74d71fc` (combined build, tile writer), `d034553` (app wiring) and `9ca7bdb` (QA tooling). The requirement, the findings and the acceptance tests are in `07-freight-detection.md`, section "Americas: correct freight labelling"; this file is the log of what was measured and seen.


Ongoing, multi-pass (started 2026-10-14). Goal: a consistent way to tell passenger rail from freight across North and South America, checked by eye on screenshots of the real map and by comparing against independent passenger-service data. This file is the working log: each pass adds a section at the bottom. Rules that come out of a pass go to `docs/07-freight-detection.md` and a dated row in `docs/00-overview.md` (Decisions win over this file).

## Method (every pass)

1. **Views.** `pipeline/qa/views.json` lists named map views (US metros and corridors, Canada, Mexico, South America). Each view is shot twice by `pipeline/qa/shoot.mjs`: the default (passenger only) and `fr=1` (freight shown), as PNGs in `data/qa/pass-N/` (git-ignored, regenerated).
2. **Judge.** Each screenshot pair is looked at and every drawn line is classed from real-world knowledge as: real passenger service, passenger only in part, not passenger (freight, industrial, tourist-only, abandoned-but-mapped). Findings go in the pass table below: view, line or area, what the map shows, what is true, evidence, action.
3. **Reference data.** Findings are checked against independent passenger data where it exists (see "Ground truth"), not only memory.
4. **Fix by rule, not by line.** A finding that repeats becomes a rule or a config setting in `classify.rs` / `config/<cc>.json`; a one-off becomes a `track_type_overrides` entry. After each fix: rebuild the Americas (`cut` the 26 slugs, `build --combined americas`), confirm the pilot regions' outputs are byte-identical, re-shoot the same views, and compare.
5. **Never lose a pass.** The pass table records what was changed and the km by type before and after, so a later pass can see what moved.

## Ground truth (what "passenger" means here)

A way is passenger when real scheduled public passenger trains use it. Not passenger: freight, industrial, military, yard and siding, tourist or heritage lines (type Other), abandoned or unused track still mapped.

Sources, best first (status in each pass):
- OSM route relations (`route=train|subway|light_rail|tram|monorail|funicular`) with `network` / `operator`: the primary signal; already used by the pipeline.
- Station tags (`public_transport=station`, `train=yes`, `network`, UIC / agency ids).
- External lists: US BTS NTAD (Amtrak routes and stations; North American Rail Network lines carry a passenger-use attribute), Canadian and Mexican agency data. To be checked in pass 1: availability, licence, field meaning. External data is a cross-check, not the map's source (OSM is).
- Knowledge of the real network: Amtrak and VIA routes, commuter agencies, metros and light rail by city.

## Open questions
- Unconnected strands: short pieces typed Intercity/Regional that connect to no passenger-typed track, carry no route and sit beside no passenger station. Candidate rule: connected components of passenger-typed track with no route, no station and under N km become Freight (or are dropped, as stray track under 100 m already is). N and the drop-or-retype choice are decided from pass-1 screenshots.
- Línea FA (Mexico, Interoceánico network): split 100 km Freight / 229 km Intercity by accident of the name pattern; decision pending.
- Mexico Línea Morelos and Línea Juárez (about 235 km each) still Intercity: freight concessions?

## Passes

### Pass 1a (2026-10-15): BTS NARN cross-check, US and Canada (measurement only, no rules changed)

Tool: `pipeline/qa/fetch_narn.py` (downloads the passenger-coded segments of NTAD North American Rail Network Lines, public domain, 22,200 segments, plus 1,036 Amtrak stations, into `data/ext/ntad/`) and `pipeline/qa/narn_check.py` (share of each OSM track way within 30 m of NARN passenger track, sampled every 20 m; output in `data/qa/narn/<slug>/`: summary.md, disagreements.geojson, three group CSVs). NARN codes A, B, C, D, E, I, R, V, O count as passenger; T (tourist) and NET X/A/R/T segments do not. Run: US 72 s, Canada 27 s. State of the rules after the 2026-10-14 freight/high-speed changes.

| | US | Canada |
| --- | --- | --- |
| NARN passenger km covered by our passenger-typed track (A / B / C / V) | 99% / 99% / 97% | 98% / 98% / 91% / V 94% |
| Our intercity track overlapping NARN passenger by 60% or more | 91% of 49,804 km | 93% of 15,146 km |
| Our regional track overlapping NARN passenger by 60% or more | 41% of 26,635 km (58% under 10%) | 32% of 5,765 km (67% under 10%) |
| Our freight km that NARN calls passenger (60% or more) | 1,404 of 180,429 km | 1,605 of 26,488 km |

Reading (the implementer's judgement, not verified line by line; metro and light rail are meaningless here, NARN has only 330 km of rapid transit):
1. **Regional is where we are loose.** About 58% of US regional km (about 15,000 km) has no NARN passenger code under it: tag-typed short-line freight (Paducah and Louisville, Grainbelt, Farmrail, Portland and Western, Reading Blue Mountain and Northern) and the largest block, 2,389 km of unnamed regional track with no operator. Names with "Freight Line" / "Freight Main Line" and NS "... Line" names (Southern Tier, Buffalo, Youngstown) are in this class: not caught by the name pattern because they lack Subdivision, District and the like.
2. **Recall gap: real Amtrak trackage is typed Freight.** UP Beaumont, Dallas, Austin and Coast Subdivisions and BNSF Spokane, Fort Worth and Seligman (Sunset Limited, Texas Eagle, Coast Starlight, Empire Builder; about 400 km) carry NARN code A but have no passenger route relation of their own and no station, so the name rule makes them Freight. Smaller mis-typed Freight on the Northeast Corridor (35 km), Metro-North Hudson Line, NJ Transit Atlantic City Line and MBTA New Bedford Main Line.
3. **Canada.** Algoma Central Soo (392 km), Ontario Northland Temagami, Newmarket and Ramore, and the Gaspé lines are Freight in our data but coded passenger in NARN; probably stale NARN codes for suspended service (Northlander, VIA Chaleur), except Newmarket (VIA) and Sussex, Edson, Halton, which need a check. Rocky Mountaineer lines (Mountain, Laggan, Prince George) show up as our passenger and NARN none: tourist, should be Other.
4. **Route-typed intercity with NARN none** (UP Elko on the California Zephyr path, Parry Sound, Carlton Trail) needs a manual check: NARN may lack the code, or a tourist/heritage route relation is being treated as service.

Actions for the next change (not done yet): protect ways with a passenger-route neighbour or NARN-style trackage rights; widen the freight name pattern ("Freight", "Line" with a freight operator) for the unnamed/short-line regional block; mark Rocky Mountaineer, Via-excluded tourist lines as Other.

### Pass 1b (2026-10-15): first screenshot pass, 54 views (data/qa/pass-1, fork `americas-qa`)

Tool: `pipeline/qa/shoot.mjs` + `views.json` (54 views; passenger-only and freight-shown shots; all rendered, 0 map errors). Judged by eye: 8 of the 54 by the reviewer (us-overview, texas-triangle, plains-southwest-chief, cuba, argentina-overview, brazil-overview, dominican-republic, mexico-overview); the rest only from the implementer's per-view descriptions and a pixel diff of passenger vs freight shots, so they still need a real look in pass 2.

What the map shows against what is true:

| Area | Map shows | Realistic | Verdict |
| --- | --- | --- | --- |
| US long-distance (Intercity, orange) | Amtrak network incl. Sunset Limited and Texas Eagle trackage (the 2026-10-14 trackage-rights fix shows: Houston-Beaumont-Lake Charles is orange) | Amtrak routes | Good |
| US short lines (Regional, yellow): west of Fort Worth, Kansas, Oklahoma, Nebraska, Dakotas | scattered yellow branches | short-line freight (Fort Worth and Western, Kansas and Oklahoma RR, Grainbelt, Farmrail ...) with no passenger service | **Wrong: freight shown as Regional** (agrees with NARN: 58% of US regional km has no passenger code) |
| Cuba | one dense yellow network over the whole island | passenger trains run on the Havana-Santa Clara-Camaguey-Santiago main line, Havana-Pinar del Rio and a few more; the rest is sugar and industrial track | **Overdrawn** |
| Argentina | dense orange network over the pampas, NW, Patagonia | Buenos Aires suburban lines and a few long-distance (Mar del Plata, Rosario, Cordoba, Tucuman, Bahia Blanca, Tren del Valle); most of the rest is Belgrano Cargas / Ferroexpreso Pampeano freight | **Overdrawn: freight shown as Intercity** |
| Brazil | orange in the SE, NE, Midwest | passenger: EFVM Vitoria-Minas, EFC Carajas, metros and CPTM; MRS, FCA, Transnordestina and the rest are freight | **Overdrawn** |
| Uruguay, Chile (south of Chillan), Peru, Bolivia | orange spider lines | AFE passenger is Montevideo suburban only; EFE passenger ends near Chillan; Peru is PeruRail tourist plus freight | **Overdrawn** |
| Dominican Republic | orange and yellow lines in the east (La Romana, San Pedro) | sugar railways (freight); only the Santo Domingo metro is passenger | **Wrong** |
| Mexico | Tren Maya loop, Interoceanico, Tren Interurbano and AIFA fragments, Chepe in yellow | all real | Good after the 2026-10-14 Mexico rule |

Cause (common): outside the US, Canada and Mexico, passenger route relations are sparse, so track is typed from `usage=main` / `branch` alone (default Intercity / Regional) and nothing demotes it unless it has a dead end under 15 km or a matching name. In the US the same thing happens to short lines.

Next (pass 2): make "evidence of passenger service" required for unrouted track, and measure it. Ideas to test, each with numbers against NARN (US, Canada) and with screenshots (rest):
1. Config option for the Americas: an unrouted way typed only from `usage`/`railway` tags becomes Freight when no passenger station serves its railway within the shield distance (the existing name rule without the name pattern, for unnamed railways too); US/CA scored by NARN precision and recall.
2. Unconnected strands: connected components of passenger-typed track with no route and no station, under N km, become Freight; N chosen on the pass-1 images (candidates: 5, 10, 25 km).
3. Station quality for South America and the Caribbean: which stations count as passenger (the China 12306 list has no equivalent; use `public_transport=station` plus `train=yes` or a passenger operator).
4. Full 54-view look, all of them by the reviewer, plus contact sheets.

### Pass 2 (started 2026-10-15, STOPPED before any measurement)

Stopped on request while the MLT tile conversion is finished in another session. State left in the fork (`/Users/leland/Downloads/osm-rail-qa`, branch `americas-qa`, nothing committed):
- Code written, compiles (`cargo check`), default off, never run and never measured: `freight_unrouted_no_station` (E1), `freight_isolated_km` (E2), `passenger_station_level` = strict | operator | any (`cut.rs`, `classify.rs`), and an env override `RAIL_CFG_SET='{"freight_isolated_km":10}'` that replaces config keys for every region loaded, so options can be measured without editing configs.
- Not verified: that the 9 pilot regions' outputs (classify_report.json, features.typed.geojsonl, lines.json) are still byte-identical with the pass-2 code in. Check this first.
- No pass-2 numbers, no rebuilt tiles, no pass-2 screenshots.

## Status summary (2026-10-15)

Done and in the fork: Americas classification rules for the US, Canada and Mexico (see docs/07), `build --combined` (one tippecanoe for all countries: 96.5 s for 26 countries), the app loading `americas.pmtiles` as a second source, screenshot tooling, NARN cross-check, pass 1a and 1b above.

Known good: Amtrak and VIA long-distance routes, US commuter and metro systems, Mexico (Tren Maya, Interoceanico, Suburbano, Chepe), Houston-Beaumont-Lake Charles trackage rights. Known wrong: short-line freight as Regional in the US and Canada; South America, Cuba and the Dominican Republic drawn mostly as passenger because their track is typed from `usage` alone; real passenger lines near freight in Canada (Ontario Northland, Algoma, Gaspe) unresolved. Pre-existing and unrelated: `check.py` fails on Chengdu station 成都 (tier 3, wanted 1 or better), also at the last commit.

## What we should do next (in order)

1. **Wait for the MLT conversion, then merge.** The fork is cut from `913ddff` with MVT tiles. After the MLT work lands on `main`, merge `main` into `americas-qa` (conflicts are likely only in `tiles/`, `mapStyle/`, `data.ts`), rebuild the Americas, re-shoot (default encoding, no `?enc=` needed), and compare with `data/qa/pass-1`. Classification is upstream of tile encoding, so none of the rule work needs redoing; only tile property names the style reads (`type`, `con`, ids) could differ.
2. **Check the pilots are unchanged** with the pass-2 code in (md5 of the three files per pilot). If any differ, the new options are not default-off: fix before anything else.
3. **Finish pass 2** as designed: measure E1 and E2 (N = 5, 10, 25 km) on US and Canada against NARN (km of our passenger types with NARN overlap at least 60% or under 10%, passenger km lost, top groups), and on the rest of the Americas by listing the 30 largest changed railways per country and checking each against real-world knowledge. Real services that must survive: Argentina Roca, Sarmiento, Mitre, San Martin, Belgrano Sur, Urquiza and the Tucuman, Mar del Plata, Bahia Blanca and Rosario trains; Chile EFE Metrotren, Biotren and Tren Central to Chillan; Brazil EFVM, EFC Carajas, CPTM, SuperVia, Recife and Natal CBTU; PeruRail and Ferrocarril Central (tourist, so Other); Panama Canal Railway; Uruguay AFE suburban; Cuba's main line. Decide whether stations are tagged well enough in each country for the evidence rule (`passenger_station_level`), and per country group which setting to use.
4. **Look at all 54 views by eye** (only 8 were judged by the reviewer in pass 1), with contact sheets, then add more views where networks are dense (Northeast, Midwest freight corridors, Southern Ontario and Quebec, Chilean central valley, Brazilian southeast).
5. **Add independent data for places NARN does not cover.** GTFS shapes as a general passenger check: VIA (checked: public, OGL-Canada), GO, exo, TransLink; Trenes Argentinos, EFE, Brazilian metros (find the feeds on the Mobility Database); BTS National Transit Map routes for US light rail, metro and commuter rail (NARN has only 330 km of rapid transit); Mexico SICT/ARTF network layer as a network mask; Chile BCN network. Wire each into a `pipeline/qa` script like `narn_check.py` so every pass can re-run it; keep external data out of the map itself (OSM stays the source).
6. **Open rule decisions** (ask the user): Linea FA (partly Freight by accident), Linea Morelos and Linea Juarez (Mexico, still Intercity), Intercity versus Regional for Amtrak corridor services, the 5 km shield and the 20 m beside-route distance, tourist lines as Other, Canadian Ontario Northland / Algoma / Gaspe (NARN codes may be stale).
7. **Regression habit each pass:** rebuild the 26 slugs, `build --combined americas`, pilots byte-identical, `check.py` (only the Chengdu failure allowed), `narn_check.py us ca`, `shoot.mjs <N>`, then log a section here with km by type before and after.
8. **App follow-ups from the Americas wiring:** San Francisco Bay Area is in both `pilot.pmtiles` and the US tiles and is drawn twice (decide: drop it from the pilot tiles or cut it from the US); test hover, click and line highlight with real input in the Americas source; check `places.json` has Americas place labels; upload `americas.pmtiles` and `americas.parts.json` to R2 before deploying (the app fails at startup without them); per-country centres and zooms in `regions.ts` are guesses.
9. **Housekeeping before commits:** split into separate commits (classification rules, `--combined`, Americas configs, app wiring, tile-writer spike, QA tooling and docs); Opus review of every Sonnet change before commit; remove the dead code the second review found; decide whether `pipeline/qa/*` and `data/ext` stay out of git (the data does); `app/node_modules` in the fork is a symlink, do not commit it.
10. **Tile writer (separate track, from the 2026-10-14 spike):** `tiles-rs` works on `ch` (3.3 s vs 7.2 s) using geojson-vt-rs, fast-mvt and pmtiles; before it can replace tippecanoe it needs spatial partitioning (4 GB for 70 MB of input), a decision on the low-zoom overview (short lines dropped, heavier coalescing), and a visual comparison. It writes MVT; if MLT wins the benchmark, `mlt-core` encodes from the same model. Revisit after the MLT work.
