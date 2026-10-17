# Action items for review (2026-10-17)

How to use: under each item, replace `Your call:` with `approve`, `reject`, or your comment. Items marked **RECOMMENDED** are what I would do. Nothing in sections B to D runs past the point it says without your answer.

Sources: `docs/qa/*.md` (four QA reports), `docs/16-low-zoom-tile-weight-plan.md`, `docs/11-performance-todos.md`, and the Opus reviews of this session.

## A. Decisions only you can make

### A1. 朔黄线 (Shuohuang) in China: freight or partly passenger?
It is a coal railway with no 12306 stations. Chinese Wikipedia says one commuter round trip runs Mon, Wed and Fri since 2019-10-09 between Cangzhou West and Huanghuagang (the short eastern end). `cn.json` now types the whole line Freight.
- Options: (a) keep all freight, **RECOMMENDED** (one round trip three days a week is not a service people plan around); (b) leave the east end passenger.
- Your call:

### A2. The Ghan: Other or Intercity?
It is a luxury tourist train (Journey Beyond), so `docs/01-features.md` puts it in Other, and the answer list now expects Other. The Indian Pacific, same operator, is typed Intercity.
- Options: (a) both Other, **RECOMMENDED** (consistent with the tourist rule); (b) both Intercity (they are the only long-distance rail across Australia); (c) leave as is.
- Your call:

### A3. Low-zoom tile weight, step 4 (steps 1 to 3 are built, uncommitted, measured on a 14-country subset only)
After steps 1 to 3, every phone view passes and only Europe z5 and z6 (about 540 to 650 KB estimated for the full world; not measured until a world build) may stay near or over 600 KB.
- 4a: cut `ln` to its first id below z9. Europe z5 becomes 562 KB. Cost: the tooltip lists one service below z9, which conflicts with the 2026-10-16 decision in `00-overview.md`.
- 4b: regional track starts at z6. Europe z5 becomes 364 KB, but z6 (563 KB) becomes the worst view, and the z5 regional draw changes.
- 4c: accept 671 KB at one view and stop after step 3.
- My lean: 4c first (see how it feels), then 4b if Europe z5 still bothers you. 4a changes a decision you made on purpose.
- Steps 5 to 7 (per-tile cap, corridor merge, clip buffer): not needed unless 4 is rejected. Your call:

### A4. Glyph rebuild: versioned folder or purge?
`app/public/glyphs` covers only the pilots, so labels lose letters (Seoul draws `서시`, Greek and Armenian are 98% broken, Russian 14%, Vietnamese and Thai broken). Fix: rerun `pipeline/glyphs/build-glyphs.mjs` over `world.pmtiles` labels. `sync.sh` serves glyphs with a one-year immutable cache, so the new set must go under a new path or the old URLs must be purged.
- Options: (a) versioned folder `/glyphs-v2/` plus a one-line app change, **RECOMMENDED** (no purge, instant rollback); (b) overwrite and purge.
- Also decide: one CJK weight (redirect Bold to Regular, halves the bytes) in the same pass? **RECOMMENDED** yes, it is the first step in `docs/11` and costs no rebuild.
- Your call:

### A5. Name overrides for ICE, Frecciarossa, Railjet
These still show as Intercity or Regional. A name override changes the track too, and cut Germany's Intercity track by 7.7% in the test, so I reverted it. Options: (a) leave, **RECOMMENDED** until there is a way to override the service without the track; (b) apply the override and accept the track change.
- Your call:

### A6. Berlin and Hamburg S-Bahn and Copenhagen S-tog: Light rail or Regional?
They are Light rail while every other S-Bahn is Regional (`docs/qa/west-europe.md` S9). Option (a): make them Regional, **RECOMMENDED** (consistent, and `07`/`01` say S-Bahn is Regional); (b) leave.
- Your call:

### A7. Uruguay suburban service
The agent found no running suburban service (Montevideo–Progreso is expected "in the coming years"). Only the Tacuarembó–Rivera line (118 km) stays passenger. If you know of a service that really runs, give me a source and I will add an exemption on the 48 km of unnamed `operator=AFE` ways.
- Your call:

