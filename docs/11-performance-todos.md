# Performance to-dos and reminders (2026-10-15)

What is left to do on speed, size and deployment, in priority order, with the numbers behind each item so the next person can re-measure. Background and method: `09-performance-playbook.md`. The world build: `10-world-build.md`. Items already tracked in `06-open-questions.md` are linked, not repeated.

P0 = blocks a release or the zoom-smoothness target (fast zoom must beat trains.theojaffee.net). P1 = real cost, fix before the world is public. P2 = repeat a measurement, or only matters at larger scale.

## P0

- [ ] **Low-zoom tile weight over the 600 KB view budget, now worldwide**
  - Symptom: overview tiles (z2 to z8) are heavy wherever many countries or dense networks share a tile address. Budget: no laptop or phone view over 600 KB gzipped.
  - Measured (`world.pmtiles`, 1,150 MB, 1,701,830 tiles, compressed tile bytes per laptop view; scratch script over the PMTiles reader, same idea as `pipeline/measure_views.py`): Europe z4 to z7 up to 1,587 KB; world view z3 973 KB; Moscow, Cairo, Delhi, Mumbai at z3 to z4 over budget; Seoul, Beijing, Shanghai at z5 over budget. Pilot file before the world: worst laptop view 806 KB (Europe, see "Must fix: Europe low-zoom tile weight" in `06-open-questions.md`).
  - Ideas, in the order to try: (1) thin by importance at z2 to z7 (main lines and big stations only; check a click still finds what it needs); (2) drop `ln`, `oc`, `kmh` (and other tooltip-only columns) at the lowest zooms; (3) merge corridors (parallel tracks to one line) at z4 to z7; (4) a per-tile byte or feature cap for the overview; (5) the z6 to z8 vertex surplus: our end-to-end line joining leaves 1.5 to 1.7 times the vertices tippecanoe kept at z6 to z8 (z9 and up are equal): simplify joined lines after the join.
  - First step: extend `pipeline/measure_views.py` to read `app/public/data/world/world.pmtiles` (it reads the pilot file only) and add the views above; then try (2), the cheapest, and re-measure.
  - Done when: no laptop or phone view over 600 KB in the views listed (Europe z4 to z7, world z3, Moscow, Cairo, Delhi, Mumbai, Seoul, Beijing, Shanghai), the pilot views no worse, `rail-extract world` still byte-identical for `--deep-zoom 5` and `6`, and the zoom bench (below) no worse.
  - Where measured: the world file built 2026-10-15 (`10-world-build.md`).

- [ ] **Glyph download size** (user decision pending; tracked in `06-open-questions.md`, "Must fix: glyph download size")
  - Symptom: first dense China view downloads about 1 MB gzipped over 32 to 82 glyph files (Chengdu z12 78 files / 1,615 KB, Chengdu z10.2 32 / 982 KB, Beijing z11.5 82 / 1,693 KB; Paris 78 KB, SF 73 KB; original app 87 KB). Cause: self-hosted CJK subset (removed a 90 ms main-thread canvas stall at the first zoom).
  - Ideas: one CJK weight (Bold ranges redirected to Regular with `transformRequest`, so each range is fetched and cached once); frequency-pruned subset (ship the common few hundred first, long-tail range files on demand; pruning one-label-only characters saves 21% of disk and loses a glyph in 603 of 117,165 labels); immutable cache headers on `/glyphs/*`; `.pbf` compression on the host (gzip takes 32% off).
  - First step: the redirect in `transformRequest` (app only, no rebuild), then re-measure bytes and settle time.
  - Done when: first dense China view under 400 KB with no new missing glyphs; zoom bench not worse.
  - Where measured: headless Chrome, `vite preview`, our `/glyphs/` files only, cold cache.

## P1

