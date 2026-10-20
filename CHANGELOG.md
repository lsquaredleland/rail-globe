# Changelog

Newest first. Decisions are in `docs/00-overview.md`; open items in `docs/17-action-items.md`.

## 2026-10-20: cleanup (docs, archive, one to-do home)

- `docs/00-overview.md`: a short "Current rules" section on top, the decision log newest first below it; 14 superseded rows moved verbatim to `docs/archive/decisions-superseded.md`.
- Stale facts fixed in the docs (glyph steps, Python and tippecanoe processing, `pilot.pmtiles`, `--combined`, the `tiles` command, search shards).
- `pipeline/README.md` is a command reference now; the pilot-era timing log is `docs/reference/pipeline-timings.md`.
- One to-do home, `docs/17-action-items.md`: open items of `06-open-questions.md` and `docs/archive/performance-todos-2026-10-17.md` moved there; closed material is in `docs/archive/`.
- Dead scripts, raw Nominatim dumps and the low-zoom scratch tools moved to `pipeline/archive/` (see its README).
- `docs/05-pilot-plan.md` has a Status column; phase 4 (Compare) was skipped.
- `docs/18-changelog-2026-10-17.md` became this file.

## 2026-10-16 to 2026-10-17: QA, fixes and tile weight

What happened in this session, in order of importance, with what was verified and what was not. Nothing here is deployed. Only two commits were made (the hover fix). Everything else is uncommitted in the working tree and waits for approval (`docs/17-action-items.md`).

Who did the work: Sonnet 5.5 agents wrote code, configs and docs in parallel. Opus 5.5 agents reviewed (three review rounds). No agent committed except the hover agent, and only with the user's earlier instruction.

### 1. Committed

| Commit | What |
| --- | --- |
| `6fe306f` | Hover lag after a pan. `prefetchHighlights` asked a point-in-outline test (`regionOf`) for every rendered track piece after every map idle: 1.8 s of a 4 s idle at Chengdu z10 (throttle 4). Now once per line key; two jobs at a time; queue dropped when the map moves. Longest long task after a jump: Chengdu 2,054 to 196 ms, Paris 593 to 81 ms |
| `ce2661e` | Fix to that fix, found by the Opus review: the dedupe key had no region (line 5 of country B skipped after line 5 of country A in border views); a prefetch cancelled by a map move was never retried. Region now resolved once per ~5 km cell; keys are `slug|ln`; a generation counter stops old runners. Basel z10 prefetch 1,663 to 233 ms (de, fr, ch all queued) |

Not deployed: the live site is older than both.

### 2. Pipeline code (uncommitted)

#### Stations (`stations/`)
- Merge by `wikidata` / `uic_ref` only within 1 km (3 km when the normalised names are equal); `railway:ref` is no longer a merge key. Poznań, Wrocław, Kraków, Saratov and Chelyabinsk are separate and correctly named again (about 600 Polish stations had been absorbed).
- A way that is both `building=train_station` and a station is now a station (456 stations had been lost in 21 countries: Boston South/North, Rawalpindi, Kandy, Nairobi Central, Varanasi, València Nord…). A polygon with no stop area joins the nearest same-mode station within 100 m when names match or it is unnamed.
- Mode comes from the station's own platforms when the guess came from the nearest track (Düsseldorf Hbf and Basel SBB were subway / light-rail only).
- Labels: own name wins over the stop-area name; `Bf.`/`Bhf.` prefix, `, Bahnhof`, `City, U/S` form, Korean 기차역 stripped; trailing `(…)` stripped only when it names a line, mode or operator and the bare name is a station within 300 m (so `(Beverly Hills)`, `(Cheshire)`, `(Mark)` survive). `english_name` uses the shared rule (below).
- Platform sections `1a`/`1b` count as one platform; 80 m mainline fallback only when no platform has heavy-rail or heritage track within 20 m (Nanjing South is tier 1 again).

#### Classification (`classify.rs`, `rules.rs`, `cut.rs`)
- Narrow gauge is typed from route relation, then `usage`, like standard gauge; with no evidence it is Other (not Freight). Seven narrow-gauge exemptions became redundant and were removed.
- `english_name` accepts only `name:en`, `int_name` and romanisation keys. No more Wikidata ids ("Q735271") or wrong-language names ("Warschau Westbahnhof", "Glaschu Mheadhain").
- High-speed: a route is High-speed only when at least half of its length is high-speed track (construction and proposed ways excluded); with some high-speed track on a route of 150 km or more it is Intercity; otherwise Regional. `service=highspeed` and `high-speed` count as high-speed (KTX relations were being missed).
- `service=international;long_distance` (multi-value) is parsed; routes named "(en construcción)", "under construction", "proyectado" and similar are dropped from service.

