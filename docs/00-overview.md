# Rail Atlas spec: Overview

As of 2026-10-11 · Leland

Rail Atlas is a static web map of passenger rail built from OpenStreetMap. Its purpose is comparison: put city or region A next to city or region B, at the same real scale, and see the difference in magnitude.

## Goal and direction

- **Primary content:** passenger rail infrastructure (track, stations, platforms, station grounds, yards) and the services that run on it (lines from OSM route relations).
- **Core use:** compare two regions side by side, e.g. the SF Bay Area vs Paris, with totals for each.
- **Magnitude, not just lines:** station and yard outlines are drawn so the physical footprint of the railway is visible. Yards are cheap in file size, so they are shown.
- **Readable by default:** a minimal basemap, standard rail types, and a choice between type colours and operator colours.

## Scope

| In v1 | Parked (not in v1) |
| --- | --- |
| Passenger track, stations, platforms, station grounds, yards | GTFS of any kind: frequencies, timetables, Portolan, train animation |
| Lines from OSM route relations | Live train positions |
| Two colour modes: by rail type, by operator colour | History timeline (`start_date` is kept in the data for later) |
| Standard rail types (high-speed to tram) | Comparing custom drawn areas |
| Compare view for preset regions | Routing |
| Local name on the map; local + English in tooltips | Accounts, saved views |
| Freight and construction track, each with a visibility toggle | Abandoned, disused and proposed track (dropped) |
| Pilot: three regions, then one country, then the world | Signalling and owner themes |

## Decisions

Everything below is settled. Anything not settled is in `06-open-questions.md`.

