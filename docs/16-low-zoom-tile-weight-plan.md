# Low-zoom tile weight: findings and plan (2026-10-17)

Scope: z2 to z8 tiles of `app/public/data/world/world.pmtiles` (built 2026-10-15) against the 600 KB per view budget. Replaces the open P0 in `archive/performance-todos-2026-10-17.md` and "Must fix: Europe low-zoom tile weight" in `archive/open-questions-2026-10-20.md` with a measured plan. Nothing in the pipeline was changed; `world.pmtiles` was only read. "KB" here is KiB (1,024 bytes), as in `measure_views.py`; the file sizes quoted from `pminfo` in MB (and the 1,150 MB world file) are decimal megabytes, so the two are not directly comparable.

Labels used below: **measured** = read from the real file; **encoder-exact** = the real tile was decoded, changed in Python, and re-encoded with the same `mlt-core` 0.20.4 code and settings as `tilewriter.rs` (an unchanged tile re-encodes to the identical byte count, 511,028 gzipped bytes for 3/4/2); **built** = implemented in a scratch copy of the Rust writer and run; **estimate** = reasoning or a rough proxy.

## 1. Summary

1. Cause (measured): the overview `track` layer is 91 to 99.7% of every low-zoom tile, and 86 to 90% of that is geometry. At z3 57% of all vertices are zero-length repeats (the same integer point again), 91% of segments are 2 tile units or shorter (0.25 px). `tiny()` in `tilewriter.rs` tags a way shorter than 2 units at its first zoom (z0 high-speed, z3 intercity, z5 regional) and slices it with tolerance 0.01 for its whole zoom range up to z8, so those ways keep every raw vertex, which then collapse onto the same integers. Most OSM ways are that short.
2. The overview is not merged: in 3/4/2, 21,968 features carry 20,898 distinct property sets, because `ln` (full list of line ids) and `nk` differ per way. With only type, kmh, con, prop there would be 30 sets. So `coalesce` and `join_lines` join almost nothing.
3. Fixing geometry alone (dedupe, then Douglas-Peucker 4 units after the join, then extent 2048) takes the worst laptop view from 1,583 to 671 KB and every phone view under 600. Getting the worst laptop view (Europe z5) under 600 needs one product decision (section 5, item 4). Without it nothing visually clean reaches the budget.
4. Worst views over a 1 degree grid of centres (measured): laptop z3 1,112, z4 911, z5 1,583, z6 1,254, z7 811, z8 399; phone z3 668, z4 709, z5 1,188, z6 784, z7 465. z9 to z12 worst laptop 368, 227, 144, 139: not a problem. So the work is z2 to z7 (z8 is under budget).

## 2. Baseline (measured)

Compressed tile bytes per view, laptop 1440x900 / phone 390x844, integer zoom (the worst tile count), the views named in the P0. Tool: `mv.py` (a copy of the `measure_views.py` logic reading `world.pmtiles`); scripts in `pipeline/bench/lowzoom/`.

| View (center) | z3 | z4 | z5 | z6 | z7 |
| --- | --- | --- | --- | --- | --- |
| Europe (10E 50N) | 826 / 639 | 837 / 478 | 1,583 / 1,123 | 1,206 / 783 | 647 / 229 |
| World (0, 20N) | 845 / 650 | 79 / 47 | 9 / 6 | 2 / 1 | 0 |
| Moscow | 1,096 / 662 | 748 / 256 | 540 / 202 | 130 / 91 | 83 / 69 |
| Cairo | 1,112 / 668 | 772 / 170 | 120 / 61 | 16 / 7 | 20 / 11 |
| Delhi | 1,056 / 462 | 319 / 82 | 207 / 152 | 109 / 60 | 54 / 27 |
| Mumbai | 1,056 / 462 | 285 / 77 | 156 / 82 | 113 / 72 | 43 / 30 |
| Seoul | 478 / 355 | 491 / 303 | 777 / 439 | 233 / 80 | 102 / 82 |
| Beijing | 478 / 334 | 528 / 437 | 673 / 245 | 352 / 203 | 174 / 72 |
| Shanghai | 511 / 388 | 573 / 273 | 691 / 371 | 341 / 79 | 147 / 104 |

