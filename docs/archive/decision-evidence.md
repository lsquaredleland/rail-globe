# Decision evidence

Measurements, counts, test results and rejected options moved verbatim out of the Detail cells of `../00-overview.md` so that the decision log stays short. One heading per decision id. The rule itself stays in the decision log.

## 2026-10-17-glyph-free-labels

<a id="2026-10-17-glyph-free-labels"></a>

Rule: [2026-10-17-glyph-free-labels](../00-overview.md#2026-10-17-glyph-free-labels). Evidence, verbatim:

The style has no `glyphs` property; every `text-font` is a stack whose first family carries the weight and the rest are fallbacks (`['Noto Sans Bold'` or `'Noto Sans Regular', 'Noto Sans', 'Helvetica Neue', 'Arial', 'sans-serif']`, in `mapStyle/`), so MapLibre 6.14 draws every label on a canvas. `localIdeographFontFamily`, the startup glyph check, `pipeline/glyphs/`, the glyph upload in `r2/sync.sh` and the `glyphs` stage of `pipeline/publish.sh` are removed. Reasons: the world build made the CJK subset 1.3 to 1.8 MB per dense China or Japan view; the glyph files cost 409 KB gzipped at the first world view; in a side-by-side benchmark there was no visible difference in Latin, Hangul or Arabic. Cost, accepted: the first-zoom hitch roughly doubles (Chengdu East max frame 75 to 125 ms, one noisy pair), and typefaces and label collisions differ by OS (a viewer without Noto Sans gets Helvetica Neue, Arial or the platform sans-serif). `app/public/glyphs*` on disk and the glyph folders in R2 are leftovers nobody reads; the next `r2/sync.sh` in sync mode deletes the R2 ones

## 2026-10-16-short-piece-smoothing

<a id="2026-10-16-short-piece-smoothing"></a>

Rule: [2026-10-16-short-piece-smoothing](../00-overview.md#2026-10-16-short-piece-smoothing). Evidence, verbatim:

Rule 8 of `07-freight-detection.md`, `classify.rs` (smoothing after the per-track rules, before exemptions). A siding at a junction had blocked it: 41.8 km of Intercity pieces inside Regional lines in Germany, 15.5 km in Japan, 14.8 km in Poland, 7.3 km in France disappeared. Route-typed, override and exemption pieces are never smoothed. Tests: `short_piece_is_smoothed_past_a_siding`, `intercity_chain_between_regional_track_becomes_regional`. The chain rule retyped 1,157 pieces in Germany, 931 in Japan, 559 in France, 376 in Italy, 315 in Poland

## 2026-10-16-freight-north-america

<a id="2026-10-16-freight-north-america"></a>

Rule: [2026-10-16-freight-north-america](../00-overview.md#2026-10-16-freight-north-america). Evidence, verbatim:

Measured against the BTS North American Rail Network passenger inventory (`pipeline/qa/fetch_narn.py`, `study_ways.py`, `eval_rules.py`): US precision 26% to 95% at 98% recall (223,000 km typed passenger to 60,500), Canada 46% to 93% at 97%, Mexico 15,400 km to 3,000. Rejected: a station within 200 m of the way as evidence (1.4 points of precision for 0.2 of recall), gap filling by railway name (4 to 10 points of precision). `13-freight-north-america.md`

## 2026-10-16-construction-dash-geometry

<a id="2026-10-16-construction-dash-geometry"></a>

Rule: [2026-10-16-construction-dash-geometry](../00-overview.md#2026-10-16-construction-dash-geometry). Evidence, verbatim:

MapLibre's dash pattern restarts at every tile edge and stretches between whole zooms (drawn dash 10.5 to 20.5 px between z12 and z12.95), which showed as uneven dashes and stray fragments. Compared by screenshots (z7 to z14, dark theme, a Chinese high-speed line under construction) against the old dashes and a hollow line (solid with a narrower land-coloured line on top, rejected: nearly invisible at z7 to z10 and double track drew as two outlines). Cost: pilot.pmtiles 302.1 MB to 306.7 MB (+1.5%), world.pmtiles 1,165 MB to 1,177 MB (+1.0%), `con_draw` removed; China has 1.54 million dashes (2.07 million before the parallel-track merge); 44 more style layers (frame cost not benchmarked)

## 2026-10-15-highlight-layer-filter

<a id="2026-10-15-highlight-layer-filter"></a>

Rule: [2026-10-15-highlight-layer-filter](../00-overview.md#2026-10-15-highlight-layer-filter). Evidence, verbatim:

With MLT, MapLibre decodes every loaded tile on the main thread once a source has any feature state: 69/72 to 41/40 missed frames over the 5-view bench (two runs; `09-performance-playbook.md`). A filter change on `pilot:hls` reloads only that small source; on the main source it threw away decoded tiles (z9 Europe hover 4.1 ms per move, 0.6 ms with `pilot:hls`). Behaviour unchanged. `archive/open-questions-2026-10-20.md` (Perf: why MLT was slower)

## 2026-10-14-freight-evidence-stray-pieces

<a id="2026-10-14-freight-evidence-stray-pieces"></a>

Rule: [2026-10-14-freight-evidence-stray-pieces](../00-overview.md#2026-10-14-freight-evidence-stray-pieces). Evidence, verbatim:

Counts of ways typed Freight by evidence: China 6,517 (no station) + 200 (short spur) + 53 (override), Germany 3,106 + 48, France 1,920 + 21, Italy 1,360 + 7, Switzerland 222 + 7. The rule is a heuristic: it can wrongly hide a passenger branch whose stations are not mapped, which is why it is limited to branches under 15 km, and why a per-line override exists. Long freight-only lines with no tag and a station beside them (Fujiang) still need an override. Not yet checked against a reference list: see "Automatic data check" in `05-pilot-plan.md`

## 2026-10-14-mlt-only

<a id="2026-10-14-mlt-only"></a>

Rule: [2026-10-14-mlt-only](../00-overview.md#2026-10-14-mlt-only). Evidence, verbatim:

`rail-extract build` and `--combined` write MapLibre Tiles straight from `tiles_tmp` (`tilewriter/`: geojson-vt-rs slicing, `mlt-core` 0.20.4 encoding, PMTiles); the app reads every source with `encoding: 'mlt'`; pilot.pmtiles is one writer run over the pilot sources (no `tile-join`). Measured (`archive/open-questions-2026-10-20.md` item 11), on the pilot file the app draws from: worst laptop view 923 KB against 994 KB (Lyon z6), worst phone view 794 KB against 883 KB (Zurich z5), Paris file 13% smaller (6.5 against 7.4 MB); both still over the 600 KB budget, so the Europe low-zoom weight is not solved. Per-region files are a different file set (worst laptop 557 KB against 641 KB, not what the app loads). China tiles 4.1-4.9 s against 36.8 s (4.1 s is the median of 3 runs in the orig-vs-mlt benchmark, 4.9 s an earlier single run), Americas combined 7.3 s against 276 s, files 6 to 10% smaller; memory 5 to 9 GB against 0.4 GB (writer only; 14.1 GB for the full seven-region build), so regions are sliced one at a time. The warm frame bench, cold-cache and throttled-network results are in `09-performance-playbook.md`. No MVT, no fallback

## 2026-10-13-open-high-speed-on-top

<a id="2026-10-13-open-high-speed-on-top"></a>

Rule: [2026-10-13-open-high-speed-on-top](../00-overview.md#2026-10-13-open-high-speed-on-top). Evidence, verbatim:

Screenshots: 21 of 24 views pixel-identical; the change is that open high-speed now draws over high-speed construction dashes where they meet (before, the dashes cut the open line), which is the user's rule. Speed gain is within benchmark noise (headless 6x missed frames 65 → 52; 120 Hz 4x 62 → 56; 1x 4 → 5), so it is kept as a simplification

## 2026-10-13-stations-in-rust

<a id="2026-10-13-stations-in-rust"></a>

Rule: [2026-10-13-stations-in-rust](../00-overview.md#2026-10-13-stations-in-rust). Evidence, verbatim:

Compared with the Python steps on all six regions: route station orders, airports and every tile input layer except stations are byte-identical; stations identical for 99.7% (China 18,846 of 18,898). The rest are ties the Python version broke by set or shapely tree order (its own station names change between runs; Rust takes the lowest index), highlight line direction, and point positions within 1e-5°. Without tippecanoe, China: 11.1 s instead of 45.5 s. All six regions, cut to tiles: 42 s instead of 106 s; tippecanoe (China 28.5 s) is now most of it

## 2026-10-13-zoomed-out-legend-changes

<a id="2026-10-13-zoomed-out-legend-changes"></a>

Rule: [2026-10-13-zoomed-out-legend-changes](../00-overview.md#2026-10-13-zoomed-out-legend-changes). Evidence, verbatim:

Track length is track km: every mapped track counts, so double track is about twice the route length (OSM maps most double track as one way per track; route km would need parallel ways merged). Guangzhou's outline is the Nominatim polygon of relation 3287346 (`pipeline/regions/guangzhou.geojson`); Hong Kong's is the level-4 boundary. The card picks an area when its width is at least 20% of the view's. Metro sketch and dots add about 2% to the z4 tile; the worst view is still the z5 laptop view of Chengdu (668 KB, over budget before this change too)

## 2026-10-13-region-cards-china

<a id="2026-10-13-region-cards-china"></a>

Rule: [2026-10-13-region-cards-china](../00-overview.md#2026-10-13-region-cards-china). Evidence, verbatim:

OSM maps most double track as one way per track, so summing ways doubled everything (China metro 24,000 km). Method: the map is cut into 60 m cells and per rail type each touched cell counts once, as the average chord of a square at that angle; tracks closer than 60 m are one. Checked: China high-speed 44,800 km (published about 48,000), Beijing metro 966 km (about 880), Shanghai 825 (about 830), Guangzhou 812 (about 650), Hong Kong metro 216 (about 270): roughly 10-25% accuracy, and the card says so. Cities with fewer than 20 metro stations are mostly stations of a neighbour's metro (Ma'anshan, Putian) and are left out. Hong Kong Island, Kowloon and the New Territories are excluded (Hong Kong is one province-level card)

## 2026-10-13-germany-pilot-overview-z8

<a id="2026-10-13-germany-pilot-overview-z8"></a>

Rule: [2026-10-13-germany-pilot-overview-z8](../00-overview.md#2026-10-13-germany-pilot-overview-z8). Evidence, verbatim:

Frankfurt's z8 view was 1,154 KB (7,214 unmerged pieces in one tile), now 452 KB; Ruhr 850 to 335 KB; Germany 686 to 273 KB. Yards were already hidden until z12, so hiding them later would not have helped. Germany: cut 7.4 s, build 47 s, 92.8 MB; the pilot file grew from 163 to 256 MB, and joining raised Paris's z5 view from 60 to 501 KB because tiles at the same address are merged

## 2026-10-12-classify-in-rust

<a id="2026-10-12-classify-in-rust"></a>

Rule: [2026-10-12-classify-in-rust](../00-overview.md#2026-10-12-classify-in-rust). Evidence, verbatim:

Checked byte for byte against `classify.py` on all six regions (SF, Paris, Chengdu, Greater Bay Area, Beijing, China), including `track_type_overrides` and the Hong Kong proposed-track area. China: 0.16 s instead of about 10 s, with no JSON written and read back between the two steps. A cut without a config file still writes its other files and skips classification. Rule changes go in `classify.rs`. `classify.py` was then removed (its last version, with `track_type_overrides`, was never committed; `classify.rs` carries those rules)

## 2026-10-12-region-outlines-planet

<a id="2026-10-12-region-outlines-planet"></a>

Rule: [2026-10-12-region-outlines-planet](../00-overview.md#2026-10-12-region-outlines-planet). Evidence, verbatim:

Comparing two states or cities needs only their outlines. Planet-260928: 218 countries and 3,062 level-4 boundaries (US 56, China 32, France 18); the cache grew by 33 MB (16.8 M boundary points kept as 4.1 M). Loose ring ends within about 1 km are joined and reported (Poland and Ukraine: one 152 m gap on their shared border); 4 level-4 boundaries with broken data do not assemble. Île-de-France from the planet outline: 67 KB instead of 403 KB, area within 0.03% of the Nominatim outline, and the same cut except 3 rail ways on the border. Outlines may cross themselves by a few metres (not strict like station areas); the cut does not mind. Macau and the Hong Kong SAR relation used before are not level 2 or 4 (Hong Kong also has a level-4 relation, `cn-hong-kong`), so `cn` is the mainland only; the hand-made `china` outline still includes both. New regions also need a `pipeline/config/<slug>.json` before classify

## 2026-10-12-extraction-in-rust

<a id="2026-10-12-extraction-in-rust"></a>

Rule: [2026-10-12-extraction-in-rust](../00-overview.md#2026-10-12-extraction-in-rust). Evidence, verbatim:

Region membership follows osmium extract's "smart" rule. Checked against a fresh run of the old pipeline on all four pilots: relations, stop positions and QA identical; every feature identical (polygons may differ only in ring start or order); airports identical. Known differences: relation polygons come after the ways in the file, which changes the name `stations.py` picks for a few stations holding two differently named stop areas (GBA: 4 names, 6 English names); a route master whose routes are all non-rail (SF E-Embarcadero, `route=tram:suspended`) is no longer pulled in. All four pilots, cut to tiles: 18.7 s; all of China cuts in 0.7 s

## 2026-10-11-stations-under-construction

<a id="2026-10-11-stations-under-construction"></a>

Rule: [2026-10-11-stations-under-construction](../00-overview.md#2026-10-11-stations-under-construction). Evidence, verbatim:

There is no physical size, so the tier is inferred from what is being built: a mainline (train) station is high-speed if high-speed track under construction lies within 200 m or its name says 高铁站 / high-speed railway; it is major (tier 1) if high-speed and 4 or more pieces of mainline track under construction lie within 200 m, or its name matches the region's major-station pattern; high-speed but not major is tier 2; everything else, including metro and light rail, is tier 3. Track pieces stand in for platforms and land, line counts are never used. Nodes whose names end in one another (深圳机场东 / 机场东) within 400 m count as one station. Proposed stations (`railway=proposed`) are still dropped. Example: Xili High-speed Railway Station in Shenzhen. Run `con_stations.py` after `stations.py`
