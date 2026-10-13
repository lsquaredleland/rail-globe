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
| Tile layout (phase 3) | Decided 2026-10-13 (see `00-overview.md`): the pilot is one joined tile source, `pilot.pmtiles`, built from per-region files. Phase 5 still decides the world layout. Tier 1/2/3 stations appear in tiles from z7/z9/z11 and on screen (circles) from z8/z10/z12; labels come from the complexes layer from z10/z11/z13 |
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

## Smooth rendering and zoom: to do

Listed 2026-10-13, updated the same day after the fast-zoom profile. Numbers: `reference/research-summary.md` ("Fast zoom: profile and comparison"). How to measure: `app/bench/README.md`. The profile shows warm zooming is limited by the main thread (symbol placement, painting), not by tiles.

Done or dropped:

1. **One tile source instead of three.** Done (`00-overview.md`). Missed frames 128 to 76 at 6x.
2. **Drop the high-speed twin layers.** Being done now (2026-10-13). Every track layer has a `-hs` copy so high-speed draws on top; a different draw order would halve those layers.
3. **Lighter tiles (tooltip-only tags to side files).** Tried and dropped: 8.5% smaller `china.pmtiles`, no frame gain. A cheaper variant is to pack those tags into one property.

Open, by measured payoff:

4. **Trim basemap layers.** The biggest measured cost: hiding the basemap took 18 missed frames to 6.5.
5. **`fadeDuration` 150 instead of 0.** Measured gain (18 to 14; placement 24% to 8% of busy time). It changes how labels look: they fade in instead of popping. **Needs the user's decision.**
6. **Hide the `hl-fs` layers when nothing is highlighted.** Small gain (18 to 15.5), free.
7. **Align tile `minzoom` with style `minzoom`.** Five layers carry data a zoom before they are drawn (list in the research summary). Free bytes; not yet measured.
8. **Stronger low-zoom simplification (`--low-detail`).** Not measured.
9. **Corridor merging prototype at z8-11.** Not measured.
10. **Stop tiles at z13 and overzoom.** The reference stops at z12. Not measured.
11. **MapLibre Tiles (MLT).** MapLibre 6 reads MLT; tippecanoe does not write it, so it needs a conversion step.
12. **Self-host Latin glyphs** (CJK stays on local fonts), so the first labels at a new zoom do not wait on a font download.
13. **Production hosting.** `r2.dev` has no Cloudflare cache and is rate-limited. Use a custom domain with cache, or chunked tiles read by HTTP Range like the reference.
14. **Pre-rendered raster tiles for z0-5.** Also fixes the z5 China views over the 600 KB budget.
15. **Move the tooltip with a CSS transform** instead of `top`/`left`.
16. **Per-layer `minzoom`/`maxzoom`** on every layer so off-zoom layers are skipped early. Not tried.

Tile options the user wants benchmarked (items 7, 8, 11, plus 9 and the drop-ids idea): 1 align zooms, 2 simplification, 3 drop ids at z8-9 (the user is hesitant: it may affect presentation), 4 corridor merge, 7 MLT. Measure them on cold-cache and throttled-network scenarios and with trackpad pinch input, since the profile shows tiles are not the limit for warm zoom. The bench does not cover those two scenarios yet.

Tried, no measurable gain (2026-10-13): `maxTileCacheSize` 200/500 (MapLibre 6.12 can only lower it; `maxTileCacheZoomLevels` is the raising knob), `cancelPendingTileRequestsWhileZooming` off plus `maxTileCacheZoomLevels` 8 (18 to 14.5, small), `renderWorldCopies: false`. `crossSourceCollisions: false` was tried and not kept: it made basemap labels overlap station labels.

## To do later

- **Move off tippecanoe** (user, 2026-10-13). Long-term goal; weigh tippecanoe-only settings against it.
- **Fast Rust pipeline from OSM to MLT** (user, 2026-10-13). If the MLT benchmark (item 4 above) shows MLT is faster than MVT, rework the pipeline to write MLT directly from the Rust extractor (`pipeline/rail-extract`), without tippecanoe or a separate MVT-to-MLT conversion, and make it as fast as possible.

## Parked (not in v1)

- GTFS of any kind: frequencies, timetables, Portolan, train animation
- Live train positions
- History timeline (`start_date` is already kept in the data for it)
- Comparing custom drawn areas (H3 grid or DuckDB in the browser)
- Routing, accounts, saved views
- Signalling and owner themes (would need train protection and `owner` tags)
