# Open questions

As of 2026-10-11. Everything else is decided and listed in `00-overview.md`.

## Still open

| Needed by | Question | Why it matters | Suggested default |
| --- | --- | --- | --- |
| Before the China-wide build | China high-speed rule: does the Chengdu rule (see overview) hold elsewhere? Also, should short connector lines default to Intercity? | Tuned on one region only | Check on Beijing–Tianjin or the Pearl River Delta; a line-length cut-off needs measuring before use |

Parked: a flag to apply the non-China high-speed rule (`highspeed=yes` or 200 km/h and above) in China. For now, 200 km/h lines are promoted one by one with `high_speed_track_overrides` in the region config.

The Chengdu rule itself is decided (see `00-overview.md`).

If something comes up that the docs don't cover, add it here and ask; don't decide silently.

## Provisional choices made in phase 2 (please confirm or change)

The docs did not cover these, so they were implemented as the obvious option and are listed here instead of in the overview.

| Topic | What the code does |
| --- | --- |
| French service values | `national` and `international` → High-speed if the network is on the region's high-speed list (TGV, Eurostar, Ouigo), else Intercity. `night` → Intercity. `tourism` → Other |
| (Superseded 2026-10-12: Commuter merged into Regional) Commuter list vs `service` | A network on the commuter list (RER, Transilien, Caltrain) beats `service=regional` or no service, so Transilien lines are Commuter. It does not beat `high_speed` or `long_distance` |
| Train route with no service and no list match | Regional (`type_source=default`). Counted in the classify report |
| Track with no route and no usage | Regional (default). `usage=main` with no route → Intercity, as the spec says, but in the SF Bay Area that colours freight-only main lines as Intercity (742 ways) |
| Mapped stop-area groups | Trusted as written. In OSM, Magenta is inside the Gare du Nord group, so it shares that complex. Inferred grouping (same name or wikidata, different modes, within 300 m, or same building) fills the gaps |
| Railway land in the size score | Only land within 500 m of the station counts (`landuse=railway` polygons span whole corridors, up to 224 ha). Weight 3 per sqrt(ha) |
| Terminus flag | At least two mainline track dead ends within 120 m of the station's own mainline platforms. Adds 10 to the score |
| Size score and tiers | 3 per mainline platform, 0.5 per mainline track way at the platforms (capped at 40), 3·√land ha, 4·√building ha, +10 terminus. Tier 1 at 60 or more, tier 2 at 25 or more, else tier 3 |
| Tier names vs the design | The design has three station symbols (Major HSR, Major, Minor) and no Terminus badge. The data has tiers 1–3 plus a terminus flag; the UI mapping is still to decide |
| Stations with no mapped platforms | Rank low. Chengdu's main station (成都) has no platforms in OSM and lands in tier 3 |
| Unrouted main line in the US | A per-region setting (`unrouted_main_type` in `pipeline/config/`). The SF Bay Area sets it to Freight, so unrouted `usage=main` track is muted grey instead of Intercity. Paris and Chengdu keep Intercity |
| Track on shared line (phase 3) | Track takes the most prominent type of any line on it, so a classic line used by a few TGVs (e.g. Paris–Strasbourg east of Noisy-le-Sec) is drawn High-speed |
| Operator strands (phase 3) | One strand per line per piece of track from z12. Above 7 lines on one piece, strand width and spacing shrink so the bundle stays at about 7 strands wide |
| Tile layout (phase 3) | Decided 2026-10-13 (see `00-overview.md`): the pilot is one joined tile source, `pilot.pmtiles`, built from per-region files. The world layout is decided and deployed (2026-10-16): one `world.pmtiles` from `rail-extract world`, the only source the app reads (`10-world-build.md`). Tier 1/2/3 stations appear in tiles from z7/z9/z11 and on screen (circles) from z8/z10/z12; labels come from the complexes layer from z10/z11/z13 |
| Design extras left out (phase 3) | Compare view (switch shown, disabled until phase 4) and the Terminus badge in tooltips (shown only in the station panel, per the design). Units (km/mi), Copy link and Inspect OSM tags are in |
| Inspect OSM tags | Shows the tags the data spec keeps (not the full OSM tag set, which the filter drops) plus fields Rail Globe derives (rail type and how it was set, routes, stop area, station model fields). Tags load on demand from 256 id-sharded files per region (`data/<slug>/tags/`), so they are not in the tiles. Hover shows tags; click pins a panel with Open in OSM and Copy tags |
| Airport icons | The plane icon and IATA code sit at the centre of the airport outline (commercial airports only: IATA code, not military, 300 ha or more; smaller airfields get outline and code, no icon). Stations at airports are ordinary circles; their tooltip and panel still say Airport. The track tooltip no longer shows a Tracks row (the value was unknown for most track) |
| Yard and area tooltips | Yard track shows its name, service type, electrification and gauge. Station grounds, yards, station buildings and platforms show kind, name, area and operator. "Station grounds" areas are whole `landuse=railway` polygons, which can span a corridor (one in Paris is 224 ha), so the area figure is not a station's size |
| Major station rules: what the prototype had | The pasted description (MAIN_RE, MAIN_BY_GROUP / MAIN_SET, hs_idx / hs_major) is not in this repo, so the rules were rebuilt from the description. Chengdu uses the name pattern only (no canonical list); the China city list in `config/chengdu.json` is short (成都, 天府, 重庆, 西安, 北京, 上海, 广州, 深圳) and needs extending for other cities |
| Major station: high-speed termini | The prototype test (terminus of a high-speed route) reads route relations, which conflicts with "rank from physical size only". Implemented instead as: mainline station within 80 m of high-speed track and (mainline terminus or 8+ mainline platforms). This also covers China, where high-speed routes are thinly mapped. Confirm this is acceptable |
| Construction zoom (changed 2026-10-11) | Under-construction track appears from the zoom of its rail type, so a high-speed line under construction shows at every zoom. This replaces the earlier z10 start in `02-architecture.md` |
| Named railway lines | Where no route relation covers a piece of track (Chengdu has almost no train routes), the track's own `name` is used: as the tooltip title, as a map label from z11.5, and as an entry in the Lines list (grouped by rail type, with approximate length from the track pieces divided by `tracks`). Only names with about 1.5 km or more of track are listed |
| Tooltip tier names | Tier 1 with a high-speed track nearby → "Major HSR station"; other tier 1 and tier 2 → "Major station"; tier 3 → "Minor station" |