Over budget on the laptop: Europe z3 to z7 (z3 826, z7 647 included), World z3, Moscow/Cairo z3 to z4, Delhi/Mumbai z3, Seoul/Beijing/Shanghai z5 (and Beijing, Shanghai z4 under, Moscow z5 540 under). Phone: Europe z3, z5, z6; World z3; Moscow, Cairo z3. Note my World z3 is 845 KB at (0, 20N); the P0 quotes 973 for another centre, and the true worst z3 view is 1,112 KB at (27E, 6N).

Per zoom, whole file (measured, `pminfo`): z3 25 tiles 1.32 MB, z4 1.79, z5 3.73, z6 4.72, z7 7.42, z8 12.09 MB; biggest tiles 3/4/2 511 KB, 4/8/5 427, 5/16/10 352, 5/16/11 317, 3/6/3 268, 5/17/10 265, 6/33/21 223.

## 3. Byte breakdown of the worst tiles (measured, re-encoded with ablation)

| Tile (area) | gz KB | geometry + feature structure | ln | nk | oc | other columns | features / vertices | zero-length segments |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 3/4/2 (Europe 0-45E, 41-67N) | 499 | 385 | 88 | 17 | 6 | 2 | 21,968 / 426,167 | 63% |
| 4/8/5 (0-22E, 41-56N) | 417 | 338 (+3.6 con_dash) | 55 | 13 | 5 | 1 | 15,844 / 380,786 | 56% |
| 5/16/10 (0-11E, 49-56N) | 343 | 284 | 37 | 11 | 4 | 2 | 12,603 / 263,143 | 45% |
| 5/16/11 (0-11E, 41-49N) | 310 | 260 | 27 | 10 | 5 | 2 | 9,718 / 263,161 | 47% |
| 3/6/3 (China 90-135E, 0-41N) | 261 | 255 | 2 | 3 | 0 | 1 | 2,563 / 331,452 | 57% |
| 5/17/10 (11-22E, 49-56N) | 259 | 219 | 23 | 8 | 3 | 1 | 8,192 / 198,341 | 46% |
| 6/33/21 (6-11E, 49-52N) | 218 | 187 | 20 | 5 | 2 | 2 | 6,483 / 159,122 | 33% |

Other layers in those tiles: con_dash 1 to 4 KB, stations 2 to 3 KB, complexes 1 to 2 KB, cities under 1.5 KB (z5 tiles: stations plus complexes about 1.7%). Summed over all view tiles per zoom (measured): track share z3 99.7%, z4 98.0, z5 97.2, z6 97.2, z7 95.0, z8 91.5; stations plus complexes 4% at z7, 5.5% at z8. Of the track bytes: geometry+structure 86 to 90%, `ln` 5.7 to 10% (zoom dependent), `nk` 2 to 3%, `oc` 0.5 to 1%, type, kmh, con under 0.5%. Raw bytes per vertex 2.26 to 2.30; gzipped 0.67 (z2) to 1.43 (z8).

By importance (length share, measured in tile units): Europe z3 to z4 intercity 76 to 84%, high-speed 16 to 19%; Europe z5 to z6 regional 54 to 58%, intercity 29 to 32%, high-speed 10 to 11%, metro sketch 2 to 4% (but 5 to 6% of z4 vertices: 13 features, 21,413 vertices in 4/8/5). Track with no `ln` and no `nk` is 5 to 8% of length; non-high-speed with fewer than 3 services is 34 to 58% of length. The overview has no yard or service marker (`sv` is not carried below z9; yard starts at z12, freight at z7), so "yard and minor" cannot be separated in the tiles; type, `ln` count and `nk` are the only importance signals.