#### Config defaults (`site.rs`)
- Own-file configs that `extends` a template now get `stat_name` / `stat_local` / `stat_auto.country` filled when they omit them. Without this, 23 new configs would have lost their area cards and outlines on the next build (card read "au", provinces and cities dropped). Test added; checked on au, kz, in, jp against the live `stats.json`.

#### Tile writer (`tilewriter/`, docs/16 steps 1 to 3)
- At z0 to z8: repeated vertices dropped, Douglas-Peucker 4 units after the join (own segment-distance implementation), extent 2048 (airports stay 4096), and no stations or complexes at z0 to z5. `ln`, `nk`, `oc` and all other columns unchanged.
- Subset (14 countries): z0 to z8 10.09 to 4.62 MB (-54%); Europe z5 laptop view 1,264 to 515 KB; z9+ tile content identical (247,591 tiles hashed); `--deep-zoom 5` and `6` identical; run time unchanged. `RAIL_LOWZOOM=off` restores the old path exactly (`cmp` identical). `vector_layers` minzoom for stations/complexes is 6 when on. 11 writer tests.
- Not measured: the full world file. My estimate for Europe z5 on the full world is about 650 KB, so z5 and z6 laptop views may still be near 600 KB. Steps 4 to 7 of docs/16 are not built (the user is thinking about them).

#### Checker (`pipeline/check.py`)
- `_known_failure` is honoured (KNOWN, and NOW PASSING when it starts passing). `--exemptions` fails on an exemption id missing from the report. Entries are tagged forced vs independent. `--data DIR` works for answer lists.

Tests: `cargo test --release` 52 passed (last full run by the regeneration agent).

### 3. Configs and answer lists (uncommitted)

- New per-country configs: ie gb es at be nl pl ua ru uz kz mn (restored after being lost in a collision between two agents) au nz id kr za ke et br cl co uy ve do eg ng bd pk lk in ir. Edited: de it ch cn th tw jp us mx.
- Routes mode (freight vs passenger evidence) applied to Brazil, Chile, Colombia, Uruguay, Venezuela, the Dominican Republic. Passenger-typed way-km: Brazil 14,044 to 4,001, Chile 3,701 to 1,143, Colombia 1,561 to 788, Uruguay 1,391 to 118, Venezuela 380 to 38, Dominican Republic 238 to 0. Uruguay: no running suburban service could be found, so nothing was preserved beyond the AFE Tacuarembó–Rivera route (118 km). Peru, Bolivia, Cuba not applied.
- China: `main_station_re` extended then narrowed (tier 1: 103 originally, 200 in the first version, 144 now); 朔黄线 is Freight (one commuter round trip Mon/Wed/Fri on the east end is noted, user decision pending). Korea: `kr.json` with `^KTX( |$)` keeps both KTX Honam routes High-speed.
- Other fixes by config: Australia and New Zealand freight (12,348 km of Pilbara iron-ore/coal/grain no longer Intercity), Kazakhstan long-distance trains, Mongolia/Poland/Ukraine/Russia/Uzbekistan freight bypasses and coal lines, Ireland Bord na Móna, Austrian narrow gauge, Gautrain, NEC commuter lines (US, kept Regional), Eurostar/TGV High-speed in GB/BE/NL, many `main_terminals` and name patterns for tier-1 stations, Bangkok Airport Rail Link.
- 19 answer lists in `pipeline/answers/`: 123 independent + 31 forced entries, 29 KNOWN failures. Forced entries mostly restate overrides and cannot catch a regression in the size score; the KNOWN list is the agents' judgement from Wikipedia rankings and the score, not confirmed OSM bugs.

### 4. App (uncommitted, one file comment changed by me)