## Highlight performance: what was tried, what is left

Measured 2026-10-13 in headless Chrome (Apple M4 Max, 2x pixel ratio, CPU throttled 6x, dense z14/z15 views). After the fixes in `00-overview.md` the hover highlight starts in 2-3 ms; what remains is MapLibre's first frame after a highlight, about 7 ms (40-50 ms throttled). Four experiments run in parallel, none kept:

- **Pixel ratio 1 and no antialiasing:** first frame unchanged (42-49 ms against 41-52 ms). The frame is CPU-bound (style recalculation, source update), not fill-rate bound. Both cost visible sharpness. A 1.5 cap was not measured cleanly.
- **Hit test and React cost:** a hit test takes about 0.1 ms throttled, so the pointer path is not the bottleneck. Skipping area-polygon queries below z12 would stop station grounds, buildings and platforms being hoverable there; a 3 px skip can leave a stale hover when tiles load under a still pointer. Not worth it.
- **Highlight data without a fetch:** a slim `hl/<id>.json` saves 1% (the 3,680-way line is 94.5 KB against 95.1 KB; `ways` and `geom` dominate). A bundle of the 60 longest lines is 619 KB raw, 128 KB gzipped, loaded once per region, and saves one round trip on a cold hover at best. Gzip shrinks the files about 4x, so check the host compresses JSON (vite preview does not). Not worth the extra files yet.
- **Highlight from GeoJSON at every zoom, no feature state:** no faster (45-54 ms at z15, 26-30 ms at z14); feature state itself is only 1-4 ms. The side-file geometry is simplified to about 100 m, so it was not checked for fit at close zoom, and a line mixing high-speed and other track would lose the per-piece ordering.

Not tried, in the order I would try them:

- Fewer or merged layers (155 layers, 3 regions): the frame cost is mostly per-layer work, and symbol placement runs again after state changes. Lazily add label layers per zoom
- `crossSourceCollisions: false`, `renderWorldCopies: false`, `maxTileCacheSize`
- Set feature state only for the ways in view (needs a bounding box per way in the side file); only matters for the 3,680-way Shanghai-Lhasa train
- One deck.gl `PathLayer` (via `MapLibreOverlay`, interleaved) for only the hovered line. No published highlight benchmark found; needs exact geometry at close zoom. Moving tile rendering to deck's `MVTLayer` was judged a bad fit (own tile loader, loses the MapLibre styling)
- Delivery on a real host: Brotli or gzip on the side JSON, long immutable cache headers, a service worker for hot tiles and line files
- MapLibre WebGPU or off-main-thread rendering: not shipped, only on the roadmap
- Test on the machine where 500 ms was seen: the headless runs never reproduced it, and the fixes above came from reading MapLibre and profiling clicks

## Faster HSR only toggle: research areas (user, 2026-10-13; nothing done yet)

Toggling "HSR only" is slow, most at low zoom (z0 to 8) where the merged overview holds the most pieces per tile. Working theory, not yet measured: the switch changes data-driven paint (colour, width and opacity through a per-feature `case` on the type, in `applyState`), and MapLibre re-processes every loaded tile for that, so the cost grows with the pieces in view. Filter changes do the same (see the Construction switch, fixed 2026-10-13 by splitting layers so it only flips visibility). Areas to research, in the order to try:

1. **Constants, not per-feature expressions.** Track already has a high-speed twin layer (`-hs`; the base layer excludes high-speed). With HSR only on, give the base layer constant grey paint (or just toggle its visibility and draw a grey copy) and leave the twin alone. No `case` on the type, so no tile re-processing.
2. **No filter changes on a toggle.** Every switch that can be a layer's visibility should be one (the same split as construction / proposed). List the switches that still change filters: hidden types, speed bands, freight, HSR only.
3. **High-speed in its own tile layer.** Greying then becomes a layer property, not an expression. Costs tile size; test on China at z5.
4. **Feature state.** Flag the pieces with `feature-state` instead of re-evaluating paint: no tile reload, but it is per feature, so it may be slow with 125 k pieces.
5. **Smaller low-zoom tiles.** Drop the attributes the toggle does not need from the overview (`kmh`, `oc`, `ln` are only needed for some modes), or lower the overview's piece count further.
6. **Skip work.** Turn off paint transitions for this switch; pass `{ validate: false }` to `setPaintProperty` / `setFilter`; apply all changes of one click in one frame.
7. **Perceived speed.** Show the change at once with a light CSS or canvas veil while the map catches up.
8. **Measure first.** Use `app/bench/zoom-pan.mjs --trace` or a CDP trace on a toggle at z4 to z6 (China, Germany): how much is worker re-parse, how much main-thread paint, how much GPU. Put the result here before choosing.

Success test: the toggle at z5 in China and Germany finishes in under 100 ms on the 6x throttled bench.