### A8. Peru, Bolivia, Cuba routes mode
Not applied (Peru 29% dropped, Bolivia 25%, Cuba 15%, mostly route-mapped, and Bolivia would lose real ways). **RECOMMENDED:** leave them, look per country later.
- Your call:

### A9. China tier-1 station override (kept, narrowed)
The narrowed `main_station_re` gives 144 tier-1 stations (103 before this QA, 200 in the first version). The Opus review ruled the mechanism allowed (`00-overview.md` 2026-10-11, decision 1). 南京南 stays tier 3 because of the `no_passenger_track` rule, which needs a code change (being looked at by the station fixer, report-only unless a clear size-based fix exists).
- Your call (keep the narrowed version?):

### A10. Station labels hidden by place labels at z9 to z10
Shanghai Railway Station, Shenzhen Luohu, Futian, West Kowloon and KL Sentral show the tier-1 dot but not the name. Needs a collision or priority change in `mapStyle.ts`. **RECOMMENDED:** raise station label priority over district and township place labels at z9 to z10 only. Do you want this done now?
- Your call:

### A11. Small items I will not decide silently
For each, say fix, ignore, or park. I would park all of them.
- Kenya's old Nairobi–Mombasa relation (495 km, probably stale)
- Peru tourist lines
- Etihad Rail freight line typed high-speed (check after the high-speed code fix)
- Cairo monorail in-service status
- Singapore LRT drawn as monorail (OSM tags it that way; about 38 km)
- PNR South Main Line Philippines (207 km Intercity, service unverified)
- Perth and Wellington suburban lines mixed Intercity/Regional
- Poland Magistrala Węglowa and Podsudecka, Gazprom Obskaya–Bovanenkovo, Uzbekistan Miskin–Bukhara high-speed section
- Sweden route km looks low (8,990 against about 14,300), not investigated
- St Petersburg metro 33% over its real length (not fixable here)
- Your call:

### A12. Search bar loose ends
Compare view is disabled (phase 4), so "results go to the focused map" is not built. Tokyo, Cairo and Moscow list a City and a Province row. No station or line search. Which of these do you want?
- Your call:

### A13. Hosting
r2.dev is HTTP/1.1 with no `Content-Encoding` on glyphs and shards, tiles `max-age=3600` (not immutable), and `shards.json` shows `max-age=3600` where `sync.sh` meant `no-cache`. Fix is a custom domain behind Cloudflare. Do you want me to prepare the steps, and do you want `shards.json` header checked now?
- Your call:

### A14. Policy wording
`CLAUDE.md` and `README.md` still say "never run a world build unless asked", while the app now reads only the world build. Keep the rule (it is about long jobs, not about the app), or reword?
- Your call:

### A15. Country card does not show on country-wide views
**Fixed (2026-10-17), needs your review.** On a cold load or deep link, France z6.3, the Netherlands z7 and Italy z6 showed the World card instead of the country card. Cause: a stale memo in `overRegions` (`App.tsx`) that filters on `partAt(center)`, which returns `''` until `world.parts.json` and the overlap outlines have loaded, and it was never re-run. It is not your `cardArea = viewArea` edit, and HEAD has the same bug: HEAD's `areasAt` fallback showed the wrong province on a cold load (Centre-Val de Loire for France, Utrecht for the Netherlands, Umbria for Italy). Fix: `onPartsChange` in `regions.ts` and a `partsVer` dependency in `App.tsx`. Cold loads now show "Rail in France", "Rail in Netherlands", "Rail in Italy", and the World card only when zoomed out.
Follow-ups from the Opus review, now applied: the zoomed-out Speed card no longer prints "none" for every band, the phone sheet in operator mode now shows the operator note plus switches (not mismatched rail-type swatches), the World card says "No area here / Move the centre over land" above z5, plus search a11y and retry fixes. Two things left for you:
- Desktop operator mode zoomed in over a country shows no card and no legend (you removed the operator Legend), while zoomed out it shows the World card. Keep that, or show the card/legend in operator mode at every zoom?
- Over open sea at z7 the region button still names the nearest country ("Portugal Country") because `inView` falls back to the nearest item. Leave, or show nothing?
- Your call (approve the fix to be committed with the app changes?):