- Search bar: IME composition no longer hijacks Enter/arrows; combobox ARIA; failed `search.json` load shows an error and retries; "Loading places…" only while loading; focus returns to the button; scroll-into-view on arrow keys only; `featured` memoised.
- Legend: a World card (colour key, type switches, freight toggle, no km) shows when no area fills the screen; Speed card no longer prints "none" for every band when zoomed out; phone sheet in operator mode shows the operator note plus switches; "No area here" above z5 over sea.
- Country card fix: on a cold load or deep link France z6.3, Netherlands z7 and Italy z6 showed the wrong card (HEAD showed the wrong province). Cause: a stale memo that depended on `partAt`, which returns an empty string until `world.parts.json` and the overlap outlines have loaded. Fixed with `onPartsChange` in `regions.ts` and a `partsVer` dependency in `App.tsx`.
- The user's own uncommitted `App.tsx` edits (operator Legend and `legendOpen` removed, `cardArea = viewArea`, `areasAt` removed) were preserved.
- `app/src/config.ts` comment about tier-1 stations from z6 (comment only).

### 5. Data (local only, nothing published)

- `data/world` and the three pilot folders were regenerated with the final code (cut 9 s, build 41 s, no panics) and installed after the checks passed; before that eight slugs (cn th au nz mn jp tw id) were in a mixed state from QA agents re-cutting individually. Two APFS-clone backups exist: `world-backup-20261017` (pre-install) and `world-backup-20261017b`, under `/private/tmp/claude-501/-Users-leland-Downloads-osm-rail/7484283d-56e2-420c-bc1d-8c4f79574406/`.
- `app/public`, `world.pmtiles`, glyphs and `r2/` were never touched. The live map shows none of the above until a world build and publish.
- Headline effects against the Oct 16 build: Poland stations 2,673 to 3,281 (tier 1 12 to 6), Kazakhstan Intercity 5,171 to 13,314 km, GB high-speed lines 21 to 9, Turkey 30 to 23, Serbia 21 to 8, US 6 to 1, Korea 24 to 21, China tier 1 98 to 141 and one new area (`cn-jiading`).

### 6. QA coverage

Four regional passes (west Europe, east Europe, east Asia and Oceania, Americas/Africa/South Asia/Middle East): about 180 country and city screenshots, 83 defects logged in `docs/qa/*.md`, most with cause, evidence and status. The screenshots are not in the repo. Gaps: Iceland and the micro-states, operator and speed colour modes in the Americas and Africa, hover and click, and rebuilt tile content. The Opus review of the final tree checks a further sample of labels and route classes.

### 7. Docs

Updated for the R2/Vercel deployment (00, 02, 05, 06, 10, 11, `CLAUDE.md`, README, `r2/README.md`); 14 checklist items ticked in 03 after checking the code; new: 16 (tile weight plan with section 9 results), 17 (action items), 18 (this file), `docs/qa/*`; 09 has the hover-lag investigation.

### 8. Known open (details in docs/17)

Decisions: Americas Peru/Bolivia/Cuba, ICE/Frecciarossa/Railjet and S-Bahn typing, Ghan vs Indian Pacific, 朔黄线, tile-weight step 4, glyph rebuild (versioned folder or purge), desktop operator mode card, 150 km cut-off. Work: station label noise and duplicate dots (A19), Macdonaldtown, bare "Hauptbahnhof", Agra Cantonment, glyph set (Seoul `서시`, Greek and Armenian labels, Vietnamese, Thai), station labels hidden by place labels at z9 to z10, custom domain and compression for shards (fast sweeps over many lines still fetch 4.5 to 5 MB of shards), stale `app/public/data/world/{lr,ne,ss,sz}` that `r2/sync.sh` would upload.

### 8b. Final Opus review (end of session)

GO for committing the code, configs, app and docs. `cargo test --release` 52 passed; world answer lists and `--exemptions` ALL PASS; `tsc` clean; configs valid JSON. Applied afterwards: Chengdu `^成都$` marked a known pilot failure (it failed before this session too; the pilot check is now ALL PASS), the London Euston answer pattern fixed, docs/17 refreshed. Found and left for the user (docs/17 A21): narrow-gauge `usage=main|branch` creates passenger track in Jordan (443 km) and Fiji (161 km); 208 Chinese stations under construction drawn as open; long French TER routes become Intercity under the 150 km rule; a few conventional trains promoted to High-speed (Toros Ekspresi, Serbian REx Petrovaradin); `Hp. ` prefixes and a few label bugs. The two `data/world` backups disappeared from /private/tmp during the session, so there is no rollback copy (regeneration takes about a minute).

### 8c. Second round (user comments on docs/17), end of session