Exact-duplicate features (border ways written by two countries) are 0 to 21 of 21,968 with equal properties: not a lever.

## 4. Levers, with saving (worst laptop view = Europe z5 1,583 KB unless said)

All "worst" figures are the worst over the 1 degree grid, all of z2 to z7 re-encoded (encoder-exact), z8 base for tiles not decoded (upper bound). Savings are cumulative where marked.

| Lever | Result | Status |
| --- | --- | --- |
| Drop consecutive duplicate vertices, drop lines with under 2 distinct points (overview track only, no ids there) | 1,583 to 1,163 KB; vertices z3 1,202k to 515k, z5 2,302k to 1,409k, z8 753k to 684k | encoder-exact; lossless render |
| Douglas-Peucker 4 units (of 4096) on every overview piece, after the join | to 836 KB; vertices z3 242k, z5 575k, z8 237k (2.7x fewer than base at z8) | encoder-exact, and built (below) |
| DP 2 / 6 units instead of 4 | 882 / 815 KB (Europe z5, 4-unit view list) | encoder-exact |
| Extent 4096 to 2048 at z0 to z8 (coordinates halved) | 836 to 686 KB (-18%) | encoder-exact |
| Drop stations and complexes from z0 to z5 tiles (the style first draws dots at z6) | -15 KB at Europe z5 (671 vs 686) | encoder-exact |
| `ln` cut to its first id | 836 to 712 KB (without q2); 686 to 577 with q2 | encoder-exact |
| `ln` cut to 3 / 5 ids (with q2 and stations dropped) | 627 / 653 KB (671 with the full list) | encoder-exact |
| Drop `ln` entirely (with q2 and stations dropped) | 519 KB | encoder-exact; breaks click and service list |
| Regional enters at z6 not z5 (`track_minzoom`), with DP4, q2, stations | z5 671 to 364 KB, but z6 stays 563 and is then the worst | encoder-exact |
| Thin: only non-high-speed track with 3 or more mapped services, z6 and below | z5 about 330 KB at DP4+q2; visible network loss | encoder-exact, product decision |
| Corridor proxy: snap to 4 units, union shared edges, attributes only type/kmh/con/prop, plus q2, stations | worst 302 KB (z5 284) | encoder-exact, approximate merge; kills click and hover |
| Per-tile cap with escalation (100 KB cap, rungs dp8 q2, dp12 q4) | after DP4+q2+ln1+stations only 4 of 2,634 tiles exceed 100 KB (max 125); a cap is a safety net not a lever | encoder-exact |
| MLT encoder options: morton, hilbert, id sort, shared dictionary on or off | within 0.2% on 5 tiles (shared dict 0.0%; mlt-core already picks dictionary per column) | measured |
| FastPFOR / FSST | gz +11% / +0.5%; raw -25 to -48% / -23% | measured, same as the playbook |
| Brotli 11 / zstd 19 / zlib 9 instead of gzip 6 | -14% / -8.5% / -1.5% on 6 tiles | measured; browsers cannot decode brotli or zstd without extra code in the pmtiles reader: not recommended |
| Tile clip buffer 80 to about 24 units at z0 to z8 | 2.5 to 11% of vertices lie beyond 16 units of the tile edge (3.0% at 3/4/2, 11% at 5/16/10) | estimate (vertex count only) |
| Rejected: DP 8 with extent 1024 | 519 KB worst, but lines visibly break into dots (image check at the top of the tile's zoom range) | encoder-exact, rejected |

Visual check (Python rasteriser on 5 worst tiles, lines 1 and 2 px, pixel of original ink not within 1 px of the changed render): dedupe 0.13%, DP4 0.14%, DP4+q2 0.29% at 1x; at 2x (the top of a tile's zoom range on a retina screen) DP4 0.10%, DP4+q2 0.27%, corridor 2 to 3.6%. A side-by-side render of 4/8/5 at 2x shows DP4+q2 indistinguishable from the original and DP8+extent 1024 broken. This is not a MapLibre render: no casing, antialiasing or real widths.

Built (not simulated): in a scratch copy of the writer (now merged into `pipeline/rail-extract/src/tilewriter.rs`; env switches `RE_DEDUPE`, `RE_DP`; dedupe plus DP applied after `coalesce` for the `track` layer at z <= 8) I ran `rail-extract world` on 14 countries (fr de be nl lu ch at it es pt gb ie pl cz), never the world. z0 to z8 total 10.08 MB to 5.42 MB (-46%), max tile z3 300 to 140 KB, z4 383 to 185, z5 347 to 192, z6 223 to 127; z9 to z14 byte-identical in size; run time 29.8 s against 29.9 s. For 14 tiles the Python estimate matched the built result within 0.3% (sum 1,469 vs 1,473 KB). The extent and `ln` levers are encoder-exact but not built. MapLibre's MLT adapter passes the layer's extent to each feature (`MLTVectorTileLayer`, maplibre-gl 6.14), so a non-4096 extent should work; not run in a browser.

## 5. App dependence (what is safe to drop below z9)

Read from `mapStyle.ts`, `MapView.tsx`, `App.tsx`, `Tooltip.tsx`, `data.ts`.

| Layer.property | Used by | Below z9 |
| --- | --- | --- |
| track.type | colour, width, every filter, hit sort | needed |
| track.con, prop | `!has con` filters, hit-con and hit-prop | needed |
| track.kmh (band) | speed mode colour and hidden-band filters; tooltip "Max speed" | needed |
| track.oc | operator mode colour and opacity (`coalesce(oc, typeColor)`) | needed in operator mode; 1% of bytes, keep |
| track.ln | click opens the first id (`App.tsx` `openHit`, hover highlight `hoverHighlight`, `prefetchHighlights` take the first id only); tooltip "Services" lists up to 5 names plus "+N" | first id needed; the rest only for the tooltip list |
| track.nk | railway name, length, hover and click fallback (`railwayOf`) | needed when there is no `ln` |
| track.n, ne, nl and the per-piece tags | not in overview tiles | n/a |
| con_dash.type, kmh, b, prop | dash layers | needed |
| stations (z <= 8) | circles from z6 (tier 1), z8 (tier 2), z10 (tier 3); tooltip uses n, ne, tier, hsr, con, ap, pc, lc, lct; click uses id | nothing is drawn from z0 to z5 tiles; z5 tile stations are unused |
| complexes | labels from z8 (tier 1), z9, z11 | unused in z0 to z7 tiles |
| cities (n, m, l, r) | dots z2 to z8 | needed, under 1.5 KB |

Constraint from `00-overview.md` (2026-10-16 row): the hover lists the services that run on the railway. Cutting `ln` to its first id changes that row at low zoom, so it needs the user's decision (decisions in `00-overview.md` win). `ln` truncated to 5 ids keeps the list for most tracks but loses the "+N" count and saves little (653 KB, still over budget).

## 6. Prioritised to-do list

Order is payoff over effort. Savings are on the worst laptop view (Europe z5, 1,583 KB) and cumulative in the prediction table of section 7.

1. **Remove duplicate vertices at low zoom (lossless).**
   - Do: in `encode_tile` (`tilewriter.rs`), for the `track` layer and tile zoom <= 8, after `coalesce`, dedupe consecutive equal points per line and drop lines with fewer than 2 distinct points. Only the overview has no `id`; keep the tiny pieces at z9 and up (their ids matter).
   - Saving: -420 KB (1,583 to 1,163) on Europe z5; vertices -57% at z3. Measured encoder-exact.
   - Risk: very low; zero-length segments draw nothing. Effort: under 1 hour.
   - Verify: `rail-extract pminfo` per-zoom table; pixel diff of a few tiles; `rail-extract world --deep-zoom 5` and `6` still byte-identical; z9 and up unchanged.
   - Done when: no zero-length segment in any z0 to z8 `track` tile, z9+ sizes unchanged.
2. **Simplify joined overview lines (Douglas-Peucker, 4 units of 4096, after the join).**
   - Saving: -327 KB more (to 836); built on 14 countries, -46% for z0 to z8 together with item 1. Fixes the "1.5 to 1.7x vertices at z6 to z8" note (z8 vertices 753k to 237k).
   - Risk: low; endpoints are kept so joins stay connected, shift under 0.5 css px at the bottom of a tile's zoom range. Effort: 2 hours (done in `tilewriter.rs`; the scratch `rdp` was only a starting point; use `geo::Simplify` instead).
   - Verify: pixel diff plus screenshots at z3, z4.9, z5, z6.9 over Europe, Chengdu; hover still finds track at 12 px.
   - Done when: worst laptop view at z3 to z8 at or under 836 KB and no visible break in screenshots.
3. **Extent 2048 for z0 to z8 and drop stations/complexes from z0 to z5 tiles.**
   - Saving: -150 KB (q2) and -15 KB (stations): to 671. Phone worst becomes 497, all phone views pass. Encoder-exact.
   - Risk: low to medium; coordinates snap to 0.5 css px at the top of a tile's zoom range; check MapLibre draws a layer with extent 2048 (code reading says yes). Effort: half a day (pass `EXTENT/2` and halve in `compact`, set `LayerWriter::new(name, 2048)`, make the station filter zoom-aware).
   - Verify: headless render of 4/8/5 at z4.9 against the build before; zoom bench no worse.
   - Done when: worst phone view under 600 KB at z2 to z8 and the laptop worst at or under 671 KB.
4. **Close Europe z5 and z6 on the laptop: choose one (user decision).**
   - 4a. `ln` to first id below z9: z5 671 to 562 KB (-109 KB), margin 38 KB. Cost: the tooltip lists one service (plus a "services" count if `nl` is added, +2 KB); `Tooltip.tsx` must not print "none mapped" when `ln` is truncated. Needs a change to the 2026-10-16 decision.
   - 4b. Regional track starts at z6 (`track_minzoom` regional 5 to 6, overview only): z5 drops to 364 KB, then z6 (563 KB) is the worst view. Visible: regional lines appear one zoom later. Also a decision change.
   - 4c. Both: roughly 450 KB worst (estimate: not run as a pair).
   - Verify: scan all views; screenshots; click and hover on a z5 line (first line opens).
   - Done when: all laptop and phone views under 600 KB in the grid scan, with the chosen change documented in `00-overview.md`.
5. **Per-tile cap with escalation (safety net).** After items 1 to 4 only 4 of 2,634 tiles exceed 100 KB. Add: if an overview tile is over about 100 KB gzipped, re-encode with DP 8 at extent 2048 (rung 1), then DP 12 at extent 2048 if still over; never extent 1024 (rejected, section 4); cap it once and log. Saving on the worst view about -50 to -150 KB beyond 4a. Risk: visible roughness only in those tiles. Effort: half a day. Done when: `pminfo --top` shows no z0 to z8 tile above the cap and the escalation count is logged.
6. **Corridor merge (parallel track to one line) with attribute-free drawing layer.** Estimate with a crude snap-and-union: worst view 302 KB, z5 284, but this needs `ln`, `nk`, `oc` off the drawing features, which removes hover and click below z9, so a separate hit layer would be needed. A hit layer (DP 8, extent 1024, `ln` first id plus `nk`) measured 3.35 MB against 4.41 MB for the draw layer alone over the 400 view tiles, which eats most of the gain. Effort: days; risk high (hover, highlights, `dedupe` of draw and hit). Do it only if 1 to 5 are not enough. Pixel diff 2 to 3.6% of ink at 2x.
7. **Minor, optional**: clip buffer 80 to 24 units at z <= 8 (estimate 2.5 to 11% of vertices, line caps need about 20 units); drop `modes`, `ty`, `mp`, `cx`, `term`, `lg` from z6 to z8 station tiles (under 1% of the view); nothing for MLT encoder options or brotli (section 4).

## 7. Recommended sequence and prediction

Worst view over the grid of centres, laptop / phone, KB (encoder-exact for z2 to z7, z8 is base 399 / 218 as an upper bound). "Fails" lists zooms over 600 KB.

| After step | Laptop worst (zoom) | Fails, laptop | Phone worst | Fails, phone |
| --- | --- | --- | --- | --- |
| Today | 1,583 (z5) | z3, z4, z5, z6, z7 | 1,188 | z3, z4, z5, z6 |
| 1 dedupe | 1,163 | z3, z4, z5, z6, z7 | 863 | z5, z6 |
| 2 + DP4 | 836 | z5, z6 | 623 | z5 |
| 3 + extent 2048 + stations | 671 | z5 | 497 | none |
| 4a + `ln` first id | 562 | none | 412 | none |
| 4b instead (regional z6) | 563 (z6) | none | 356 | none |
| 5 cap, if added | below 562 | none | below 412 | none |

By zoom after step 4a (laptop): z2 42, z3 293, z4 275, z5 562, z6 477, z7 340, z8 322 (base). The named views: Europe z5 laptop 1,583 to about 560; World z3 845 to about 290; Moscow, Cairo, Delhi, Mumbai z3 about 300; Seoul, Beijing, Shanghai z5 about 250.

## 8. Limits of this study

- Steady-state tile bytes per view only. MapLibre also loads parent and neighbour tiles while zooming, so a gesture can fetch more than any single view figure.
- Views are integer zooms at 1 degree grid centres, laptop 1440x900 and phone 390x844, as in `measure_views.py`; z8 savings are not computed (worst 399 KB is already under budget).
- When this study was written, the levers extent, `ln`, regional, thin, corridor and cap were simulated only. Since section 9, dedupe, DP4, extent 2048 and the station-attribute drop are built in `tilewriter.rs`; `ln`, regional, thin, corridor and cap are still not built. DP was applied to already simplified geometry; the Rust build matched the simulation within 0.3% on the 14-tile check.
- Visual claims come from my rasteriser, not MapLibre screenshots. Zoom bench, hover and click were not run. Cold-load time was not measured.
- Importance thinning can use only type, `ln` count and `nk`; there is no tag for minor or service track in the overview.
- Scratch tools are in `pipeline/bench/lowzoom/` (`mv.py`, `exp.py`, `ladder.py`, `ladder_scan.py`). The scratch writer patch is not kept: its code is now in `pipeline/rail-extract/src/tilewriter.rs`. `reenc/` and the scratch crate are not kept.

## 9. Steps 1 to 3 built and measured (2026-10-17)

Approved by the user: steps 1 to 3 of section 6 (lossless-ish). Steps 4 and later are not approved and not built: `ln`, `nk`, `oc` and every other column are exactly as before. Code: `pipeline/rail-extract/src/tilewriter.rs` (`encode_tile`, `low_zoom_layer`, `douglas_peucker`, `dedupe_lines`, `halve`; constants `LOW_ZOOM` 8, `LOW_EXTENT` 2048, `DP_TOLERANCE` 4.0, `NO_STATIONS_ZOOM` 5). Measured on subsets of the real tile input, never the world: 14 countries (fr de be nl lu ch at it es pt gb ie pl cz, `--deep-zoom 6`) and East Asia (cn kr jp tw). Outputs in the scratchpad, own `CARGO_TARGET_DIR`, `app/public` and `data/world` untouched. A build with step 0 (the new code switched off) is byte-identical to the build before the change (309,793,980 bytes, `cmp`).

What each step does, as built:
1. **Dedupe**: for the `track` layer at z <= 8, after `coalesce` (which joins), repeated consecutive vertices are dropped and lines with under 2 distinct points are dropped with their piece. Overview track has no per-way id (`track.geojsonl` starts at z9), so nothing with an id is lost.
2. **Douglas-Peucker, 4 units of 4096**, on every overview line after the join and the dedupe. Own implementation (`douglas_peucker`), not `geo::Simplify`: it measures the distance to the segment (geo measures to the infinite line, which flattens an out-and-back spur) and keeps both ends, so joins stay connected.
3. **Extent 2048 at z0 to z8** for every layer without polygons (track, con_dash, cities, stations, complexes, airport_pts): coordinates halved with rounding, then repeated vertices dropped again (halving creates new ones), so no zero-length segment is left. **Deviation:** `airports` (polygons, z8 only) keeps extent 4096, because halving collapses small rings; MapLibre reads the extent per layer. **Stations and complexes are not written to z0 to z5 tiles**: the style draws the first station dot at z6 (`STATION_ZOOM`, tier 1) and the first complex label at z8, and `queryRenderedFeatures` for hover and click only asks the style's layers (which start at z6), so nothing reads them below z6. Only z5 was affected (tier 1 starts at z5 in `tiles.rs`). Complexes were dropped too (the plan said "stations and complexes"; the request said stations). A tile left with no layer is not stored.

### Whole-file z0 to z8 on the 14-country subset (measured, compressed tile bytes)

| z | base MB / max KB | 1 dedupe | 2 + DP4 | 3 + extent 2048 + stations |
| --- | --- | --- | --- | --- |
| 3 | 0.41 / 293 | 0.24 / 173 | 0.19 / 136 | 0.13 / 97 |
| 4 | 0.53 / 374 | 0.35 / 244 | 0.25 / 181 | 0.19 / 136 |
| 5 | 1.37 / 339 | 1.00 / 251 | 0.72 / 188 | 0.56 / 142 |
| 6 | 1.68 / 218 | 1.34 / 175 | 0.90 / 124 | 0.76 / 102 |
| 7 | 2.36 / 95 | 2.04 / 83 | 1.29 / 54 | 1.13 / 47 |
| 8 | 3.57 / 65 | 3.29 / 61 | 2.01 / 40 | 1.81 / 36 |
| z0 to z8 total | 10.09 MB | 8.36 (-17%) | 5.43 (-46%) | 4.64 (-54%) |

z9 to z14: all 247,581 tiles byte-identical to the base build (`pminfo --tsv` content hashes), the same tile count. z0 to z8: 586 tiles to 584 (two corner tiles that held only a sliver of buffer track are gone). Run time 52.0 s base, 51.4 s with all three steps (same machine, the runs were not alone on the CPU).

### Views (laptop 1440x900 / phone 390x844, KB, `WORLD=<file> python3 pipeline/measure_views.py`, a new mode; the table below is the same measure computed from the `pminfo --tsv` sizes because the Python reader took over an hour on the 1 degree grid)

14-country subset, worst over a 1 degree grid of centres from -12..25E, 35..62N (the plan's world figures in brackets):

| After step | Europe z5 laptop (plan) | Worst laptop (zoom) | Worst phone (zoom) |
| --- | --- | --- | --- |
| base | 1,264 (1,583) | 1,320 (z5) | 1,051 (z5) |
| 1 dedupe | 918 (1,163) | 960 (z5) | 759 (z5) |
| 2 + DP4 | 661 (836) | 690 (z5) | 550 (z5) |
| 3 + extent + stations | 515 (671) | 538 (z5), 532 (z6) | 429 (z5) |

Ratios against the plan: step 1 x0.73 (plan 0.73), step 2 x0.72 (plan 0.72), step 3 x0.78 (plan 0.80). The subset has fewer countries than the world, so its absolutes are lower; the reductions match. Applying the measured ratios to the plan's world numbers gives about 1,163, 836 and 650 for Europe z5 (the plan said 671). The "worst phone under 600 KB" and "laptop worst at or under 671" done-when of step 3 both hold on the subset; the full-world scan still has to be run after the real world build. East Asia (cn kr jp tw): worst laptop 752 (z5) to 246, worst phone 497 to 162; Seoul z5 752 to 246, Beijing 639 to 211, Shanghai 681 to 221.

### Checks

- `rail-extract world` on the 14 countries: `--deep-zoom 5` and `--deep-zoom 6` give byte-identical files (`cmp`), and equal the three-step experiment build.
- No zero-length segment in any of the 584 z0 to z8 tiles of the subset (decoded with `@maplibre/mlt`, 985,179 segments, 0 repeated points); layer extents 2048 everywhere except `airports` 4096; no `stations` or `complexes` layer in z0 to z5 tiles (z6: 28, z7: 97 / 64, z8: 248 / 248 tiles).
- `cargo test` for the writer: 3 passed.
- Render (headless Chrome, own profile, the real app from a Vite dev server on port 5199 reading the subset PMTiles from a local range server; screenshots before and after, looked at): Europe z5.0, z4.9, z3.2, z6.9 and z7.9 at device pixel ratio 2, China z6.0, z5.9, z4.2 and z7.9 at ratio 2: no visible line breakage, dots or gaps; the before and after images are indistinguishable at normal size. No console errors.
- Hover and click: with the app's own hit layers, `queryRenderedFeatures` over a 12 px box on a 40 px grid (600 boxes) finds track at 413 / 305 / 67 / 389 / 219 boxes before and 412 / 303 / 67 / 390 / 221 after (Europe z5, 4.9, 3.2, 6.9, 7.9); China 200 / 353 / 133 / 273 before, 200 / 352 / 135 / 274 after. `ln` and `nk` are present on the hit features as before (Europe z5: 160 / 251 boxes carry `ln` / `nk` before, 172 / 267 after: joins now give longer pieces; the first sample features are identical).
- MapLibre handles the per-layer extent 2048 (tracks, dashes, cities, stations at z6 to z8 all drawn and hit at the right place).

Left for the user (steps 4 and later, section 6): `ln` to its first id (Europe z5 about -109 KB), regional track at z6, the per-tile cap. Not run: the world build, the zoom bench, a MapLibre run on the full world file.

### Off switch, tests, metadata (2026-10-17, after review)

- **Switch:** `RAIL_LOWZOOM=off rail-extract world ...` (or `build`/`tiles`: any run of the writer) restores the writer as it was before section 9; unset or any other value is the new behaviour. Read once per process (`lowzoom_on` in `tilewriter.rs`); the constants `LOW_ZOOM`, `LOW_EXTENT`, `DP_TOLERANCE`, `NO_STATIONS_ZOOM` stay constants.
- **Reproduced on a snapshot** (the live `data/world` tile input changed while I worked, so both runs read a copy of the 14 countries' `tiles_tmp`): old binary vs `RAIL_LOWZOOM=off` `cmp` identical; with the switch on, all 247,591 z9+ tiles have the same content hashes as the old binary; z0 to z8 10.07 to 4.62 MB; `--deep-zoom 5` vs `6` `cmp` identical.
- **Metadata:** `vector_layers` now gives `stations` and `complexes` minzoom 6 (what the tiles hold; was 5), `layer_minzoom` in `tilewriter.rs`. Nothing in `app/src` or `pipeline` reads `vector_layers` or the metadata minzoom (grep). `app/src/config.ts` line 3-4 comment is stale: it says tiles hold tier 1 stations and names "from z5"; tier 1 stations and complexes are now in tiles from z6.
- **Unit tests** (`cargo test tilewriter`, 11 pass): `dp_keeps_ends_and_drops_collinear_run`, `dp_closed_ring_keeps_far_vertices`, `dp_out_and_back_spur_is_not_flattened`, `dedupe_drops_repeats_and_degenerate_lines`, `halve_rounds_up_including_negative_buffer_coordinates`, `low_zoom_track_layer_removes_collapsing_lines_and_halves`, `low_zoom_polygon_layer_keeps_extent_4096`, `tile_with_only_stations_below_z6_is_empty_and_off_switch_keeps_it`.