- [ ] **World app wiring, places for 146 countries**
  - State: `VITE_TILE_SOURCE=world` (dev switch, `app/src/config.ts`) lets the app read `world.pmtiles` from `app/public/data/world/`; the default still reads only `pilot`. `rail-extract places` now writes `world/places.json` and `world/search.json` with the world build, so city and country names exist; the region picker still lists the pilot regions only and the search bar is not built.
  - First step (places done): build the search bar on `search.json` (countries, provinces / states, and cities that have a metro system, one per metro area within 20 km, found within 12 km of open metro or light rail track; no towns: see `docs/10-world-build.md`, Places and search), then decide how the app offers the world (picker, region list for 146 countries, default view) and stop treating the pilot regions as special.
  - Done when: the world source shows city and country names at the same zooms as the pilot, every country is reachable from the picker or by search, and `npm run build` plus a headless render of z3, Cairo z9 and Tokyo z9 pass.

- [ ] **Glyph coverage for other scripts**
  - Symptom: Arabic labels (Cairo) come out as disconnected fragments (no shaping; the glyph step used the OpenFreeMap server's Noto Sans for these code points). Cyrillic (Moscow) and CJK (Tokyo) draw. Devanagari, Thai, Hebrew, Greek, Armenian, Georgian, Ethiopic and others were not checked.
  - First step: render one city per script from `world.pmtiles` and list the scripts that draw blank or broken; rebuild `pipeline/glyphs` over the world file's labels (it reads `pilot.pmtiles` and `americas.pmtiles` today).
  - Done when: a screenshot per script with legible labels, or a documented list of scripts that are not supported. Arabic needs shaping (MapLibre's right-to-left text plugin or a different approach); decide whether it is in scope.

- [ ] **US, Canada, Mexico freight drawn as passenger with generic configs**
  - Symptom: with the generic rules (no freight name pattern) freight track in the Americas is typed as passenger types. `07-freight-detection.md` (the ordered rules, known failures) and the Americas row in `00-overview.md` (rules reverted 2026-10-15) cover it.
  - First step: decide per `00-overview.md` whether the Americas freight rules come back; the world build uses the generic template `pipeline/config/_default.json` for every country without its own config.
  - Done when: a spot check of the US Midwest and Mexico (answer lists as in "Automatic data check", `05-pilot-plan.md`) shows freight as freight, with the pilot countries unchanged.

- [ ] **Cloudflare R2 deployment reminders** (nothing deployed yet; `r2.dev` is for the pilot only)
  - HTTP/2 (or 3) on the tile host: PMTiles makes many small range requests.
  - Range requests are billed per operation: keep the deduped fetcher (`dedupedPmtiles` in `MapView.tsx`) and do not add a second source on the same file without it.
  - Cache headers: `Cache-Control` on `/data/*` (tiles are immutable per build: put the build date in the file name, or purge after a rebuild) and immutable on `/glyphs/*`.
  - Compression: `.pbf` glyph files are not compressed by `r2.dev` or `vite preview` (gzip saves 32%); tiles are already gzip inside the PMTiles.
  - CORS: `GET`, `HEAD` and the `Range` header allowed, and `Content-Range` exposed (`app/.env.example`).
  - Custom domain: `r2.dev` has no edge cache and is rate limited; use a custom domain through Cloudflare for caching.
  - Chunked tiles option: the world file is 1.15 GB; every cold range is a cache miss at the edge. If misses hurt, split by zoom (an overview file plus a detail file) or serve one object per tile. Decide after measuring on the real host.
  - Done when: a cold load of Paris and Chengdu from the real host matches the localhost request counts (14 and 11 `pilot.pmtiles` requests), second loads hit the cache, and `curl -I` on a glyph file shows `immutable` and `content-encoding`.

- [ ] **Tile-build memory and a streaming reader**
  - Measured: pilot writer 5 to 14 GB (the per-region slicing holds a whole region; build with seven slugs peaks at 14.1 GB), world `rail-extract world` 10.5 GB peak RSS (7.9 GB footprint) in 55 to 59 s; `build --no-tiles` for 146 countries 12 to 18 GB peak, 37 s. The world writer already partitions by tile (`--deep-zoom`, default 6; 5 and 6 are byte-identical).
  - First step: measure the writer on a smaller machine (16 GB) and lower `--deep-zoom` partition size or thread count; the longer fix is a streaming reader (the index of 8.5 million features is about 4 GB, mostly mapped files) and a streaming station model (`build` holds a whole region).
  - Done when: the world builds on a 16 GB machine without swapping, with `world.pmtiles` unchanged byte for byte.

- [x] **Pack the per-feature side files into per-region shards** (done 2026-10-16: 307,522 files became 11,185; results below the list)
  - Symptom: `app/public/data` is 307,522 files (222,772 under `world/`); 269,148 are under 4 KB. Upload to R2 runs at about 220 KiB/s for 1.2 GiB because each file is one HTTPS request (first upload 2026-10-16: 45,106 files, 11+ min for the first 1.1 GiB); every sync lists and checks them all. R2 bills per operation, and a deploy or a re-sync pays it again.
  - Where the files come from (counts in `app/public/data`, 2026-10-16): `{slug}/stations/{id}.json` 184,829 (one per station); `{slug}/tags/{n}.json` 50,944 and `{slug}/name-hl/{n}.json` 50,944 (already 256 hash shards per region, so a small region still gets up to 256 mostly empty files); `{slug}/lines/{id}.json` 19,566 (one per line). Written by `pipeline/rail-extract/src/tiles.rs` (`NAME_SHARDS = 256`) and the station side-file code; read by `app/src/data.ts` (`loadStation`, `loadLine`, `loadLineHighlight`, `loadNameHighlight`, `loadFeatureTags`).
  - Design (per region slug, same as today's folders): hash-shard stations and lines the way `tags` and `name-hl` already are, but pick the shard count per region from its record count so a shard is about 50 to 200 KB (a few shards for Luxembourg, a few hundred for China). The shard count goes in a small per-region `manifest.json`, so the app computes `fnv1a(id) % n` and fetches one shard on click. Same fix for `tags` and `name-hl`: stop writing 256 shards for regions that need 4. Fallback if shards get too big: one pack file per region plus an offset index, read with `Range` (PMTiles-style); more code, so only if shards do not hold up.
  - Target: under 600 files per region, under 25,000 in the whole world output (from 307,522), the biggest shard under 250 KB gzipped.
  - Constraints: ids must not change (`osm_id` stays the key inside each shard, `00-overview.md` and `CLAUDE.md`); keep the in-memory `get` cache in `data.ts` so a second click in the same shard costs nothing; the files are offered under ODbL, so document the new layout in `03-data-spec.md` and `02-architecture.md`; shard files stay gzip-friendly JSON (Cloudflare compresses them on a custom domain; `r2.dev` does not).
  - First step: measure the size distribution of `stations/*.json` and `lines/*.json` in two regions (Paris, China) to choose the shard sizing rule; then change the writer for stations and lines, then the app readers, then the shard counts for `tags` and `name-hl`.
  - Done when: `find app/public/data -type f | wc -l` is under 25,000 with the world output; clicking a station, a line and a named railway in Paris, Chengdu and the Americas source shows the same card and highlight as before (spot check against the old output); click-to-card latency not worse (one request, under 250 KB); the automatic data check in `05-pilot-plan.md` passes; a full `r2/sync.sh` into an empty bucket finishes in under 5 minutes.
  - Result (2026-10-16): `tiles.rs` writes shards (`shard_count`, `write_shards`) and `shards.json`; `app/src/data.ts` reads them (`fromShard`); `pipeline/repack_side_files.mjs` converted the existing output in 31 s, without a rebuild, so the Americas files (their configs are not committed) were not regenerated. 307,522 files to 11,185 (world 222,772 to about 9,000; 199 regions). Largest shard 470 KB raw (`world/ru/name-hl`), 9 station, 72 line and 970 tag shards over 256 KB, none over 1 MB; 221 lines stay their own file in `lines-big/` (over 64 KB; one is 19 MB). Checked record by record against the old output for Italy, Switzerland and Paris (stations, lines, tags, named-line highlights): 0 differences; the repack tool's output for Italy has the same shard counts and sizes as the Rust writer. The only Paris difference was 3 station files from earlier builds that the old writer never deleted. Clicked a station and a line card in the app (Greater Bay Area, dev server): both load, highlight draws, no console errors. `pipeline/check.py` has one failure, `^成都$` is tier 3 (want 1), and it fails the same with the old code.
  - Not measured yet: gzipped shard sizes and click latency on the real host; full `r2/sync.sh` into an empty bucket. Named-railway highlight and the Inspect tags panel were checked by data equality and share the station and line read path, but not clicked.

## P2

- [ ] **Highlight filter cost on very long lines, in the workers.** `setFilter` on `pilot:hl-fs` (its own source `pilot:hls`, minzoom 9) with a long list of way ids (a whole line crossing a country) costs worker time on every reload. Not traced yet. First step: highlight the longest line (a Trans-Siberian or China HSR route) and trace the worker once (Chrome trace, split by category). Done when: the worker time per highlight is known and either under 20 ms or reduced (for example by sending a Set instead of a literal list).
- [ ] **Chengdu settle time at 150 ms latency:** 6,462 ms against 6,066 ms for the original (6% slower, one run each, glyph files included; SF 10% faster). Repeat on an idle machine (load average was 5), 3 runs ABAB. Done when: the median is within noise of the original or the cause is found.
- [ ] **Zoom out to the world (z2):** a tie with 120 to 170% run-to-run spread. Repeat on an idle machine (`--zoomout`), now against the world file as well (`VITE_TILE_SOURCE=world`).
- [ ] **Headed hover and click sweeps** were one run per app. Repeat on an idle machine (`hv.mjs` idea in the playbook: instrument the handler, sweep rail at z3 to z12, count frames over 25 ms).
- [ ] **First-zoom hitch repeats.** The 90 ms hitch at the first zoom was the CJK canvas drawing and went away with self-hosted glyphs; check it has not come back after any app change (quick bench, first view, max frame under 20 ms).

## Reminders (not tasks)

- **Dev server is slower than preview.** `vite dev` is not a performance reference; bench `vite preview` on a production build. `npm run build` copies `app/public/data/` into `app/dist/` (several GB with the world files): delete or move the world folder before building for deployment, or point `VITE_DATA_BASE` at the host.
- **`omc ralph verify` noise:** it counts passing test lines as new failures when the baseline had no tests. Read the real test output.
- **Method** (details in `09-performance-playbook.md`, "Method lessons"): keep a baseline worktree on its own port and never edit it; alternate A/B/A/B; headed Chrome on the 120 Hz display (budget 8.3 ms; headless 60 Hz hides overruns); CPU throttle 4; wheel delta 127; latency 50 and 150 ms with 50 Mbps; one warm-up; note the load average and repeat when repeats differ by more than 25%; quick preset (2 hardest views, 2 runs) to screen, full five views (3 runs) to confirm; check equal work (features, vertices, columns) before blaming a format.

## Still open in `06-open-questions.md` (by reference)

- Table "Still open": China high-speed rule beyond Chengdu.
- "Must fix: performance dropped after the themes, labels and Europe": measure headed on the user's display and A/B each change.
- "Must fix: Europe low-zoom tile weight": superseded by the P0 item above, which covers the world.
- "Must fix: glyph download size": the P0 item above.
- "To do later": tile-build memory (the P1 item above).
- "Smooth rendering and zoom: to do", "Highlight performance: what was tried, what is left", "Faster HSR only toggle: research areas": unfinished research lists.
- Parked (not in v1): GTFS, live data, history timeline, custom areas, routing, signalling themes.
