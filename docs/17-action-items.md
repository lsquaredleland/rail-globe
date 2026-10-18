# Open action items (rewritten 2026-10-17)

Everything fixed in the 2026-10-16/17 session has been removed from this list; what was done is in `docs/18-changelog-2026-10-17.md`. What is left needs a decision from the user, a check, or follow-up work. Reply under each `Your call:`.

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
- Your call:

### 1.4 Under-construction stations: shared predicate (proposed)
The app already shows under-construction stations only when the construction flag is on (layers `stcon1-3`, `lblcon1-3`, built by `construction.rs`). `stations.rs` now keeps them out of the open-station model, but `construction.rs` only recognises some tag patterns, so a station skipped by `stations.rs` for `railway=station` + `construction=yes|station` or a `（在建）` name suffix (cn: 63 + 15 + 181) is never drawn, even with the flag on. Proposed: one shared predicate used by both files so every skipped station appears with the flag on. Small change, changes data, needs one more world build.
- Your call:

### 1.5 Station construction skip: tags that cannot prove "open"
Kept open because they carry both an open tag and a construction tag, unverifiable from tags: Montpellier tram line 5 stops, the Dudley extension, au Trentham, Lyonville and Wombat State Forest, kr 하양 and 군위. Still skipped (tags say "being built", unverified): de Sielmingen and Neuhausen auf den Fildern; fr four Ajaccio stops; gb one unnamed node; it Canepari and Pozzuoli; kr nine stations (과천정보타운 to 합천); ch Ils Cugns; th six Bangkok MRT stations. A manual check of a handful settles it.
- Your call:

## 2. To investigate

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

Nothing below has been run.
1. Publishing deploys the live site: `pipeline/publish.sh` (build, verify, upload, push, prune, live checks) after the checks in the changelog. Never run without a go.
2. `r2/sync.sh` has not run since 16 Oct, so `shards.json` still has `max-age=3600` where `no-cache` was intended; one run fixes it. New: it prunes dropped slugs through `r2/dropped-slugs.sh` (guards: kept/dropped overlap, `world.parts.json`).
3. r2.dev is HTTP/1.1, uncompressed, tiles `max-age=3600`: custom domain checklist is in `r2/README.md` (compression rules for `.json` and `.pbf`, build-dated tile file names for immutability, then disable the r2.dev URL).
4. Legacy top-level `data/{br,ca,ch,co,de,it,mx,us,ve}` have no `classify_report` for the current configs; the plain checker now reads `data/world`, so they are unused. Delete or keep.
5. Backups outside the repo: `/Users/leland/Downloads/osm-rail-backups/20261017c` and `20261017d` (APFS clones; delete when satisfied). The earlier two in /private/tmp disappeared.
6. Old `tiles_tmp/con_draw.geojsonl` files are ignored by the tile writer; delete for tidiness if wanted.

## 5. Follow-ups, none blocking

- Repeat the P2 measurements on an idle machine (docs/11): highlight filter cost on very long lines, Chengdu settle time at 150 ms latency, zoom out to z2, hover and click sweeps, first-zoom hitch (roughly doubled by the glyph-free decision: Chengdu East max frame 75 to 125 ms, one noisy pair).
- Not covered by QA: Iceland and the micro-states, operator and speed colour modes in the Americas and Africa views, hover and click on rebuilt tiles.
- More independent answer-list entries; the 29 known failures are judgements from Wikipedia rankings and the size score, not confirmed OSM bugs.
- Pipeline robustness: `build` with no prior `cut` panics (`write route_stations.json: NotFound`); the `type_source` of a route master comes from its first member, not the winning member.