### A16. Stale country folders on disk
`app/public/data/world/{lr,ne,ss,sz}` are listed in `data/world/dropped.txt` but still exist, and `r2/sync.sh` would upload them (it syncs the whole `app/public/data` tree). **RECOMMENDED:** delete the four local folders (they are build output, regenerable) and add a prune to `publish.sh`. I have not deleted anything.
- Your call:

### A17. Station-model leftovers (after the `stations.rs` fixes)
Not fixed, and why. Say fix, park, or ignore for each.
- Bare "Hauptbahnhof" names (Hanover, Chemnitz, Bremen, Marburg) are not qualified: no place name is available and the agent would not invent one. Option: qualify with the city from the place hierarchy. **RECOMMENDED** if cheap.
- "St Pancras Low Level station" and Kyoto's 近鉄京都 name are unchanged.
- Sydney Macdonaldtown stays tier 1: its 19 "platforms" are 17 ref-less depot ways plus 2 reffed ones; collapsing touching ref-less ways also collapses Wien Hbf (12 to 3), so it was reverted.
- Agra Cantonment is still missing (tagged `railway=station` plus `public_transport=platform`, an OSM tagging problem; 6 India station features unmodelled).
- Venezia Santa Lucia went from tier 2 to tier 3 (score 24.1) and Arnhem Centraal from 50.0 to 44.0, both from the `1N` platform-ref collapse. Both are on the known-failure list. Is that acceptable, or do you want the `1N` rule limited?
- China tier 1 went from 61 to 141 stations because the nationwide merge had absorbed forced major stations and others had lost their train mode.
- Your call:

### A18. Classification side effects (after the `classify.rs` fixes)
Conventional trains that only touch a high-speed link fell to Regional: GB 14 lines (GWR Penzance, CrossCountry, Transpennine), Turkish expresses (Doğu Ekspresi), Serbian regional trains, Etihad Rail, Tanger–Fes, and Korea's KTX Honam (43% of 317 km on high-speed track, `service=highspeed`). The Opus review showed my first idea (keep Intercity when `service` says long-distance) would change nothing, since `service` is already checked first and these GB routes have no `service` tag. Replacement rule, implemented (see also A21 for what the final review found with it): share of high-speed track >= 0.5 gives High-speed; otherwise some high-speed track and route length in the region >= 150 km gives Intercity; otherwise Regional. A route with `service=highspeed` must stay High-speed. Expected: GWR, TPE, CrossCountry long routes and the Turkish expresses become Intercity; Merseyrail, Barrow–Lancaster, Nottingham–Birmingham (about 92 km) and Serbia's REx become Regional. China: 9 lines change, none in Chengdu.
- Your call (is 150 km the right cut-off for "long-distance"?):

### A19. Station label noise and duplicate dots found after the final regeneration
Sample of 15 stations from each of jp, fr, de, it, us, gb, cn, pl plus a scan of all their stations. No Q-ids and no wrong-language English names remain. Still there:
- Semicolon lists in names (`黄福柵原駅;黄福柵原`, `Bain Hollow;Pieter Pass`, `Meridian Union Station;Meridian`).
- Tag or date noise: `Dudley Castle & Zoo [Aug 2026]`, `Męcina [Nowa]`, `龙吴路（在建）` (under construction), double spaces, a bare `Station`.
- Placeholder English names: `待定` ("to be determined") on two Chinese stations; 10 Japanese `name_en` values holding non-Latin characters.
- Nameless stations: jp 54, fr 33, de 33, it 20, us 110, gb 45, cn 132. 7 bare "Hauptbahnhof" in Germany (A17).
- Duplicate dots (same mode, within 100 m, different ids): jp 83, fr 14, de 55, it 29, us 98, gb 31, cn 60. Most are a node beside the way/relation of the same station with a different name (Gilroy / Gilroy Transit Center, 13 m); a few are real (五所川原 / 津軽鉄道 五所川原駅 57 m, Neckarsulm / Neckarsulm AVG 37 m). No before/after comparison yet; Japan operator-prefixed names (`阪急西院駅` beside `西院`) are 18 of the remaining polygon duplicates.
- **RECOMMENDED:** one more station-label pass: take the first item of a semicolon list, strip `[...]` and `（在建）`/`(under construction)` suffixes, drop placeholder English names (`待定`, `TBD`), collapse whitespace, and merge the operator-prefixed Japanese duplicates. Then a before/after duplicate-dot count.
- Your call:

### A20. Housekeeping on the installed data
- Legacy top-level `data/{br,ca,ch,co,de,it,mx,us,ve}` have no `classify_report` for the current configs, so plain `check.py --exemptions` (no `--data`) fails with 17 MISSING lines. The app does not read them. Regenerate them from the new output (the permission classifier blocked the agent from overwriting them), or point the plain check at `data/world`? **RECOMMENDED:** point the check at `data/world` and leave the legacy folders alone.
- Old `tiles_tmp/con_draw.geojsonl` files (Oct 16) remain in every slug and in the 3 pilots. The tile writer reads an explicit list of file names (`TILE_LAYERS`, `con_dash` not `con_draw`), so they are ignored. Delete them for tidiness, or leave?
- Backups: `world-backup-20261017` and `world-backup-20261017b` (APFS clones, 9.5 GB each in /private/tmp). Delete once you are happy, or keep until after publish?
- Your call:

### A21. Found by the final Opus review: fix before publishing (none blocks committing the code)
1. **Narrow gauge `usage=main|branch` now makes passenger track where there is none** (HIGH): Jordan 443 km Intercity by tags (Aqaba Railway is freight, the Hejaz line has no regular service), Fiji 161 km (sugar-cane tramways); check Kunming metre gauge in China (215 km Intercity) and Czechia (74 km). India (568 km), Italy FAL/FCE and North Korea look plausible. Proposed: `pipeline/config/jo.json` extending `_default` with `type_overrides` `[{"operator_re":"^Aqaba Railway","type":"freight"},{"name_re":"Hejaz|Hijaz|الحجازي","type":"other"}]`, and `fj` into the freight-by-evidence group (or an `fj.json` typing narrow gauge as freight). **RECOMMENDED**, small.
2. **208 Chinese stations under construction are drawn as open stations** (HIGH, existed before this session): `陶然桥(在建)`, `青龙(建设中)`; `stations.rs:259-273` only skips `railway=construction`. Fix: also skip `construction=yes|station`, `construction:railway`/`construction:public_transport`, and names ending `（在建）`/`(建设中)`. This is part of A19.
3. **150 km rule makes long French TER routes Intercity** (MEDIUM): TER 31 Bordeaux–Limoges 226 km, TER 32 294 km, TER 33 Bordeaux–Sarlat 191 km, TER 2 Paris–Montluçon. Fix: `fr.json` `type_overrides` `{"network_re":"^TER ","type":"regional"}` placed so the TERGV/K9x+ high-speed results stay.
4. **The 0.5 high-speed share promotes a few conventional trains to High-speed** (MEDIUM): Toros Ekspresi (tr, 296 km, share 0.61), Serbian REx Петроварадин–Београд (69 km, 0.54, I had expected Regional), TER 12 Poitiers–Angoulême (0.58); GWR Paddington–Oxford/Cheltenham/Swindon–Westbury are defensible (GWML is `highspeed=yes`). Optional `type_overrides` for `^Toros Ekspresi$` (Intercity) and Serbian `^REx` (Regional).
5. Label bugs (LOW): `clean_name` strips `Bf. `/`Bhf. ` but not `Hp. ` (63 German stations); the merged Dadar station is labelled `Dadar` but its `name_en` is `Dadar (Central)` (`name_tags` adds the stop area's `name:en` even when the own name wins, `stations.rs:899-915`); semicolon lists, `[Aug 2026]`, `待定` and double spaces remain (counts in A19).
6. Data oddities (LOW): Eurotunnel Le Shuttle (`service=car_shuttle`) is Regional, map to Other; KTX 경부선 Pohang routes cover only 17 km in the cut (incomplete OSM relation); Warren Street (London) has modes subway+train, not investigated.
- Your call (apply 1, 3, 5 and the small ones of 6 now? 2 with A19?):

## B. In progress (approved earlier; results will be added here)

| Item | State |
| --- | --- |
| Tile weight steps 1 to 3 (vertex dedupe, DP 4 after join, extent 2048 plus no stations z0 to z5) | built in `tilewriter.rs`, uncommitted. Subset (14 countries): z0-z8 10.09 to 4.64 MB (-54%); Europe z5 laptop 1,264 to 515 KB; z9+ tiles byte-identical; `--deep-zoom 5` and `6` identical. Full-world scan not run (needs a world build); my estimate for the full world is about 650 KB at Europe z5, so z5/z6 may sit just under or over 600 KB |
| Search bar fixes A1 to A9 (IME, memo, failed-load, ARIA, focus), World card, doc fixes D1 to D12, README and r2/README, `.gitignore` | done, `tsc` and `npm run build` pass, screenshots viewed; the country-card problem (A15) is being diagnosed |
| Station code fixes (`stations.rs`): merge within 1 km only, station-before-building, mode from own platforms, names, duplicates, platform counting | in the working tree, uncommitted; verified on scratch output (checker 1 FAILED = known Chengdu, 26 KNOWN); see A17 |
| Classification code fixes: narrow gauge in general, `english_name`, high-speed needs half the route length, multi-value `service`, construction routes | in the working tree, uncommitted; 40 tests pass; see A18 |
| Regeneration of `data/world` with the final code, checker, diff | done and installed; checks green. **Both backups (`world-backup-20261017`, `...b`) have since disappeared from /private/tmp** (not deleted by any agent report I have; probably temp cleanup), so there is no rollback copy: `data/world` is regenerable in about a minute (cut 9 s + build 41 s) |
| Opus final review | done: GO for commit; fixes 5, 6, 10 applied; publish blocked by A21 |
| Opus review of app and docs | done: no critical or high issues; its fixes are being applied |
| Hover lag | cause found and fixed, commit `6fe306f` (`prefetchHighlights` asked a point-in-outline test for every track piece after every idle: 1.8 s of a 4 s idle at Chengdu z10; longest long task 2,054 to 196 ms, Paris 593 to 81 ms). Opus review found two bugs in the fix (region-less dedupe key, no retry after a cancelled prefetch); a follow-up commit is in progress. Not deployed yet. Remaining cost: fast sweeps over many lines still fetch 20-23 shards (4.5-5.2 MB), which needs smaller shards or a custom domain |
| Done: `site.rs` default fill, mn.json restored, cn and at/it narrowed, Americas routes mode, `check.py` and answer lists | done, awaiting review |

## C. Next, after B finishes (needs no further decision unless you say so)

1. Regenerate `data/world` for every changed slug (cn, th, au, nz, mn, jp, tw, id are in mixed states now, plus all configs changed this session). Scratch first, then the real folders. This needs a full re-cut and `build --no-tiles`. Not a world tile build.
2. Run `check.py --data data/world` (answer lists), `--exemptions`, and the pilot check. Goal: ALL PASS except known failures.
3. Opus re-review of everything (configs, Rust changes, search, docs).
4. Commit in separate commits: (1) docs, (2) app (search, legend, hover fix), (3) pipeline code, (4) configs and answers, (5) QA docs. Not before the check passes.
5. World tile rebuild, size check against the 600 KB budget, visual spot check, then `publish.sh`. Needs your go.

## D. Follow-ups worth doing, none blocking

- Glyph coverage for scripts beyond CJK and Cyrillic: Arabic needs shaping, Devanagari, Thai, Hebrew, Greek, Armenian, Georgian, Ethiopic not checked.
- P2 re-measurements on an idle machine: highlight filter cost on very long lines, Chengdu settle at 150 ms, zoom out to z2, hover and click sweeps, first-zoom hitch.
- Not yet checked by QA: Iceland and the micro-states, operator and speed colour modes in the Americas and Africa views, hover and click, rebuilt tile content.
- More independent answer-list entries: the lists are 123 independent and 31 forced; the 32 known failures are judgments from Wikipedia ranking and score, not confirmed OSM bugs.