- Decisions applied: both Australian long-distance trains Intercity; world build is the default (CLAUDE.md, docs/00-overview.md); glyphs: standard practice (CJK drawn locally, `glyphs-v2` without the 4,211 locally drawn code points, Arabic presentation forms added; `maplibre-gl` pinned at 6.14.0); stale `lr ne ss sz` folders deleted and `r2/dropped-slugs.sh` prune guard added to `sync.sh` and `publish.sh`.
- New code: construction tags on open-looking track and stations are read as construction (rail/narrow gauge only with a rail-kind value, no `ref`, and an operator or `highspeed=yes`, and not on a running route; monorail, light rail, subway and tram close on `yes` unless on a running route); stations tagged with `construction:*` are kept out of the open model unless they carry an open tag or belong to a route; label cleaning (semicolon lists, `[...]`, `（在建）`, `Hp. `, placeholder English names), `Hauptbahnhof` qualified by city, 70 duplicate-dot groups merged; `car_shuttle` is Other.
- New configs: jo fj ae pe sg kr; edited: au nz cn fr ke eg pl ru. Station label layers copied above the place labels from z9 (tier 1 and 2 names no longer hidden by district labels); search keeps the City row and drops the same-named Province row when they are about the same size.
- Final world build (local, nothing published): all stages exit 0; `world.pmtiles` 1,159.4 MB, z0 to z8 15.9 MB gzip (31.3 MB before). Only Europe z5 on the laptop is over the 600 KB budget (678 KB). The first world view loads 21 glyph files (409 KB gzipped).
- Opus final reviews found and the team fixed: search twin rule hiding big states, Bönningstedt dropped as under construction, running-route check for every mode, prune guard defeated by its own age check. A second look found that skipping `construction:*` stations drops some open ones; the rule was tightened (Montpellier line 5, Crumpsall, Arcueil-Cachan are open again).
- Open: see docs/17 (Europe z5 678 KB, small-script local fonts, Turkey station tiers fell back and are not traced, construction predicate shared by `stations/` and `construction.rs`).

### 9. Things that went wrong or were not verified

- Two agents collided on Mongolia: the East Asia agent's re-cut and cleanup removed `mn.json`; it was rewritten from the first agent's report.
- The first `data/world` regeneration was refused install (two non-known failures); fixed by deleting a stale Chile exemption and correcting an Australian answer pattern, then re-run.
- My early suggestion for the high-speed rule ("keep Intercity when `service` says long-distance") would have changed nothing; the Opus review caught it and the 150 km rule replaced it.
- The hover investigation first found no regression (it hovered after the prefetch had finished); the second pass reproduced it by sweeping right after a jump. No headed 120 Hz run was possible (machine load 20 to 40).
- A harness flag on one reviewer report ("settings-json") was only a note that `.claude/settings.json` should not be committed; nothing in Claude's settings was changed.
- Every number in this file comes from agent reports; the ones I checked myself: commit contents of `6fe306f` and `ce2661e`, the `con_draw` files being ignored by the tile writer, and the `FIXHASH` placeholder being gone.

### 10. Glyph-free app (user decision, end of day)

The app no longer has any glyph files: the style has no `glyphs` property and every `text-font` is a system-font stack (`Noto Sans Bold` or `Regular`, `Noto Sans`, `Helvetica Neue`, `Arial`, `sans-serif`), so MapLibre draws all text locally. Removed: `GLYPHS_URL`, the startup glyph check in `main.tsx`, `localIdeographFontFamily` (MapLibre only reads it for its default stack), `pipeline/glyphs/` (tracked files removed with `git rm`), the `glyphs` stage and `--glyphs` flag of `pipeline/publish.sh`, the glyph upload in `r2/sync.sh`, the glyph lines of `app/.vercelignore`. Why: the world build made the CJK subset 1.3 to 1.8 MB per dense view and the first world view cost 409 KB gzipped of glyph files; the benchmark showed no visible difference in Latin, Hangul or Arabic. Cost: the first-zoom hitch roughly doubles (Chengdu East max frame 75 to 125 ms, one noisy pair), and typefaces and label collisions differ by OS. `app/public/glyphs*` on disk and the glyph folders in R2 were not touched; the next `r2/sync.sh` in sync mode deletes the R2 ones. Checked: `tsc --noEmit` and `npm run build` pass, headless Chrome shots of Paris z11, Seoul z11, Moscow z10, Cairo z9 and world z2 with an empty console. This overrides the earlier "glyphs" lines in sections 8 and 8c.
