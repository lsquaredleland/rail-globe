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

| Phase | Work | Done when |
| --- | --- | --- |
| 0 · Setup | Repo, `docs/`, `CLAUDE.md`. Vite + React + TypeScript app skeleton. One osmium config cutting the three regions | Three extracts from one planet pass; empty app runs locally |
| 1 · Data | Filter to the data spec: features, relations side table, stop positions. QA report per region | Every data spec checklist item done; QA report lists broken routes and orphan stations |
| 2 · Types and stations | Rail type classification, station model, size tiers, per-region commuter lists and override file | A hand-made answer list of about 10 stations and 10 lines per region passes (e.g. Gare du Nord is one complex; Chengdu East is top tier; BART is Metro) |
| 3 · Map | Tiles per region, minimal basemap, both colour modes, construction and freight toggles, labels and tooltips, outlines | Each region reads correctly on a laptop and a phone; views under 600 KB |
| 4 · Compare | The three presets, same-scale side by side, synced zoom, stats panel with region size | SF vs Paris and Paris vs Chengdu show the right totals at the same real scale |
| 5 · Scale | One country (France), then the world. Corridor merging spike and the low-zoom overview. Custom domain before public launch | World build runs end to end; low-zoom views stay inside the budget |

The three pilot regions are small, so the low-zoom problems measured in the research (tile caps, millions of tiny pieces) won't appear until phase 5. Corridor merging can wait until then.

France is the natural step up from Paris for phase 5: dense, well mapped, with a full high-speed network.

## Before phase 2

The China high-speed vs intercity rule is decided (see `00-overview.md`). Phase 2 should confirm it on Chengdu; a second Chinese region is only needed before the China-wide build. See `06-open-questions.md`.