| Date | Decision | Detail |
| --- | --- | --- |
| 2026-10-11 | "HSR only" is a switch in the toolbar that works in every colour mode (rail type, operator, speed); in the link it is `hsr=1`. With it on, track that is not high-speed is greyed and thinner (open or under construction), their names are hidden, and stations that high-speed rail does not serve are removed from the map (stations and names, including those under construction). A station counts as served if its high-speed flag is set or high-speed is among the rail types serving it | Same station-type data that makes hiding a rail type hide its stations. Under-construction track follows the same rule: grey unless high-speed. One shared definition of "active" (`activeTrack`, `activeStation` in `app/src/mapStyle.ts`) drives open track, construction track, track names and stations |
| 2026-10-11 | Stations under construction are included and shown (hollow ring, muted name) only while the Construction toggle is on. A named node counts if it has `railway=construction` with `construction=station\|halt`, or `construction:railway=station\|halt\|stop`, or `construction:public_transport=station\|stop_position`. Nodes of the same name within 400 m are one station; one is skipped if an open station of the same name is within 600 m | There is no physical size, so the tier is inferred from what is being built: a mainline (train) station is high-speed if high-speed track under construction lies within 200 m or its name says 高铁站 / high-speed railway; it is major (tier 1) if high-speed and 4 or more pieces of mainline track under construction lie within 200 m, or its name matches the region's major-station pattern; high-speed but not major is tier 2; everything else, including metro and light rail, is tier 3. Track pieces stand in for platforms and land, line counts are never used. Nodes whose names end in one another (深圳机场东 / 机场东) within 400 m count as one station. Proposed stations (`railway=proposed`) are still dropped. Example: Xili High-speed Railway Station in Shenzhen. Run `con_stations.py` after `stations.py` |
| 2026-10-11 | Phase 1 filter runs as `osmium tags-filter` (prefilter) followed by a C++ program (`pipeline/cpp/filter.cpp`, libosmium). Output is identical to the old Python filter on all three pilots (same features, geometry, relations, stop positions and QA) | Filter and classify went from about 7 minutes to about 2 seconds on the Greater Bay Area extract. Needs `brew install libosmium protozero`. The Python version stays as `filter_py.py` for comparison |
| 2026-10-11 | Fourth pilot region: Greater Bay Area in China (`greater-bay-area`): the 9 Pearl River Delta cities plus Hong Kong and Macao, cut from a China extract (`china.osm.pbf`). A larger 15-county Northern California region was built as a test and then deleted at the user's request | Outline from one Nominatim lookup per area (about 5 s each). Timings are in `pipeline/README.md` |
| 2026-10-11 | Light rail and tram are one rail type, "Light rail / tram"; Metro is labelled "Metro / subway". Hiding a rail type also hides stations whose only service is hidden types (types come from the station's lines and from track within about 70 m that matches the station's mode). A Speed colour mode (by maxspeed, or grey out non high-speed) joins Rail type and Operator colours | Eight rail types remain: high-speed, intercity, regional, commuter, metro, light rail / tram, other, plus freight and yard |
| 2026-10-11 | Airports: the aerodrome outline and terminals are drawn as infrastructure (pulled from OSM `aeroway=aerodrome` and `aeroway=terminal`; the basemap only has runways, taxiways and aprons, which are restyled). The plane icon and IATA code sit at the centre of each commercial airport's outline; stations at an airport are ordinary station circles | Commercial = an IATA code, not military, 300 ha or more. No icon on stations (changed 2026-10-11 on request); the tooltip still says when a station is at an airport. Airport outlines are an addition to the v1 scope, listed in `03-data-spec.md` |
| 2026-10-11 | Major station rules: a station is forced to tier 1 by (1) a per-region name pattern (China: city name plus optional direction, e.g. 成都东, 成都西, 天府), (2) a canonical terminal list kept per region (Paris termini, SF 4th & King and Diridon), or (3) a mainline station within 80 m of high-speed track (by the track's own tags) that is a terminus or has 8 or more mainline platforms | Rules 1 and 2 are hand-kept overrides in `pipeline/config/<slug>.json`; rule 3 uses infrastructure only. The score itself is unchanged and line counts still never set rank. A "terminus of a high-speed route" test was not used because it would read the service layer |
| 2026-10-11 | China high-speed vs intercity, applied to track: maxspeed at or above 250 km/h, or a name containing 高速线, 客专/客运专线, 高速鐵路/高速铁路, "High Speed Rail" or "Express Rail Link" → High-speed (the last three added 2026-10-11 so Hong Kong's 廣深港高速鐵路 section, limited to 200 km/h or less inside Hong Kong, counts). `highspeed=yes` (or 城际 in the name) with maxspeed 200–249 or no usable speed → Intercity | Tested on Chengdu: 200 km/h lines (成灌, 沪蓉, 川藏) carry `highspeed=yes` but are intercity-class. Train-number prefixes are not used (Chengdu has almost no mapped train routes). Recheck on a second Chinese region before the China-wide build |
| 2026-10-12 | Commuter is merged into Regional for now (one type, "Regional / commuter"; `service=commuter` and the per-region commuter lists no longer make a separate type). Funicular and cable car (`railway=funicular`, `route=funicular`) are Light rail / tram. Monorail stays Other. Tourist and heritage rail (`usage=tourism`, `service=tourism`) is Other | The commuter/regional split depended on hand-kept lists per region; split again later with a measurable rule. Regional track now appears from z5 (Commuter was z7), so RER and Transilien show earlier |
| 2026-10-12 | Monorail and Funicular and cable car are their own rail types (replaces the funicular → Light rail / tram line above; monorail was Other). The rail types legend is a row of toggle pills with All and None buttons (None hides every type; Freight keeps its own pill) | Gives them their own colour, thickness and show/hide toggle. Tracks, route lines and stations all carry the new types, so the pipeline needs a rebuild |
| 2026-10-12 | Proposed track (`railway=proposed`) is back as an optional layer, off by default, with its own "Proposed" switch next to Construction (link: `prop=1`). Test scope is Hong Kong only: the region config names a `proposed_region` (`cn/hk.json` for the Greater Bay Area) and proposed track outside it is dropped in classify. The mode comes from `proposed=*` (rail, subway, light_rail...), else from rail-like tags, else Other. Drawn dashed like construction but fainter. Proposed stations, the Lines panel and station ranking ignore it | Reverses the 2026-10-11 "proposed dropped" decision, for testing. The filter keeps all proposed ways in every region; no PBF extract needs redoing (the prefilter already keeps `railway=*`). Hong Kong has 36 proposed ways in the data, most named (KCR Northern Link, South Island Line West, Tuen Ma Line pieces) |
| 2026-10-11 | China 200 km/h lines that should still count as high-speed are listed per region as `high_speed_track_overrides` in `pipeline/config/<slug>.json` (same match rules as `type_overrides`: `name_re`, `osm_id`). The 250 km/h rule stays the default. **Footnote:** later, add a flag (per region or global) to switch China to the non-China rule (`highspeed=yes` or maxspeed at or above 200 km/h), which would make these lists unnecessary | Chosen over changing the global threshold because the Chengdu test showed 200 km/h lines are intercity-class in China's own terms. Overrides are tagged `type_source=override` |
| 2026-10-11 | Rich tooltips: stations show size tier, platforms and line counts; track shows lines, electrification, max speed, gauge and tracks; lines show type, operator and ends | Full lists stay in click panels. Line counts are labelled "mapped in OSM" and never set rank. Phones: tap for tooltip, tap again for panel |
| 2026-10-11 | Pilot regions: SF Bay Area, Chengdu, Paris | Paris for Europe: extract, measurements and station tests already exist |
| 2026-10-11 | App: Vite + React + TypeScript | React makes the UI easier to manage over time |
| 2026-10-11 | Preset regions use administrative boundaries | 9-county Bay Area, Île-de-France, Chengdu municipality; region size and per-km² figures shown |
| 2026-10-11 | Construction lines shown dashed, with a visibility toggle; proposed lines dropped | |
| 2026-10-11 | Freight and industrial track shown muted grey, with a visibility toggle | |
| 2026-10-11 | English names: `name:en`, then `int_name`, then a Latin-script `name:*`, then none | Automatic pinyin only if Chengdu reads badly |
| 2026-10-11 | High-speed threshold: maxspeed at or above 200 km/h | China needs a separate rule for high-speed intercity (still open) |
| 2026-10-11 | Hybrid systems: route `service` tag, else a per-region override file. BART is Metro | Commuter network lists written by hand for the three pilot regions |
| 2026-10-11 | Station size weights and tiers tuned on the answer lists | Terminus = mainline track ending inside the station's railway land |
| 2026-10-11 | Operator-colour mode: side-by-side strands from z12, one colour below; missing colours use the type colour at lower weight | Line ordering built in-house |
| 2026-10-11 | Basemap: OpenFreeMap restyled | Protomaps basemap if restyling is too limited |
| 2026-10-11 | Performance budget: under 600 KB per view on a mid-range phone | |
| 2026-10-11 | Compare panel: region size, track km by rail type, stations by size tier, platforms, station ground and yard area, electrified share | Synced zoom (scale-corrected), independent pan |
| 2026-10-11 | Corridor merging: spike on France in phase 5 | Attribute-merged overview is the fallback |
| 2026-10-11 | Custom domain deferred to public launch | `r2.dev` for the pilot |
| 2026-10-11 | Attribution: OSM credit on the map; side files offered under ODbL | |
| 2026-10-11 | Keep `start_date` in the filtered data | For a later history timeline; the feature stays parked |
| 2026-10-11 | Direction: passenger infrastructure and services, built for comparing regions | Station and yard outlines show magnitude |
| 2026-10-11 | GTFS is not used | OSM route relations are the only service source. Chinese intercity services stay a known gap |
| 2026-10-11 | Abandoned and disused track is dropped | Not in the data or the tiles |
| 2026-10-11 | Yards are shown | Yard outlines from z12, yard track from z13 |
| 2026-10-11 | Stable ids | Built from OSM ids; see `02-architecture.md` |
| 2026-10-11 | Labels | Local name on the map; local and English names in tooltips |
| 2026-10-10 | Station ranking uses physical size only | Mainline platforms and tracks, land area, a terminus flag. Line counts never set rank |
| 2026-10-10 | Three-level station model | Complex, station, stop point |
| 2026-10-10 | Data spec keeps OSM ids and a relations side table | See `03-data-spec.md` |
| 2026-10-08 | Three data layers: infrastructure, service, link | Infrastructure builds first |
| 2026-10-04 | Static delivery | PMTiles on R2, MapLibre, monthly rebuild, no server |
