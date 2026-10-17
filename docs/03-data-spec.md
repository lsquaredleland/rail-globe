# Data spec v3

What the filter keeps from the OSM planet, and what it drops. v3 (2026-10-11) applies the overview's decisions: yard track kept, abandoned, disused and proposed track dropped, no GTFS, local plus English names. Changes from the prototype are marked New or Changed.

## Changes to implement (vs the prototype filter)

- [x] Keep the OSM type and id on every feature as `osm_id` (e.g. `way/123`) (`classify.rs` `osm_id` on every track; checked 2026-10-17)
- [x] Keep `highspeed`, `voltage`, `frequency` and `tracks` on track (`rules.rs` `KEEP`; `tiles.rs` writes `v`, `fq`, `tr`)
- [x] Keep yard track (`service=yard`) (`rules.rs` `SERVICE_KEEP`; `cut.rs` labels `track:<rw>:yard`; type `yard` in `classify.rs`)
- [x] Keep `wikidata`, `uic_ref`, `railway:ref`, `official_name`, `int_name` and `name:*` on stations, halts and platforms (`rules.rs` `KEEP` plus every `name:*` (`cut.rs`); read in `stations.rs`)
- [x] Write the relations side table: `stop_area`, `stop_area_group`, route and `route_master`, with members, roles and the route `service` tag (`cut.rs` writes `relations.jsonl`; `REL_TAGS` has `service`; `RelKind::StopAreaGroup` in `rules.rs`)
- [x] Write stop positions to a build-time file (not tiled) (`cut.rs` writes `stop_positions.jsonl`)
- [x] Change `_routes` to hold route relation ids, not refs or names (`cut.rs` `way_routes`, `"relation/123"` ids)
- [x] Add a derived `_stop_area` id to stations and platforms (`cut.rs` `member_area` writes `_stop_area`; used in `stations.rs`)
- [x] Stop copying `colour` onto ways; it stays on route records (`colour` is in `REL_TAGS` only, not in `KEEP`)
- [x] Drop `passenger` (not in `KEEP` or `cache_key`)
- [x] Drop proposed track (`railway=proposed`); keep construction (`classify.rs` drops it outside `proposed_region` (kept for Hong Kong only, as in `00-overview.md`); construction kept)
- [x] Keep `start_date` on track and stations (`rules.rs` `KEEP`)
- [x] Keep `construction` on `railway=construction` track (`rules.rs` `KEEP`; `classify.rs` reads `construction=*` as the mode)
- [x] Run on the three pilot regions and check that Gare de Lyon and Gare du Nord group correctly (`python3 pipeline/check.py` 2026-10-17: Gare du Nord and Gare de Lyon pass (tier 1, train+subway complex); the one failure in the run is `^成都$` tier 3, known)

## Source and processing

- Source: OpenStreetMap planet extract filtered to railway-related objects
- Output 1: GeoJSON features, converted to MapLibre Tiles (MLT) in a PMTiles file by the Rust tile writer (`pipeline/rail-extract/src/tilewriter.rs`); no tippecanoe, no MVT
- Output 2 (New): relations side table, JSONL, used at build time and for side files, never tiled
- Output 3 (New): stop positions, JSONL, used at build time only
- Every feature keeps its OSM type and id as `osm_id` (New)
- Coordinates rounded to 5 decimals (about 1 m)
- All OSM tags except those listed below are removed

## Geometry types kept

- LineString (track, platform edges, some outlines)
- MultiPolygon (station areas, platforms, yard and depot areas)
- Point (stations and halts)

## Features kept (by `railway` value)

- Operating track: rail, subway, light_rail, tram, monorail, funicular, narrow_gauge
- Narrow gauge (2026-10-17): a `railway=narrow_gauge` way is typed like standard gauge: from its route relations, else `usage` (`tourism` Other, `industrial|freight|military|farm` Freight, `main` / `branch` Intercity / Regional, service track Yard). With none of these it is Other (mine, peat, park and miniature railways are mapped this way too). Under `passenger_needs_evidence` it needs the same evidence as `railway=rail`, and narrow gauge with no evidence of use (no passenger station, nothing beside a passenger route) is Other, not Freight: it is as likely a heritage railway as a freight one (Blenheim Riverside).
- Route `service` with several values (`international;long_distance`): the most prominent type any value gives. A route whose name says it is not running (`en construcción`, `under construction`, `proyectado`, `planned`, ...) or whose `state` is construction / proposed is no service: not a line, and its ways take no type from it.
- A route is High-speed "by track" (no `service` or network rule decides) only when at least half of its length (the track ways of the region that list it) is high-speed track: `highspeed=yes`, or high-speed by its own tags. Track under construction (`railway=construction|proposed`) counts for neither the length nor the high-speed share. Under half, a route at least 150 km long in the region (`LONG_ROUTE_KM`) is Intercity (source `track`: a GWR or CrossCountry service that runs part of its way on a fast line); a shorter one takes the default (Regional). `service=highspeed` and `service=high-speed` count as `high_speed`. The us config keeps the exemption `commuter-lines-not-intercity` (NJ Transit Coast, SEPTA Trenton and Wilmington/Newark, CTrail Shore Line East are Regional).
- English label (`rules::english_name`): `name:en`, then `int_name`. Only for a name that is not Latin script (CJK, Cyrillic, Arabic, ...) also a romanisation key (`name:*-Latn*`, `name:*_rm`, pinyin, romaji). A Latin `name` without `name:en` / `int_name` has no English label (the label is the name); `name:etymology*`, `name:*:wikidata` and other languages' names are never used.
- Planned track: construction only (Changed: proposed is dropped)
- Station-related: station, halt, platform
- Yard areas: yard, and `landuse=railway` polygons
- Service track: siding, spur, crossover, and yard track (`service=yard`, Changed: was dropped)
- Airport outlines and terminals (New): `aeroway=aerodrome` with an IATA or ICAO code, and `aeroway=terminal` with a name, IATA code or ref, as polygons. Written to their own file (`airports.geojsonl`) by the region cut (`pipeline/rail-extract`)
- Station buildings and public transport areas (`building=train_station`, `public_transport=station|platform`)

