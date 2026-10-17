# Pilot plan

Build three regions end to end before any country or the world: the SF Bay Area, Chengdu and Paris. Each tests something the others don't, and together they cover every v1 feature. They are also the first compare-view presets.

## Pilot regions

Boundaries are administrative areas (decided).

| Region | Boundary | Why this one | What it tests |
| --- | --- | --- | --- |
| **SF Bay Area** (`sf-bay-area`) | The 9 Bay Area counties | Leland knows it well, so mistakes are easy to spot | Many system types side by side: BART, Caltrain, Muni Metro and streetcars, VTA light rail, Amtrak. US tagging conventions. Hybrid systems (BART is classed as Metro) |
| **Chengdu** (`chengdu`) | Chengdu municipality | The one Chinese region | A large metro, high-speed hubs, thin service mapping, Chinese local names with English tooltips. Proves size-only station ranking works where services are missing. Tests the China high-speed vs intercity rule |
| **Paris** (`paris`) | Île-de-France | The complex European region | Six major mainline terminals, RER sharing track with Transilien (heavy interlining), metro, tram, TGV. A Paris extract, baseline measurements and station tests already exist |

## Phases

Status 2026-10-17: the world build runs end to end and is deployed (R2 `r2.dev` for data, Vercel for the app, `pipeline/publish.sh`); the app reads only `world/world.pmtiles` and no longer has the pilot regions. Phase 5 "Done when" is not met yet: the low-zoom views are over the 600 KB budget (`11-performance-todos.md`) and the custom domain is not attached. The pilot regions below stay as the data-check regions (`pipeline/answers/`).

| Phase | Work | Done when |
| --- | --- | --- |
| 0 · Setup | Repo, `docs/`, `CLAUDE.md`. Vite + React + TypeScript app skeleton. One osmium config cutting the three regions | Three extracts from one planet pass; empty app runs locally |
| 1 · Data | Filter to the data spec: features, relations side table, stop positions. QA report per region | Every data spec checklist item done; QA report lists broken routes and orphan stations |
| 2 · Types and stations | Rail type classification, station model, size tiers, per-region commuter lists and override file | A hand-made answer list of about 10 stations and 10 lines per region passes (e.g. Gare du Nord is one complex; Chengdu East is top tier; BART is Metro) |
| 3 · Map | One joined tile source (`pilot.pmtiles`, from per-region files), minimal basemap, both colour modes, construction and freight toggles, labels and tooltips, outlines | Each region reads correctly on a laptop and a phone; views under 600 KB |
| 4 · Compare | The three presets, same-scale side by side, synced zoom, stats panel with region size | SF vs Paris and Paris vs Chengdu show the right totals at the same real scale |
| 5 · Scale | One country (France), then the world. Corridor merging spike and the low-zoom overview. Custom domain before public launch | World build runs end to end; low-zoom views stay inside the budget |

The three pilot regions are small, so the low-zoom problems measured in the research (tile caps, millions of tiny pieces) won't appear until phase 5. Corridor merging can wait until then.

France is the natural step up from Paris for phase 5: dense, well mapped, with a full high-speed network.

## Before phase 2

The China high-speed vs intercity rule is decided (see `00-overview.md`). Phase 2 should confirm it on Chengdu; a second Chinese region is only needed before the China-wide build. See `06-open-questions.md`.

## Automatic data check (Claude runs this, unasked)

After any change that can alter the data (a new region, a rule in `classify.rs` or `stations.rs`, a config, a rebuild of tiles or side files), Claude runs a checker before reporting the work as done, and reports what failed. It does not wait to be asked, and a region is not "done" until it passes.

What the checker covers, per region touched:

1. **Answer lists.** `pipeline/check.py` runs the hand-made lists in `pipeline/answers/<slug>.json` (major stations and their tier, lines that must exist, rail types of named lines). A region without a list gets one drafted from its biggest stations (`top.py`) and the user's knowledge of the place, then checked.
2. **Major stations.** The top 20 stations by size must be real, correctly named, in the right tier and (for the big terminals) tier 1; none may be missing, split in two, or an airport or yard wrongly ranked. Compare against a known list (Wikipedia busiest stations of the country).
3. **Random city spot checks.** Pick at least 5 cities at random (a mix of big, mid and small, and at least one with only a tram, one with a metro, one with none), and for each check: it has a dot only if it has metro, tram or monorail; it has one dot, not two; its metro/tram length and station count are within about 25% of a reference (Wikipedia, operator pages, or OSM itself with a second query); its main station is on the map under the right name.
4. **Aggregates.** Region totals (route km by type, stations, lines) against published figures: network length per country or city, count of high-speed km (UIC 200 km/h outside China, 250 km/h in China), metro systems and their km. Flag anything off by more than 25%, or a type that is zero where it should not be (no metro in a city that has one, no high-speed in France).
5. **Rules that are easy to get wrong.** High-speed threshold by country; S-Bahn is not a city dot; tram is told from light rail by its `railway` tag; no abandoned, disused or proposed track in the data; every feature keeps its `osm_id`.
6. **Diff against the previous build.** Counts per type and per region compared with the last build (`qa.json`); a change above 10% must be explained or fixed.

How to report: a short table of what was checked, what passed, what failed (with the number and the reference), and what was not checked. A failure caused by OSM tagging (for example Stuttgart's U-Bahn tagged `light_rail`) is fixed with a per-region rule in `config/<slug>.json` and added to the answer list so it stays fixed.

Status: `check.py` covers items 1 and part of 2 today; items 3 to 6 are to be built into a single `pipeline/verify.py` (random city sampler, reference table, build diff) in phase 5, and run from then on after every build.