## Smooth rendering and zoom: to do

Listed 2026-10-13, updated the same day after the fast-zoom profile. Numbers: `reference/research-summary.md` ("Fast zoom: profile and comparison"). How to measure: `app/bench/README.md`. The profile shows warm zooming is limited by the main thread (symbol placement, painting), not by tiles.

Done or dropped:

1. **One tile source instead of three.** Done (`00-overview.md`). Missed frames 128 to 76 at 6x.
2. **Drop the high-speed twin layers.** Done 2026-10-13 (`00-overview.md`). Twins are kept only for track and strands; construction and the highlight use a sort key. 66 to 62 layers; speed gain within noise, kept as a simplification.
3. **Lighter tiles (tooltip-only tags to side files).** Tried and dropped: 8.5% smaller `china.pmtiles`, no frame gain. A cheaper variant is to pack those tags into one property.

Open, by measured payoff:

4. **Trim basemap layers.** The biggest measured cost: hiding the basemap took 18 missed frames to 6.5.
5. **`fadeDuration` 150 instead of 0.** Measured gain (18 to 14; placement 24% to 8% of busy time). It changes how labels look: they fade in instead of popping. **Needs the user's decision.**
6. **Hide the `hl-fs` layers when nothing is highlighted.** Small gain (18 to 15.5), free.
7. **Align tile `minzoom` with style `minzoom`.** Five layers carry data a zoom before they are drawn (list in the research summary). Free bytes; not yet measured.
8. **Stronger low-zoom simplification (`--low-detail`).** Not measured.
9. **Corridor merging prototype at z8-11.** Not measured.
10. **Stop tiles at z13 and overzoom.** The reference stops at z12. Not measured. The user is fine with overzooming (2026-10-13), so this is open to benchmark with the other tile options.
11. **MapLibre Tiles (MLT). DONE 2026-10-14.** MapLibre 6 reads MLT (the source needs `encoding: 'mlt'`); tippecanoe does not write it. Done in the Rust writer (`tilewriter.rs`, `rail-extract tiles` alone, called by `build` and `--combined`; cut over and tippecanoe and `tile-join` removed 2026-10-14; `mlt-core` 0.20.4, 2026-10-14). Measured facts (same `tiles_tmp` input, same flags for the tippecanoe reference, this laptop, 14 cores, 36 GB):

    | | tippecanoe (MVT, removed) | Rust writer (MLT) |
    | --- | --- | --- |
    | China tile build, wall | 36.8 s now (28.5 s earlier today), 0.37 GB peak | 4.9 s (single run; 4.1 s median of 3 in the final orig-vs-mlt benchmark), 5.3 GB peak RSS |
    | Pilot combined (sf-bay-area china de fr ch it), wall | not re-run | 12.9 s, 9.3 GB peak RSS (writer only) |
    | Americas combined (27 regions), wall | 276 s (earlier record) | 7.3 s, 6.3 GB peak RSS |
    | Paris / SF Bay Area, wall | 3.9 s / 0.8 s | 0.4 s / 0.1 s |
    | File size: China | 148.3 MB | 133.3 MB (-10%) |
    | File size: pilot (6 regions) | 343.7 MB | 310.5 MB (-10%; the FastPFOR/FSST build of 2026-10-14, now 298.8 MB) |
    | File size: Americas | 222.0 MB | 207.9 MB (-6%) |
    | File size: SF Bay Area / Paris | 1.84 / 7.4 MB | 1.65 / 6.5 MB |

    Fidelity against the tippecanoe tiles of the same input (all tiles of SF Bay Area and Paris; every tile of China): every OSM id tippecanoe has at z9 to z14 is also in the MLT file at that zoom (0 missing; MLT has 125 / 851 / 1,659 extra tiny ones in SF / Paris / China that tippecanoe drops); feature ids were compared in every tile of all three; per-tile property key sets and the property values of every feature id at z10 and up were compared for all tiles of SF Bay Area and Paris but only a 6.3k-tile sample of China, and are equal there; line length was not compared; Overview (z3 to z8) feature counts differ because tippecanoe joins touching lines and keeps more separate features; MLT merges consecutive equal-property pieces and joins end-to-start lines.

    Where the China time goes (after the fixes below): read and parse 0.9 s, slicing 0.9 s, gather 0.2 s, encode plus gzip 2.0 s (parallel), PMTiles write 0.4 s. The first version took 29 s, all of it in the slicing step: China's one 245k-feature track group ran on a single thread (21.6 s), and the pmtiles writer gzipped every tile one after the other (4 s). Fixed by cutting big groups into runs of 8,000 features, gzip in the encode threads, shared (Arc) properties per source feature, a compact i16 geometry for pieces, and slicing one region at a time (writer-only peak for the six pilot sources in one writer run, fell from 33 GB to 9.3 GB; the full `build` of seven regions peaks at 14.1 GB because the parallel station models run too). Memory is 15 to 30 times tippecanoe's: the input GeoJSON is parsed whole (about 9 bytes in memory per byte of text). A larger combined build than Americas needs slicing per top-level tile or a streaming reader.

    View weights (`measure_views.py` logic on the MLT files; compressed bytes, 600 KB budget): worst laptop view 557 KB (Chengdu East z5) against 641 KB with MVT; worst phone view 451 KB (Germany z6) against 524 KB. Per zoom, the worst view is lower with MLT at every zoom (z4 450 vs 481, z5 557 vs 641, z6 451 vs 524, z8 399 vs 449, z9.5 329 vs 481, z10.6 216 vs 314, z12 310 vs 393, z13 193 vs 238, z14 128 vs 146 KB); single views can be a little higher (France z6 321 vs 288 KB, France z8 147 vs 119 KB). With every region in one file (`TILES=pilot`): worst laptop 923 KB (Lyon z6) against 994 KB, worst phone 794 KB (Zurich z5) against 883 KB. So the Europe low-zoom problem below is not solved by MLT.

    NOT yet measured: the warm frame bench, cold-cache load and throttled-network load have not been run on MLT (they need the user's headed 120 Hz display). The earlier MVT baseline files were overwritten by the MLT build; an MVT baseline can be rebuilt from git HEAD in a worktree with tippecanoe installed.
12. **Self-host glyphs**: done 2026-10-15 (Latin and CJK), see "Self-hosted subset glyphs" below.
13. **Production hosting.** `r2.dev` has no Cloudflare cache and is rate-limited. Use a custom domain with cache, or chunked tiles read by HTTP Range like the reference. Status 2026-10-17: the world build is live on `r2.dev` (R2) with the app on Vercel; the custom domain is still open (live checks and what is missing: `11-performance-todos.md`, R2 item).
14. **Pre-rendered raster tiles for z0-5.** Also fixes the z5 China views over the 600 KB budget.
15. **Move the tooltip with a CSS transform** instead of `top`/`left`.
16. **Per-layer `minzoom`/`maxzoom`** on every layer so off-zoom layers are skipped early. Not tried.

Tile options the user wants benchmarked (items 7, 8, 11, plus 9 and the drop-ids idea): 1 align zooms, 2 simplification, 3 drop ids at z8-9 (the user is hesitant: it may affect presentation), 4 corridor merge, 7 MLT. Measure them on cold-cache and throttled-network scenarios and with trackpad pinch input, since the profile shows tiles are not the limit for warm zoom. The bench does not cover those two scenarios yet.

Tried, no measurable gain (2026-10-13): `maxTileCacheSize` 200/500 (MapLibre 6.12 can only lower it; `maxTileCacheZoomLevels` is the raising knob), `cancelPendingTileRequestsWhileZooming` off plus `maxTileCacheZoomLevels` 8 (18 to 14.5, small), `renderWorldCopies: false`. `crossSourceCollisions: false` was tried and not kept: it made basemap labels overlap station labels.

## Must fix: performance dropped after the themes, labels and Europe (user, 2026-10-14)

The user reports the map feels slower after the dark / light themes, the rail-only place labels and the Europe countries (France, Switzerland, Italy, Germany) were added. The bench did not show it (fast zoom, 1 headless run, 6x throttle, loaded machine: 14 missed frames with the place labels, 12 without, inside the noise), so it is probably real-device cost the headless bench misses, or the Europe tile weight (see the section above), not the labels alone. To do: measure headed on the user's display (`--headed`, 120 Hz), A/B each change on its own (themes, place labels, extra landcover / landuse base layers, the `within` label filter, Europe tiles), and fix what shows. Until then, expect lower smoothness than the last commit before `c4ddee1`.

## Perf: why MLT was slower than MVT, the fix, and the levers tried (2026-10-15)

User-facing behaviour is unchanged: the same highlight (hover, selected line), the same hover cards and clicks; screenshots at three views are pixel-identical to the build before (0 pixels differ by more than 40/255).

**Root cause (measured).** MapLibre 6.12 and 6.14 call `Tile.setFeatureState` -> `FeatureIndex.loadVTLayers()` on the main thread for every loaded tile as soon as the source has any feature-state entry. For MLT that decodes the whole tile eagerly (all layers, all features, all columns; about 0.66 ms per tile at z9-11, about 10x MVT's lazy parse, x4 under the CPU throttle). The line highlight used `setFeatureState` on `track` (`promoteId: { track: 'id' }`), and the bench pointer sits on the map, so every view after the first paid it. With feature state disabled MLT equalled MVT.

**Fix (kept).** No feature state anywhere. `pilot:hl-fs` is now a filter layer: `['in', ['get', 'id'], ['literal', ways]]` set with `setFilter` on hover (cleared to a match-nothing filter), `promoteId` removed (feature ids were not used by clicks; they read `properties.id`). The GeoJSON overlay `pilot:hl` below z9 is unchanged. Checked in headless Chrome: `tileManagers.pilot._state.state` is `{}` after hovering, three views, highlight shows and clears, hover card and click panel the same as before. A queryRenderedFeatures call still decodes the tile it touches (cached per tile), so hover and the idle `prefetchHighlights` still decode tiles on the main thread, but only on query, not on every tile load. The cost moved to `setFilter`: MapLibre re-lays out every loaded tile of the source on a filter change (same as the other toggles, `00-overview.md`), a hover change costs a worker re-parse, off the main thread. Not measured separately: a hover-while-zooming stress test.

Quick bench (2 views, headed 120 Hz, throttle 4, delta 127, ABAB, total missed frames of Chengdu East + Shenzhen; one number per run):

| Lever | Before | After | Verdict |
| --- | --- | --- | --- |
| 1. No feature state (MapLibre 6.12) | 38.5, 40.5 | 23.5, 26.5 | kept, -37% |
| 2. maplibre-gl 6.12 -> 6.14.0 (same step 1 code) | 27, 29 | 26, 25 | kept (asked for); within noise; MLT decode path unchanged between the two (`MLTVectorTile` identical) |
| 3a. `fadeDuration` 150 (now 100) | 27.3 | 28.7 | rejected (no gain; also changes label fade) |
| 3c. `maxTileCacheZoomLevels` 2 / 10 (default 5) | 26.8 | 29.8 / 25.8 | rejected (2 worse, 10 within noise) |
| 3c. `cancelPendingTileRequestsWhileZooming: false` | 26.8 | 26.3 | rejected |
| 3c. `refreshExpiredTiles: false` | 26.8 | 25.0 | rejected (-7%, below the 15% bar) |
| 3c. `validateStyle: false` | 26.8 | 26.0 | rejected |
| 3c. `antialias: false` | | | n/a: MapLibre 6 already defaults to no antialiasing and the app does not set it; `prefetchZoomDelta` does not exist in 6.14; `trackResize` not tested (no resize in the bench) |
| 3e. pmtiles Protocol | | | nothing to fix: created once, headers and directories are cached (SharedPromiseCache, 100 entries); in a 20-jump session 9 leaf-directory reads, all unique, plus one range per tile (tile ranges repeat only when a tile was evicted from MapLibre's cache and is revisited) |

**The first-zoom hitch (3b) is the CJK glyph path (3d), not shaders or a tile.** A 90 ms frame about 375 ms into the first zoom-in of each page is worth about 10 of the ~25 missed frames in Chengdu East. Warming first (eval: jump to Beijing z12 or z13 hidden, wait for idle, jump back) cuts the quick-bench total 29.3 -> 15.6 and Chengdu East 18.3 -> 8, max frame 91 -> 40 ms (4 runs each); warming at the app's own start view (z12, Greater Bay Area) only 28.7 -> 24.7, warming at a feature of the initial view at z13 28 -> 24.5, z14.5 at Beijing 24. That sensitivity to what is drawn, not where, matches MapLibre drawing each CJK character on a canvas on the main thread the first time (`_drawGlyph`, 35% of the bad frames in the earlier trace). Confirmed directly: `localIdeographFontFamily: false` (CJK glyphs from the OpenFreeMap font server instead) takes the quick bench from 27.2 to 2.8 missed frames, Chengdu East 18 -> 1.5, max frame 91 -> 17 ms (3 runs each, and at 50 ms latency 18 -> 1.8). A different local font (`sans-serif`) changes nothing. Not applied, because it costs bytes and changes the CJK font: the three views Chengdu z12, Chengdu z10, Beijing z11.5 pulled 97 glyph range files, 13.0 MB (cache disabled), against 0 now; the Noto Sans glyphs look nearly the same as the local font (screenshots, kept locally only; a few labels wrap or place differently). This breaks the 600 KB view budget on first load. **Decision for the user, and the recommended next step:** self-host subset glyph files (item 12 above, now for CJK too): the tile data has only about 2,800 distinct CJK characters (station, track and place names of the pilot sources), so a subset of the Noto Sans glyph ranges is a few MB in total and a few hundred KB per view; then set `localIdeographFontFamily: false` with `glyphs` pointing at them. A hidden pre-render warm-up was not shipped: it only helps if it draws the same characters, so it would be a hack with a location-dependent gain.

**MLT-specific levers (3f), measured in the real app** (full pilot.pmtiles rebuilt into a scratch folder per variant with the Rust writer; `app/public/data` untouched; quick bench ABAB, 2 runs each; the file currently in `app/public/data` is the older FastPFOR+FSST build, 310.5 MB):

| Variant | pilot.pmtiles | Missed (2 runs) | Verdict |
| --- | --- | --- | --- |
| FastPFOR + FSST on (file in public/data) | 310.5 MB | 27, 25.5 | baseline |
| Writer default now (both off, shared dict on) | 298.8 MB | 25, 25.5 | no speed change; 3.8% smaller, promote with the next rebuild |
| + Morton spatial sort | 297.0 MB | 27, 25.5 | no gain |
| + Hilbert spatial sort | 297.5 MB | 26.5, 26 | no gain |
| + id sort | 298.8 MB (identical) | not run | no effect (ids are already ascending or absent) |
| + tessellation | 308.6 MB | 28.5, 28 | rejected: MapLibre 6.14 does not read the triangles (it runs earcut in the worker), so it only adds 3% bytes |
| Drop 8 tooltip-only `track` columns (el, v, fq, g, usage, sv, br, ne) | 266.4 MB (-11%) | 27.5, 60.5 (outlier), then 26, 30 vs control 31, 29 | rejected as a speed lever: the worker decodes off the main thread; the main thread no longer decodes per tile. Worth about -11% bytes only |
| Wire format v2 (tag 2; delta2, packed dict codes, ALP) | 274.7 MB (-8%) | tiles fail to decode | rejected: the shipped JS decoder (`@maplibre/mlt` 1.3.0 in maplibre-gl 6.12 and 6.14) accepts tag 2 but cannot read what mlt-core 0.20.4 writes with `unstable-v2`; revisit when maplibre-gl ships a v2 decoder |

MapLibre 6.14 has no faster MLT path: `MLTVectorTile` / `MLTVectorTileLayer` still call `getFeatures()` for every layer and build one property object per feature; no lazy per-layer decode or option exists. So per-column decode cost cannot be cut from the app side; only the data model (fewer string columns) shrinks it, and the bench says it no longer matters on the main thread.

Final confirmation (full 5-view quick bench, headed 120 Hz, throttle 4, delta 127, 3 measured runs + 1 warm-up, medians, load average 2.3 to 4.5 from other apps; "original" = the build before this task: 6.12, feature state):

| | Original r1 | Final r1 | Original r2 | Final r2 |
| --- | --- | --- | --- | --- |
| Chengdu East | 23 | 16 | 24 | 17 |
| Shenzhen | 16 | 10 | 18 | 9 |
| Guangzhou | 9 | 6 | 8 | 6 |
| Beijing | 10 | 3 | 11 | 4 |
| Shanghai | 11 | 6 | 11 | 4 |
| Total missed | 69 | 41 | 72 | 40 |
| Dropped | 33 | 23 | 36 | 22 |
| Jank ms | 2296 | 1982 | 2398 | 2222 |
| Max ms | 92.5 | 91.8 | 91.2 | 93.4 |

Missed frames -42%. The max stays about 92 ms: that is the first-zoom CJK glyph hitch above, in Chengdu East only (Beijing and Shanghai max 18 ms). `cargo test --release` 4 passed; `npm run build` passes.

## Self-hosted subset glyphs, and MVT against MLT on the final app (2026-10-15)

(2026-10-17: the CJK part of this is superseded, CJK is drawn locally again; `docs/00-overview.md`, row 2026-10-17.)

**Glyphs (done).** `pipeline/glyphs/build-glyphs.mjs` (see `pipeline/README.md`) writes `app/public/glyphs/Noto Sans Regular|Bold/<start>-<end>.pbf` with only the 3,288 code points the map can draw (label properties `n`, `ne`, `lb`, `lbe` of every pilot and Americas tile, plus `places.json`, ASCII and upper-case forms; the hover cards and side files are DOM text, not glyphs, so they are not in the set). 2,670 Han, 105 hangul, kana only as the middle dot, Cyrillic, Arabic, Lao, Tibetan, Mongolian. 138 files per stack, 2.4 MB (Regular) + 2.5 MB (Bold) raw, 3.3 MB gzipped. CJK font: Noto Sans CJK SC (Simplified Chinese design, one design for all Han; the data has no Japanese text beyond U+30FB, Korean only as hangul in the Yanbian stations, which every CJK font draws the same; Hong Kong Traditional names use the SC shapes for the few characters whose regional forms differ). `localIdeographFontFamily: false`.

Measured (headless, cache off, three views Chengdu z12, Chengdu z10.2, Beijing z11.5): 93 glyph requests, 1,893 KB (cold z12 78 files 1,577 KB, then 8 files 225 KB, 7 files 92 KB), against 97 files and 13.0 MB with the whole OpenFreeMap font. Per view that is not "a few hundred KB": the first dense CJK view costs about 1.6 MB raw (about 1 MB gzipped; vite preview does not gzip, so check the host compresses `.pbf`), because 2,800 characters are spread over about 100 ranges and any view at z12 touches most of them in two weights. Levers if that matters: draw CJK in Regular only (halves it), or drop Bold CJK. Hunchun (hangul) z10 and z12 and Ma'anshan, Shanghai z14 (the middle dot) render with no missing glyph boxes; 35 code points (Canadian syllabics, Tai Le, Yi, a keycap mark) and 2 above U+FFFF are in no source font and draw blank, as before (OpenFreeMap has none either).

Quick bench, headed 120 Hz, throttle 4, delta 127, Chengdu East + Shenzhen, 2 runs after 1 warm-up, ABAB, total missed frames (load average 1.9 to 4.4): before (local font, OpenFreeMap glyphs) 26.5, 25; after 2, 2. Chengdu East 17.5, 16.5 to 1, 1; max frame 87 and 88 ms to 17.5 and 18.1 ms; jank 917, 956 ms to 741, 745 ms. The first-zoom hitch is gone.

Visual differences from the font change (screenshots kept locally only, not in the repo): Chengdu z12 1.1% of pixels differ by more than 40/255, Chengdu z10 0.13%, Beijing z11.5 1.9%, Hunchun 0.09%, Ma'anshan z14 0.24%. The glyph shapes are Noto Sans CJK (the macOS local font before was PingFang-like), slightly lighter and wider; some labels wrap or sit a few pixels differently, and a label is dropped or added where the width changed. Latin labels come from Noto Sans, as with OpenFreeMap's font before, so they are unchanged.

**MVT against MLT on the final app (the same code, glyphs, places.json and side files; only `encoding` and the pilot file differ).** MVT = tippecanoe with the flags of the removed `tiles::tippecanoe` plus `tile-join` over the current `data/<slug>/tiles_tmp` (the `zoom` member renamed back to `tippecanoe`), so the input is the same as the MLT writer's; the old scratch MVT in the session folder had other SF bounds and side files and was not used. Scratch copy of `app/` with `encoding: 'mvt'`, nothing left in the repo. MVT Americas built the same way (222.0 MB).

| | MVT | MLT |
| --- | --- | --- |
| pilot.pmtiles | 343.7 MB | 298.8 MB (-13%) |
| americas.pmtiles | 222.0 MB | 205.9 MB (-7%) |

Laptop view weights, KB compressed (1440x900; z5 / z6 / z8 / z10 / z12): Chengdu East MVT 641 / 305 / 77 / 109 / 85, MLT 479 / 216 / 58 / 76 / 66. Shenzhen MVT 341 / 440 / 109 / 134 / 126, MLT 255 / 321 / 87 / 91 / 100. Beijing MVT 716 / 379 / 116 / 133 / 154, MLT 537 / 292 / 88 / 90 / 111. Phone (390x844) MLT is 15 to 35% lighter in every cell (Chengdu East z5 289 to 211, Beijing z6 256 to 202). MLT is 20 to 30% lighter per view.

Frame bench (headed 120 Hz, throttle 4, delta 127, alternating MVT / MLT / MVT / MLT; median across repeats; "dropped" is frames over 1.5 budgets; p95 / p99 are 10.1 / 10.3 ms for all):

| Condition | | Total missed | Dropped | Jank ms | Max ms | Load avg at start |
| --- | --- | --- | --- | --- | --- |
| Full 5 views, 3 runs + 1 warm-up, 4 repeats each | MVT | 8 (9, 8, 8, 7) | 6.5 | 1,960 (1,744 to 1,993) | 26.2 | 1.5 to 2.5 |
| | MLT | 7.5 (6, 8, 7, 9) | 6.5 | 1,742 (1,669 to 1,956) | 26.1 | 1.4 to 2.6 |
| Quick, 2 views, 2 repeats | MVT | 2.25 (2.5, 2) | 2.25 | 692 | 17.6 | 1.7 to 2.7 |
| | MLT | 2.25 (2, 2.5) | 2.25 | 710 | 17.5 | 1.8 to 1.9 |
| Quick + 50 ms latency, 2 repeats | MVT | 1.75 (1.5, 2) | 1.75 | 774 | 17.9 | 1.9 to 2.3 |
| | MLT | 1.5 (1, 2) | 1.5 | 772 | 16.1 | 2.0 to 2.2 |

Per view, full bench (MVT / MLT): Chengdu East missed 1 / 1, jank 383 / 344; Shenzhen 1 / 1, 388 / 348; Guangzhou 3.5 / 3.5, 408 / 364; Beijing 1 / 1, 394 / 338; Shanghai 1 / 1, 376 / 346; max 17 to 26 ms in both; idle after the last wheel tick 405 to 412 ms in both; tile requests per view about the same (Chengdu East 81 / 83, Shenzhen 102 / 101, Guangzhou 120 / 125, Beijing 82 / 81, Shanghai 108 / 111). Result: **no measurable difference in missed or dropped frames**; MLT has about 11% less jank ms in the full bench (every view, every repeat but the last), inside the quick bench's noise. The first full pair differed by more than 25% in total missed (6 against 8, two frames), so two more pairs were run; jank was stable (within 15%). Bytes transferred per view are not reported: the bench's `encodedDataLength` counts cache hits as 0 and varied from 8 to 417 KB between identical runs; use the view weights above. With the glyph fix both encodings stay at 1 to 4 missed frames per view, so MLT is a size and build-time win (13% smaller files, 4.9 s against 36.8 s for China), not a frame-rate win or loss.

Side-by-side screenshots (kept locally only, not in the repo): rail geometry, colours, dashes and station dots match; 0.01 to 0.5% of pixels differ by more than 40/255 (Paris z13 highest), almost all label placement and a few line-name labels present on one side only.

## Must fix: Europe low-zoom tile weight (user, 2026-10-13)

Superseded by the measured plan in `docs/16-low-zoom-tile-weight-plan.md`.

Adding France, Switzerland and Italy (with Germany) made the zoomed-out views heavy: laptop views at z5 and z6 across Europe are 590 to 1,030 KB (France z5 936 KB, Lyon z6 1,026 KB, Switzerland z6 993 KB, Germany z6 600 KB), over the 600 KB budget, because the four countries' z0 to z8 overview tiles share the same tile addresses and the joined file puts them in one big tile (history: this was measured when `tile-join` did the join, and tippecanoe wrote the tiles; both are gone since 2026-10-14, and MLT with a single writer run measured 923 KB for the worst pilot-file laptop view, so the problem remains). Every region added in the same part of the world makes it worse (Paris z5 went from 60 KB to 906 KB). Not acceptable for release; the zoom-smoothness target (fast zoom must beat trains.theojaffee.net) applies. To do, before more countries are added:

- Merge the overview harder at z5 to z8 (corridor merging, fewer attributes, drop `ln` / `oc` / `kmh` at the lowest zooms) and measure with `TILES=pilot python3 pipeline/measure_views.py`.
- Thin by importance at low zoom (main lines only at z5 and z6) and check that nothing needed for a click is lost.
- Cap the work per tile: a per-tile feature or byte limit for the overview.
- Re-test the whole of Europe together after each change; the budget is per view, not per country.
- Success test: no laptop or phone view over 600 KB for the worst Europe views (France z5, Lyon z6, Zurich z6), and the zoom/pan bench not worse than before the countries were added.

## Must fix: glyph download size (user, 2026-10-15) - resolved 2026-10-17: CJK is drawn locally, see `docs/00-overview.md` and `docs/11-performance-todos.md`

The self-hosted CJK subset costs about 1 MB gzipped across 32-43 files on a first dense China view (cold: Chengdu z12 78 files / 1,615 KB, Chengdu z10.2 32 / 982 KB, Beijing z11.5 82 / 1,693 KB; Paris 78 KB, SF 73 KB), against 87 KB for the original. Why it exists: self-hosted glyphs removed a 90 ms main-thread canvas stall at the first zoom; a local CJK font costs no download but costs main-thread time. Ideas, measured offline only (nothing shipped): serve CJK from one weight (Bold CJK ranges redirected to the Regular files with `transformRequest`, so each range is fetched and cached once); prune characters used by one label only (21% less on disk, 603 of 117,165 labels lose one glyph); immutable cache headers on `/glyphs/*` (`public/_headers`, purge after regenerating); host compression of `.pbf` (gzip takes 32% off; `vite preview` does not compress it). Success test: first dense China view under 400 KB with no new missing glyphs, zoom bench not worse.

## To do later

- **Glyph download size**: moved to Must fix below (2026-10-15).

- ~~**Move off tippecanoe**~~ DONE 2026-10-14 (user, 2026-10-13). `build` and `--combined` write MLT through `tilewriter.rs`; `tiles::tippecanoe`, the `tile-join` step, the MVT encoder and the dev-only `?enc=mvt` / `?tiles=` hooks are gone. Pilot, the six pilot regions and Americas regenerated as MLT: `build sf-bay-area paris china de fr ch it` 40.4 s wall, 14.1 GB peak RSS for the full build (the parallel per-region station models plus one writer at a time), pilot join 12.7 s (310.5 MB; 9.3 GB writer-only peak); Americas `--combined` 15.3 s wall, 11.4 GB peak, 207.9 MB. Run with PATH lacking tippecanoe.
- ~~**Fast Rust pipeline from OSM to MLT**~~ DONE 2026-10-14 (user, 2026-10-13): `rail-extract build` goes from the extractor's `tiles_tmp` to MLT with no tippecanoe or MVT step. Still open: its memory (15 to 30 times tippecanoe's: the input is parsed whole); a combined build bigger than Americas needs slicing per top-level tile or a streaming reader. The only tile-input field renamed: the per-feature zoom range is now the `"zoom"` member (was `"tippecanoe"`), so `tiles_tmp` written before 2026-10-14 is refused by the writer ("stale tile input"): rebuild the region.

## Parked (not in v1)

- GTFS of any kind: frequencies, timetables, Portolan, train animation
- Live train positions
- History timeline (`start_date` is already kept in the data for it)
- Comparing custom drawn areas (H3 grid or DuckDB in the browser)
- Routing, accounts, saved views
- Signalling and owner themes (would need train protection and `owner` tags)

## Hover lag at very low zoom on MLT: the highlight filter reloaded the main tile source (2026-10-15)
- Symptom: hovering rail lines from z3 to z8 was laggy on MLT (original feature-state MVT app: fast). Measured with a synthetic mouse sweep (650 moves over the busiest rows, China and Europe, headless, CPU throttle 4; scratch hv/hv.mjs): per-move handler time at z3 3.0-3.5 ms (original 1.2-1.4), 17-34 frames over 25 ms and 3-6 over 50 ms per sweep (original 0), the same at z5-z10 and z12 in Europe (4 ms, 50-60 frames over 25 ms).
- Root cause: `setFilter` on `pilot:hl-fs` makes MapLibre reload every tile of the layer's source (here the main rail source). A reloaded tile gets a new feature index, so its decoded copy is gone, and the next `queryRenderedFeatures` (every hover frame) decodes each queried MLT tile whole again on the main thread. Overview tiles at z3-z8 are the largest, so that is where it showed. MVT parses lazily, and feature state never reloaded tiles, so the original never paid this. The query itself was not slower per call; it was re-decoding after each highlight change.
- Fix (mapStyle.ts): `pilot:hl-fs` has its own vector source `pilot:hls` (same PMTiles URL, minzoom 9, so no tile below z9). A filter change now reloads only that source; the main source keeps its decoded tiles. `showHighlight` no longer sets the clearing filter before the new one (one reload per change instead of two).
- Result, per-move handler ms, z3/z6/z9/z12, China: original 1.4/1.3/0.7/0.6; before fix 3.0/1.7/1.5/0.9; fixed 1.0/0.8/0.3/0.3. Europe z3 to z12 fixed 1.1/1.3/1.1/0.8/0.6/0.6/0.5/0.5 against original 1.2/1.8/1.4/1.2/0.8/0.9/0.6/0.6. Frames over 25 ms during a sweep: 0-1 everywhere (before: up to 59).
- Checked: screenshots with hover at z9, z12, z13.5 identical (0 px differing) to the build before the fix, same highlighted-feature counts, click and hover cursor identical at z6 and z12. Zoom bench quick preset (headed, throttle 4, ABAB): 2 and 5 missed before, 2 and 3.5 after. The second source adds tile loads only from z9; no regression seen.
- Left: a filter change still reloads the `pilot:hls` tiles in the worker (cheap: one layer). An alternative with no reload at all would be a highlight drawn from a GeoJSON overlay at every zoom, but the side-file geometry is simplified to 100 m, too coarse above z9.


## Original MVT app against the final MLT app (2026-10-15)

Whole benchmark, metric by metric, with WIN / LOSS / TIE verdicts: the orig-vs-mlt report (local only, not in the repo; its key numbers are in `09-performance-playbook.md`). Original = commit 913ddff (MapLibre 6.12, feature state, local CJK font, tippecanoe MVT 343.7 MB); final = current tree (MLT 298.8 MB, MapLibre 6.14, `pilot:hls` hover source, self-hosted subset glyphs).

- Faster in the final app: tile build (China tile step 31.5 s to 4.1 s; full 7-slug build 88 s to 27 s), compressed tile bytes (0.74x over the standard views, worst laptop view 994 to 806 KB), fast zoom at 120 Hz (54 to 14.5 missed frames, max frame 98 to 32 ms), realistic zoom out under latency (65 to 33 missed), click at z9 (43 to 4.6 ms), map `load` under latency (0.5-0.7x). Hover handler, first rail pixels and 0 ms load are ties.
- Slower in the final app: (1) settled idle under latency is 14-34% longer because `pilot:hls` requests the same PMTiles ranges as `pilot`, doubling pmtiles requests (1.9x) and queuing 10-12 duplicates behind the real tiles; fix: dedupe tile fetches across sources in the pmtiles protocol handler, or add `pilot:hls` only on the first hover. Without the duplicates the measured timelines settle faster than the original. (2) China views download 11x more glyph bytes (959-1037 KB vs 87 KB) and 18-23% more in total; not on the critical path, but a mobile cost; fix: one CJK weight, pruned subset, immutable cache headers. (3) Tile writer memory 5.3 GB vs 0.35 GB for China (full build +7%). (4) Zoom out to the world (z2) is a tie with 3-4x run-to-run noise; overview vertex count (z3-8 1.5-1.7x) stays the suspect.
- Not decided: whether to apply fixes (1) and (2); they change app code, so they were only measured and proposed.
