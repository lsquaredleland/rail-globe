# Open action items (the one to-do home; merged 2026-10-20)

Everything fixed in the 2026-10-16/17 session has been removed from this list; what was done is in `CHANGELOG.md` (root). What is left needs a decision from the user, a check, or follow-up work. Reply under each `Your call:`.

Decided and closed (no action left): the app is glyph-free, all text drawn locally from system fonts (docs/00-overview.md 2026-10-17); 朔黄线 stays Freight; The Ghan and Indian Pacific both Intercity; Uruguay none; Peru, Bolivia, Cuba left alone; China tier-1 override stays narrowed; Berlin and Hamburg S-Bahn and Copenhagen S-tog stay Light rail; ICE, Frecciarossa, Railjet name overrides left out; the 150 km Intercity cut-off stays; the world build is the default (CLAUDE.md); stale country folders deleted and pruned.

## 1. Decisions

### 1.1 Europe z5 laptop view is 678 KB (budget 600 KB)
Measured on the final world build: only this zoom fails (303 of 48,960 view centres, worst at lon 7, lat 50). Every phone view passes. Worst per zoom, laptop / phone KB: z4 386 / 296, z5 678 / 501, z6 574 / 364, z7 399 / 236.
- 4a: cut `ln` to its first id below z9: about 562 KB; the tooltip then lists one service, which conflicts with the 2026-10-16 decision in `00-overview.md`.
- 4b: regional track starts at z6: z5 about 364 KB, z6 (574 KB now) stays the worst view; the z5 regional draw changes.
- 4c: leave as is.
- My lean: 4c now, 4b later if Europe z5 bothers you.
- Your call:

### 1.2 French TER K1 Paris Est to Strasbourg and Jordan "International Train"
The blanket `^TER ` override makes TER K1 (about 450 km) Regional; Intercity looks more sensible. Jordan's "International Train" (51 km) is now Freight; it is probably the Hejaz alignment, so Other is also defensible.
- Your call:

### 1.3 Desktop operator mode and the open-sea region button
In operator mode zoomed in over a country there is no card and no legend (the operator Legend was removed); zoomed out it shows the World card. Over open sea at z7 the region button still names the nearest country ("Portugal Country"). Keep both, or change?
- 2026-10-20: the user noticed the missing card and wants it fixed later. Likely fix: show the region card (totals and rail-type switches) in operator mode too, with the "lines take each operator's colour" note (`App.tsx`: `mode !== 'operator' && <TypeCard>`).
- Your call:

### 1.4 Under-construction stations: shared predicate (proposed)
The app already shows under-construction stations only when the construction flag is on (layers `stcon1-3`, `lblcon1-3`, built by `construction.rs`). `stations/` now keeps them out of the open-station model, but `construction.rs` only recognises some tag patterns, so a station skipped by `stations/` for `railway=station` + `construction=yes|station` or a `（在建）` name suffix (cn: 63 + 15 + 181) is never drawn, even with the flag on. Proposed: one shared predicate used by both files so every skipped station appears with the flag on. Small change, changes data, needs one more world build.
- Your call:

### 1.5 Station construction skip: tags that cannot prove "open"
Kept open because they carry both an open tag and a construction tag, unverifiable from tags: Montpellier tram line 5 stops, the Dudley extension, au Trentham, Lyonville and Wombat State Forest, kr 하양 and 군위. Still skipped (tags say "being built", unverified): de Sielmingen and Neuhausen auf den Fildern; fr four Ajaccio stops; gb one unnamed node; it Canepari and Pozzuoli; kr nine stations (과천정보타운 to 합천); ch Ils Cugns; th six Bangkok MRT stations. A manual check of a handful settles it.
- Your call:

### 1.6 HSR-only label filter reads `hsr` on complexes
In HSR-only mode the station label layers (`lbl1`-`lbl3`, `lblcon1`-`lblcon3`) filter on `hsr == 1` or `ty` containing `high_speed`, but the `complexes` tile layer has no `hsr` property, so for complexes only the `ty` test applies. Nothing visibly breaks. Found by `app/src/tileSchema.test.ts` (listed there as `KNOWN_MISSING`). Options: add `hsr` to complexes in the pipeline (tiles change), or drop the `hsr` read for complex labels.
- Your call:

