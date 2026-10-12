# Data spec v3

What the filter keeps from the OSM planet, and what it drops. v3 (2026-10-11) applies the overview's decisions: yard track kept, abandoned, disused and proposed track dropped, no GTFS, local plus English names. Changes from the prototype are marked New or Changed.

## Changes to implement (vs the prototype filter)

- [ ] Keep the OSM type and id on every feature as `osm_id` (e.g. `way/123`)
- [ ] Keep `highspeed`, `voltage`, `frequency` and `tracks` on track
- [ ] Keep yard track (`service=yard`)
- [ ] Keep `wikidata`, `uic_ref`, `railway:ref`, `official_name`, `int_name` and `name:*` on stations, halts and platforms
- [ ] Write the relations side table: `stop_area`, `stop_area_group`, route and `route_master`, with members, roles and the route `service` tag
- [ ] Write stop positions to a build-time file (not tiled)
- [ ] Change `_routes` to hold route relation ids, not refs or names
- [ ] Add a derived `_stop_area` id to stations and platforms
- [ ] Stop copying `colour` onto ways; it stays on route records
- [ ] Drop `passenger`
- [ ] Drop proposed track (`railway=proposed`); keep construction
- [ ] Keep `start_date` on track and stations
- [ ] Keep `construction` on `railway=construction` track
- [ ] Run on the three pilot regions and check that Gare de Lyon and Gare du Nord group correctly

## Source and processing

- Source: OpenStreetMap planet extract filtered to railway-related objects
- Output 1: GeoJSON features, intended for conversion to vector tiles (tippecanoe, then PMTiles)
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
- Planned track: construction only (Changed: proposed is dropped)
- Station-related: station, halt, platform
- Yard areas: yard, and `landuse=railway` polygons
- Service track: siding, spur, crossover, and yard track (`service=yard`, Changed: was dropped)
- Airport outlines and terminals (New): `aeroway=aerodrome` with an IATA or ICAO code, and `aeroway=terminal` with a name, IATA code or ref, as polygons. Written by `pipeline/airports.py`, not by the main filter
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