## Features dropped

- Stop positions (`public_transport=stop_position`, `railway=stop`) from the tiles. They are written to Output 3 for station matching
- Station and subway entrances, including any feature with an `entrance=*` tag
- Switches, buffer stops, crossings, level crossings, signals
- Abandoned, disused, razed, demolished, miniature, milestone and other non-operating railway types (decided: not shown)
- Proposed track
- `railway=platform_edge`

## Tags kept

| Tag | Purpose | Change |
| --- | --- | --- |
| osm_id (derived) | OSM type and id, e.g. `way/123`; the basis for stable ids | New |
| railway | feature type | |
| public_transport | platform / station class | |
| building | station buildings | |
| landuse | railway area class | |
| station | station type (subway, light_rail, ...) | |
| service | siding / spur / crossover / yard | Changed (yard added) |
| usage | main / branch / industrial (passenger vs freight) | |
| railway:traffic_mode | passenger / freight / mixed | |
| highspeed | high-speed flag; zoom class for z0–4 | New |
| name, name:*, official_name, int_name | labels in all languages; English fallback | Changed (was name, name:en) |
| ref | on track: infrastructure line number. On platforms: platform number | Changed (was "line or route ref") |
| wikidata | joining duplicates, station importance | New |
| uic_ref, railway:ref | station ids | New |
| operator, operator:short, network | agency (raw OSM values) | |
| gauge | track gauge | |
| electrified | power type (contact line, third rail) | |
| voltage, frequency | AC vs DC | New |
| tracks | number of tracks, for corridor counts | New |
| maxspeed, maxspeed:freight | speed limits | |
| bridge, tunnel | structure flags | |
| light_rail, subway, train | mode flags | |
| start_date | opening date, kept for a later history timeline | New |
| construction | what a `railway=construction` way is being built as (`rail`, `subway`, `tram`, ...); needed to colour it by type | New |
| _operators (derived) | agencies from route relations that contain the feature | |
| _routes (derived) | ids of route relations that contain the feature | Changed (was refs or names) |
| _stop_area (derived) | id of the `stop_area` a station or platform belongs to | New |

Removed: `colour` on ways (it stays on route records, since shared track has several colours) and `passenger` (found on one feature; `usage`, `railway:traffic_mode` and route relations carry that signal).

## Relations side table (New)

One JSONL row per relation, written by the filter. The service layer and the station model read it; tiles never include it.

| Field | Contents |
| --- | --- |
| id | relation id, e.g. `relation/456` |
| kind | `stop_area`, `stop_area_group`, `route` or `route_master` |
| tags | name, `name:*`, ref, colour, operator, network, route (mode), service (high_speed, long_distance, regional, commuter), from, to, wikidata |
| members | ordered list of type, id and role (`stop`, `platform`, or empty for track) |

## Stop positions file (New)

One JSONL row per `public_transport=stop_position` or `railway=stop` node: `osm_id`, coordinates, name, mode flags, ref. Used to match route stops to stations, then discarded.

## Known gaps and caveats

- Operator names are not normalized; one agency can appear under several names, and some features list several agencies.
- `colour` is mostly set on route relations rather than ways. It stays on the route records in the side table.
- `railway:traffic_mode` is sparsely tagged. Passenger vs freight mostly has to be inferred from `usage` and route relations.
- `stop_area` relations cover at most about 14% of rail stations, so most station grouping is inferred (see the station model).
- Tag completeness varies by country and mapper. Chinese intercity services are thin in OSM; this is a known gap.
- `owner` and train protection tags are not kept (signalling and owner themes are parked).