## 2. To investigate

- **China services from 12306** ([2026-10-20-china-12306-services](00-overview.md#2026-10-20-china-12306-services)): before publishing, a decision on bandwidth (a zoomed-out Chengdu East click is ~689 KB uncompressed, of which `stretches.json` 586 KB / 149 KB gzip; `r2.dev` serves no compression; options: custom domain with compression, or stretch geometry in tiles); 15 chord pairs incl. the Hainan ferry (no `railway=ferry` in the `cn` cut); 郑州东–新乡东 G legs switch 京广高速 -> 济郑高速 (likely one corridor under two names); 汨罗 unmatched; 189 stations without English; 井店–涉县 doubtful 5.2 km bridge; publishing needs the user's go.
- **Turkey station tiers fell back** after the last station fix: tier 1 10 to 5, tier 2 14 to 7; Şakirpaşa, Lüleburgaz YHT and Merkez Otogar went from tier 1 to tier 3; Gürsu, Yenişehir YHT, Karacabey, Teknosab from tier 2 to tier 3. Not traced. Assigned to the stations agent.
- **Search card rules:** a picked city without its own outline collapses to its province (Seoul City to `kr-seoul`; Manchester City shows England Province at z12). The area card picks the largest area that fills half the screen; a picked place only keeps the card while it fills 15% of the width. Decide whether a picked city should pin its card.
- **Hover:** the idle-prefetch freeze is fixed (commits 6fe306f, ce2661e). A fast sweep over many lines still fetches 20 to 23 shards (4.5 to 5.2 MB) on demand because r2.dev serves HTTP/1.1 without compression; a headed 120 Hz test was not possible (machine load 20 to 40). If lag remains after deploy: does it happen only right after a pan, is the page frozen or the highlight late?

## 3. Small data items

- PNR South Main Line (Philippines): the 207 km Intercity is the Lucena–Sipocot gap with no route relation; one source says it was not operational. Retype it?
- Uzbekistan Miskin–Bukhara high-speed section: nothing specific found, left as is.
- Delhi Pink Line way/364876210 (0.42 km, `construction=subway`, no route) is closed by the construction rule; probably a leftover tag. Ruhrbahn tram tunnels tagged `construction=subway` (5 ways) are closed; skip `tunnel=yes` tram ways?
- Kenya old Nairobi–Mombasa metre-gauge relation: retyped; the end of passenger service could not be confirmed from a source.
- Station labels: 17 German `Hp ` names without a dot, 1 bare "Hauptbahnhof" (Berlin Parkeisenbahn), `Męcina [Nowa]` (pl), 201 cn names with brackets (mostly place qualifiers), nameless stations (de 30, jp 40, gb 44, us 107, fr 31, cn 116); duplicate dots within 100 m: jp 56, de 31, us 93, gb 28, cn 17, pl 15, fr 11 (mostly a node beside the way or relation of the same station).
- Station model: Sydney Macdonaldtown stays tier 1 (19 "platforms", 17 ref-less depot ways); Agra Cantonment missing (OSM tagging); Venezia Santa Lucia and Arnhem Centraal lower after the `1N` platform-ref collapse; Kyoto's JR station is named by the Kintetsu stop 近鉄京都; "St Pancras Low Level station".
- Optional overrides not added: Toros Ekspresi (Intercity) and Serbian REx Петроварадин–Београд (Regional), both promoted to High-speed by the 0.5 track share.
- Japanese Han characters draw with Chinese letterforms (no language is set); the first-zoom hitch is about 70 ms at 4x throttle (bench: Chengdu East 3 missed frames, max frame 66.7 ms).

## 4. Hosting and publishing (needs the user's go)

1. Publishing deploys the live site: `pipeline/publish.sh` (build, verify, upload, push, prune, live checks) after the checks in the changelog. Never run without a go. Done 2026-10-20 for the world data (195 files updated, prune deleted nothing).
2. Done 2026-10-20: `r2/sync.sh` ran, so `shards.json` is `no-cache`; it prunes dropped slugs through `r2/dropped-slugs.sh` (guards: kept/dropped overlap, `world.parts.json`). It now uploads and prunes only `world/`, and the bucket holds nothing else (3,424 stale objects, 1.35 GB, deleted).
3. r2.dev is HTTP/1.1, uncompressed, tiles `max-age=3600`: custom domain checklist is in `r2/README.md` (compression rules for `.json` and `.pbf`, build-dated tile file names for immutability, then disable the r2.dev URL).

## 5. Follow-ups, none blocking

- Repeat the P2 measurements on an idle machine (`archive/performance-todos-2026-10-17.md`): highlight filter cost on very long lines, Chengdu settle time at 150 ms latency, zoom out to z2, hover and click sweeps, first-zoom hitch (roughly doubled by the glyph-free decision: Chengdu East max frame 75 to 125 ms, one noisy pair).
- Not covered by QA: Iceland and the micro-states, operator and speed colour modes in the Americas and Africa views, hover and click on rebuilt tiles.
- More independent answer-list entries; the 29 known failures are judgements from Wikipedia rankings and the size score, not confirmed OSM bugs.
- Pipeline robustness: the `type_source` of a route master comes from its first member, not the winning member.

## 6. Performance, hosting and research (moved from the old 06 and 11, 2026-10-20)

Evidence and numbers for each item: `archive/performance-todos-2026-10-17.md` (old file 11) and `archive/open-questions-2026-10-20.md` (old file 06).

### Blocks a release
- **Low-zoom tile weight over the 600 KB view budget.** Plan, measurements and the ordered steps: `16-low-zoom-tile-weight-plan.md` (steps 1 to 3 built; Europe z5 laptop is the only failing zoom, see 1.1 above). Done when no laptop or phone view is over 600 KB in the listed views (Europe z4 to z7, world z3, Moscow, Cairo, Delhi, Mumbai, Seoul, Beijing, Shanghai), `rail-extract world` is still byte-identical for `--deep-zoom 5` and `6`, and the zoom bench is no worse. Measure with `python3 pipeline/measure_views.py` or `rail-extract pminfo --tsv`.
- **Custom domain for R2 (needs the user's go).** Live on `r2.dev`: HTTP/1.1, no compression, tiles `max-age=3600`, no edge cache. Done when a custom domain is attached (`VITE_DATA_BASE`, `connect-src` in `app/vercel.json` and `r2/cors.json` changed together, `r2.dev` disabled), `curl -I` shows HTTP/2 or 3, a shard file has `content-encoding`, a second tile range shows `cf-cache-status: HIT`, and a cold load of Paris and Chengdu matches the localhost request counts. Option if edge misses hurt: split the 1.18 GB world file by zoom. Keep `dedupedPmtiles` in `MapView.tsx` (range requests are billed per operation). Checklist: `r2/README.md`.
- **Freight spot check (US, Canada, Mexico).** The rule is built (`passenger_needs_evidence`, `13-freight-north-america.md`). Still to do: a spot check of the US Midwest and Mexico against the answer lists; freight must show as freight, the pilots unchanged.

### Search and app wiring
- Compare mode is disabled (phase 4 skipped), so "results target the focused map" is not built.
- Two cities with the same name (Tokyo city and province) both list; the city has no outline. No search-as-you-type for stations or lines. Countries have `stations` 0 in `search.json`, so ties follow file order (rail size).
- The "Done when" renders (z3, Cairo z9, Tokyo z9) were not re-run as a set.

### Measurements to repeat on an idle machine (3 runs, ABAB; method in `09-performance-playbook.md`)
- Highlight filter cost on very long lines: `setFilter` on `world:hl-fs` (source `world:hls`, minzoom 9) with a long way-id list. Not traced. Done when worker time per highlight is known and under 20 ms or reduced (a Set instead of a literal list).
- Chengdu settle time at 150 ms latency (6,462 ms against 6,066 ms for the original, one run each).
- Zoom out to the world (z2): a tie with 120 to 170% spread; repeat against the world file.
- Headed hover and click sweeps (rail at z3 to z12, count frames over 25 ms).
- First-zoom hitch (glyph-free app, Chengdu East max frame 75 to 125 ms). Accepted for now.
- Headed measurement on the user's 120 Hz display of the slowdown the user reported after the themes, place labels and the Europe countries (2026-10-14); A/B each change.
- Cold-load numbers, gzipped shard sizes and click latency from the real host; a full `r2/sync.sh` into an empty bucket in under 5 minutes.

### Research lists never finished (details in `archive/open-questions-2026-10-20.md`)
- Faster "HSR only" toggle: eight ideas (constants not expressions, no filter change on a toggle, a high-speed tile layer, skip validation, a veil), nothing measured. First step: a trace of the toggle at z4 to z6.
- Smooth rendering and zoom, items 4 to 10 and 13 to 16: trim basemap layers (biggest measured cost: 18 to 6.5 missed frames with the basemap hidden), hide the `hl-fs` layers when nothing is highlighted, align tile and style `minzoom`, stop tiles at z13, corridor merging, pre-rendered z0 to z5 raster, CSS-transform tooltip, per-layer `minzoom` and `maxzoom`.
- Highlight performance ideas not tried: fewer or merged layers, `renderWorldCopies: false`, a deck.gl path layer for the hovered line, delivery tuning.
- China high-speed rule beyond Chengdu (check on Beijing-Tianjin or the Pearl River Delta). Parked: a flag to apply the non-China rule (`highspeed=yes` or 200 km/h and above) in China; 200 km/h lines are promoted one by one with `high_speed_track_overrides`.

### Provisional choices (the code decided where the docs were silent; they stand until the user changes them)
Moved here from the old file 06 (open questions) on 2026-10-20. Two still wait for the user's confirmation:
- Major station rule for high-speed termini: "mainline station within 80 m of high-speed track and (terminus or 8+ mainline platforms)" replaces the prototype's route-relation test. Confirm.
- Tier names against the design: the UI shows "Major HSR station", "Major station", "Minor station"; the design has no Terminus badge. Confirm.

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
| Tile layout (phase 3) | Decided 2026-10-13 (see `00-overview.md`): the pilot was one joined tile source (since removed). The world layout is decided and deployed (2026-10-16): one `world.pmtiles` from `rail-extract world`, the only source the app reads (`10-world-build.md`). Tier 1/2/3 stations appear in tiles from z7/z9/z11 and on screen (circles) from z8/z10/z12; labels come from the complexes layer from z10/z11/z13 |
| Design extras left out (phase 3) | Compare view (switch shown, disabled until phase 4) and the Terminus badge in tooltips (shown only in the station panel, per the design). Units (km/mi), Copy link and Inspect OSM tags are in |
| Inspect OSM tags | Shows the tags the data spec keeps (not the full OSM tag set, which the filter drops) plus fields Rail Globe derives (rail type and how it was set, routes, stop area, station model fields). Tags load on demand from 256 id-sharded files per region (`data/<slug>/tags/`), so they are not in the tiles. Hover shows tags; click pins a panel with Open in OSM and Copy tags |
| Airport icons | The plane icon and IATA code sit at the centre of the airport outline (commercial airports only: IATA code, not military, 300 ha or more; smaller airfields get outline and code, no icon). Stations at airports are ordinary circles; their tooltip and panel still say Airport. The track tooltip no longer shows a Tracks row (the value was unknown for most track) |
| Yard and area tooltips | Yard track shows its name, service type, electrification and gauge. Station grounds, yards, station buildings and platforms show kind, name, area and operator. "Station grounds" areas are whole `landuse=railway` polygons, which can span a corridor (one in Paris is 224 ha), so the area figure is not a station's size |
| Major station rules: what the prototype had | The pasted description (MAIN_RE, MAIN_BY_GROUP / MAIN_SET, hs_idx / hs_major) is not in this repo, so the rules were rebuilt from the description. Chengdu uses the name pattern only (no canonical list); the China city list in `config/chengdu.json` is short (成都, 天府, 重庆, 西安, 北京, 上海, 广州, 深圳) and needs extending for other cities |
| Major station: high-speed termini | The prototype test (terminus of a high-speed route) reads route relations, which conflicts with "rank from physical size only". Implemented instead as: mainline station within 80 m of high-speed track and (mainline terminus or 8+ mainline platforms). This also covers China, where high-speed routes are thinly mapped. Confirm this is acceptable |
| Construction zoom (changed 2026-10-11) | Under-construction track appears from the zoom of its rail type, so a high-speed line under construction shows at every zoom. This replaces the earlier z10 start in `02-architecture.md` |
| Named railway lines | Where no route relation covers a piece of track (Chengdu has almost no train routes), the track's own `name` is used: as the tooltip title, as a map label from z11.5, and as an entry in the Lines list (grouped by rail type, with approximate length from the track pieces divided by `tracks`). Only names with about 1.5 km or more of track are listed |
| Tooltip tier names | Tier 1 with a high-speed track nearby → "Major HSR station"; other tier 1 and tier 2 → "Major station"; tier 3 → "Minor station" |

### Reminders (not tasks)
- `vite dev` is not a performance reference; bench `vite preview` on a production build. `npm run build` copies `app/public/data/` (several GB with the world files) into `app/dist/`: move the world folder away before building for deployment, or point `VITE_DATA_BASE` at the host.
- Method: keep a baseline worktree on its own port; alternate A/B/A/B; headed Chrome on the 120 Hz display; CPU throttle 4; wheel delta 127; latency 50 and 150 ms; one warm-up; note the load average and repeat when runs differ by more than 25%.

## 7. Parked (not in v1) and research

GTFS of any kind (frequencies, timetables, Portolan, train animation); live train positions; history timeline (`start_date` is kept in the data); comparing custom drawn areas (H3 grid or DuckDB in the browser); routing, accounts, saved views; signalling and owner themes (would need train protection and `owner` tags).

### Research notes (moved from `README.md`, 2026-10-20)
Research notes, not decisions or commitments. v1 stays GTFS-free and fully static (see `CLAUDE.md`), so the GTFS-based tools below are for ideas only.

- **Line number badges** (rounded badge with the line's `ref`, repeated along the track, as on Beijing's map): no library needed. Use a MapLibre `symbol` layer with `symbol-placement: 'line'`, `icon-text-fit: 'both'` and a white stretchable SDF image tinted with `icon-color`; keep the text upright with `text-rotation-alignment: 'viewport'`. Images can be made on demand in the `styleimagemissing` event. Needs `ref` and `colour` in the `lines` tiles. [`@americana/maplibre-shield-generator`](https://cdn.jsdelivr.net/npm/@americana/maplibre-shield-generator@0.1.1/README.md) uses the same on-demand pattern for road shields but is road-specific, so skip it.
- **Interlining and parallel lines** (several lines on shared track drawn side by side, splitting at junctions):
  - MapLibre's built-in `line-offset` paint property works for simple cases; sharp angles can show artifacts. Fixed per-strand offsets baked in the pipeline work with stock MapLibre.
  - [Portolan](https://github.com/alexwohlbruck/portolan) (MIT, Go, MapLibre) does this: it detects corridors where tracks run parallel, orders the routes in each corridor to avoid crossings at the next split, and fans them out at junctions. It is a CLI and workbench driven by GTFS, with no documented library API, so treat it as a design reference. Its note: even spacing at every zoom needs a custom MapLibre build, while fixed pre-baked offsets do not. Busy junction interiors still show artifacts.
  - [LOOM](https://github.com/ad-freiburg/loom) (University of Freiburg; papers: [Efficient Generation of Geographically Accurate Transit Maps](https://ar5iv.arxiv.org/html/1710.02226)) orders lines along shared edges with an ILP to minimise crossings and separations. Its helper tools `topo` (overlap-free line graph) and `pfaedle` (fits routes to real track) match what our line layer needs. Possible to port the ordering idea to the Rust pipeline; not tried.
  - [Turf `lineOffset`](https://atlas.co/functions/turf/turf-line-offset/) computes offset geometry if offsets are precomputed in JS instead of in the pipeline.
  - Design caution (Remix, Esri threads): manual offsets cascade, so one change can hide a line elsewhere. Compute offsets from the line graph, not by hand.
- **Station markers** (white circle, dark ring, name beside it): `circle` layer or a canvas icon like the airport icon in `app/src/mapStyle/icons.ts`.
