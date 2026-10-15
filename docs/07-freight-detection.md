# Telling freight from passenger track

OSM has no reliable "freight only" flag. The OSM wiki says lines are assumed to carry passengers unless tagged otherwise, and most freight-only track in China (and much in Europe) is not tagged. This file holds what the research found and the rules the pipeline applies, in order. Code: `pipeline/rail-extract/src/classify.rs` (rules 1 to 9), `cut.rs` (stations, stray track), `stations.rs` (rule 10). Per-region settings: `pipeline/config/<slug>.json`.

## What the research found (2026-10-14)

No library or published heuristic does this. OpenRailwayMap renders from tags (`usage`, `service`) and does not infer freight. What exists is tagging convention, and it is the basis of the rules:

- **Default is passenger.** "Lines are generally assumed to be primarily for use by passengers unless tagged as `railway:traffic_mode=freight` for freight only lines or `usage=industrial` (mines, factories, lumber yards)." ([Railways](https://wiki.openstreetmap.org/wiki/Railways)) So a missing tag means unknown, never freight.
- **`railway:traffic_mode`** takes `freight`, `passenger` or `mixed` (listed on [Key:railway:traffic_mode](https://wiki.openstreetmap.org/wiki/Key:railway:traffic_mode), which does not define them). `freight` on `railway=rail` with `usage=main` or `branch` is the wiki's example of a freight-only line. It is sparse: not in widespread use in North America ([Tagging in North America](https://wiki.openstreetmap.org/wiki/OpenRailwayMap/Tagging_in_North_America)).
- **`usage` or `service`, not both.** `usage=industrial` is for track inside industrial areas, `service=spur` for "short lengths of track, built to give one company or entity access to the main line, normally for freight-only use", and not for spurs that carry passengers ([Tag:service=spur](https://wiki.openstreetmap.org/wiki/Tag:service=spur)). `siding` and `yard` describe layout, not traffic.
- **Shared track** takes the type of the more major service: the longer-distance or heavier line.
- **China** ([OpenRailwayMap/Tagging in China](https://wiki.openstreetmap.org/wiki/Zh-hans:OpenRailwayMap/Tagging_in_China)): main running track is `usage=main` or `branch` by traffic and line class; spurs to factories are `service=spur`; there is no freight tag; freight judgements rely on the national freight station diagram and 95306. **"Stations that handle freight but not passengers should not carry `public_transport=station`."** Station nodes carry `railway:ref` (the 5-digit TMIS code) on freight and passenger stations alike, so a ref says nothing about passengers.
- **Passenger stations in China** can be known exactly: 12306, the national ticketing service, publishes a list of the ~3,400 stations that sell tickets (`station_name.js`). Of 17,457 OSM stations in China, 3,626 are on it. Zhuhai West is not (it is a freight station), 江边 is not.

## The rules, in order (first match wins)

Applied to each track way; "unrouted" means no passenger route relation (`route=train`, `subway`, ... mapped to a line) uses it.

1. **Tags.** `usage=tourism` is Other. `railway:traffic_mode=freight`, `usage=industrial|freight|military|farm` is Freight. `highspeed=yes` or maxspeed at least 250 km/h (200 outside China) is High-speed. `usage=main` is Intercity, `branch` Regional (`unrouted_main_type` in the config).
   - **1b. Minimum high-speed speed** (`high_speed_min_kmh`, 200 in every Americas config: US, Canada, Mexico and the rest). Absent: no change (every other region). Set:
     - A way is High-speed from its own tags only if `maxspeed` reaches the minimum (mph converted: 125 mph = 201 km/h passes, 120 mph does not). `highspeed=yes` alone is not enough. A minimum above 200 is honoured.
     - The "route is High-speed because one of its ways is tagged `highspeed=yes`" step is switched off. A route is High-speed only by network (`high_speed_networks_re`), `service=high_speed` or a `type_overrides` entry, and then only if at least 35% of its track length reaches the minimum (`HS_MIN_SHARE`). Otherwise its type is computed without the high-speed step: Regional (default) for a route that was High-speed by track tags, Intercity for network, service or override.
     - A way on a High-speed line whose own maxspeed is below the minimum or missing takes the line's type without the high-speed step (`type_source` `route-slow`, only when that type really came from the demotion). Neighbour smoothing never lifts a slow way to High-speed.
     - `lines.json` follows the same rule. The app colours by line type (`mapStyle.ts`, `tiles.rs` hs_lines, `stats.rs` hsr). Of 11 formerly High-speed US lines only Amtrak Acela remains High-speed, by a `type_overrides` entry (OSM tags its network `Amtrak`, so `^Acela` never matched; before, it was High-speed only because one of its ways was tagged `highspeed=yes`). An override on a whole line skips the 35% test; Acela's own routes are 24 to 63% fast, so the test would not be stable for it. NJ Transit North Jersey Coast, SEPTA Trenton and Wilmington/Newark and CTrail Shore Line East are Regional (default); Keystone Service is Regional; Northeast Regional, Silver Meteor and both Brightline lines are Intercity by `type_overrides` (see Known failures, long-distance types). MARC Penn Line is Regional by a `type_overrides` entry: 55% of its track is 125 mph (the NEC), so no share threshold separates it from Acela, and it is a commuter service by operator.
     - Track and lines can differ: station `hsr` flags and major-station rules read each track's OWN tags (`stations.rs`, "high-speed track by its own tags"), not line types. Track that is 200 km/h or more by its own tags (Brightline, the NEC where mapped fast, test tracks) is High-speed track whether or not a line on it is High-speed. 41 US stations are flagged `hsr`.
     - `maxspeed:forward` / `maxspeed:backward` are not kept in the data, so a way with only those has no speed and counts as slow.
2. **Service track** (`service=siding|yard|spur|crossover`) is Yard.
3. **Route relations.** Track on a passenger route takes the most prominent route type, whatever its own tags say.
4. **Name share** (`classify.rs`, `mostly_freight`). A named railway whose unrouted ways are Freight on 90% or more, with at least 5 of them, is Freight on all of them. Fixes the untagged ways of a tagged freight line (Haoji: 119 ways).
5. **Per-line overrides** (`track_type_overrides` in the config) for lines nothing else catches. China: 抚乐线 (Fujiang), 东吴支线, 来合铁路.
6. **Station evidence** (`freight_by_evidence`). An unrouted Intercity or Regional railway (ways of one name and ref; unnamed ways joined end to end) is Freight when it is a branch (it has a dead end), it is under 15 km, and no passenger station serves it.
   - A station belongs to the track nearest to it within 200 m, service track not counted, so a branch beside a main line does not get the main line's stations.
   - A station within 800 m of where the railway joins another track belongs to that track (a freight branch starts in a passenger yard).
   - **Passenger station:** China: its name is on the 12306 list (`config/cn-passenger-stations.txt`, from `pipeline/fetch_passenger_stations.py`). Elsewhere: `public_transport=station`, `train=yes` or a UIC code.
   - Never judged: names with 联络线, 直通线, 环线, 高铁, 高速, 客专, 客运专线, 城际, 动车 (connectors, loops, passenger lines); `service`, `traffic_mode=passenger|mixed`, tourism.
   - **6b. Freight names** (`freight_name_re`, set for US, Canada and Mexico). An unrouted Intercity or Regional way whose name matches the pattern is Freight (`type_source` `name-no-station`) unless a passenger station is near it. No dead-end or length condition. Ways on a passenger route never change (rule 3 comes first), and a way whose operator matches `freight_operator_exempt_re` (Metra, Metro-North, LIRR, SEPTA, NJ Transit, MARC, Caltrain, VRE, Amtrak, Metrolink, Brightline, GO Transit, VIA ...) is never changed.
     - US and Canada pattern, case-insensitive, whole words: Subdivision, Sub, District, Division, Railroad, Railway, Rail Line, Rail Link, Branch, Secondary, Industrial, Lead, Spur, RR. These are freight operators' names ("BNSF Panhandle Subdivision", "NS Lafayette District"); "Line", "Main Line" and "Corridor" alone are not in it (Northeast Corridor, Mon Line stay).
     - Shield: a passenger station that belongs to a track of the same name, routed or not, keeps the unrouted ways of that name within 5 km of it (`NAME_SHIELD_KM`), measured from each way's ends and middle. A station within 800 m of a junction of that name's ways does not count (`JUNCTION_CLEAR_M`). The shield is per way, so a long subdivision with one Amtrak stop is Freight beyond 5 km of it. A same-name station anywhere else in the country does not shield.
     - Beside a route: unrouted track that runs along passenger-route track is left alone: at least 80% of the way, not counting 100 m at each end where branches leave (a way under 200 m: all of it), lies within 20 m of track on a passenger route (`ROUTED_BESIDE_M`; the second track of a double-track line, crossovers), or both its ends touch passenger-route track (a gap in a route relation: BNSF Marceline Subdivision on the Southwest Chief). Freight lines 20 to 200 m from an Amtrak line (NS Lake Erie District, CN Matteson Subdivision) are still Freight, correctly.
     - Mexico (`freight_name_per_way`): nearly all unrouted main and branch track is freight (public passenger rail is Tren Maya, Interoceanico, Suburbano, El Insurgente, AIFA), so each way is judged only by the stations beside that way, and the pattern is the concession names: `L[ií]nea <letters>`, `Ramal`, `Libramiento`, `Ferrocarril Mexico/Queretaro/del Norte`. Excluded by design: Tren Maya, Ferrocarril Felipe Angeles, El Insurgente, Suburbano, and `Linea K` and `Linea Z` (single letter, the Interoceanico lines; Z-prefixed names such as ZA are excluded too, K-prefixed such as KA are not, which is arbitrary).
7. **Short spurs.** A railway under 3 km with a dead end that no passenger station is beside is Freight.
8. **Neighbours.** A way under 2 km, typed from its own tags, whose neighbours at both ends all have the same other type (Intercity, Regional, Freight or High-speed) takes that type. A few hundred metres of "Regional" in a freight line is a tagging gap.
9. **Stray track.** A piece under 100 m that touches no other track and is on no route is dropped (not drawn, not counted).
10. **Stations** (`stations.rs`). A train station with no Intercity, Regional or High-speed track near it is never a major station (not by name list, not by high-speed rule) and is at most the smallest size. Zhuhai West.

## Settings and limits

| Setting | Value | Why |
| --- | --- | --- |
| `PAX_NEAR_M` | 200 m | Station nodes sit up to 100 m from track |
| `JUNCTION_CLEAR_M` | 800 m | Stations in a junction's yard belong to the line it joins |
| `MAX_FREIGHT_BRANCH_KM` | 15 km | Longer branches with no station are as likely passenger lines with unmapped stations |
| `SHORT_SPUR_KM` | 3 km | A spur this short with no station at its dead end is a siding to a customer |
| `SNIPPET_KM` | 2 km | Longest piece smoothed to its neighbours' type |
| `STRAY_M` | 100 m | Unconnected pieces below this are noise |
| `high_speed_min_kmh` | 200 (Americas configs) | A route or tag called high-speed must have the speed; absent = off |
| `freight_name_re` | US, CA: operator-style names; MX: concession names | Names that mean a freight railway; absent = off |
| `freight_name_per_way` | MX only | Judge each way by its own stations, not the stations of the same name within `NAME_SHIELD_KM` |
| `ROUTED_BESIDE_M` | 20 m | Unrouted track this close to passenger-route track along its length is part of the corridor (name rule) |
| `NAME_SHIELD_KM` | 5 km | Reach of a same-name passenger station's shield (name rule) |
| `HS_MIN_SHARE` | 35% | Share of a route's track that must reach `high_speed_min_kmh` for a High-speed route (Acela 41%, SEPTA Trenton 26%) |
| `freight_operator_exempt_re` | US, CA: passenger agencies | Operators never made Freight by the name rule |

Known failure modes, so check these when a line looks wrong:

- A **passenger branch whose stations are not mapped, or are mapped without a passenger mark, is hidden as Freight.** Fix: a station tag in OSM, a `track_type_overrides` entry with type `intercity` or `regional`, or the name on a never-judged list.
- A **freight line with a passenger station beside it** (Fujiang: 抚州北, 崇仁 are real passenger stations) is not caught. Fix: an override.
- A **dead end made by the region edge** (a way cut at the border) can make a through piece look like a branch. The 15 km limit contains it.
- **Name rule, US/Canada.** A real passenger line with a freight-style name, no route relation, no mapped station within 5 km and an operator not on the exempt list is hidden. Seen and fixed with `track_type_overrides`: Metrolink subdivisions, GO Transit Weston Subdivision (Canada). A freight subdivision with a historic depot tagged as a passenger station keeps 5 km of itself passenger on each side of it. Unnamed track is not touched, so Intercity and Regional ways with no name stay as they were (US: 8,111 km, mostly freight spurs and secondary track).
- **Long-distance types** (`type_overrides`, US and Canada): Amtrak Northeast Regional, Silver Meteor, Auto Train, Cascades, Adirondack, both Brightline lines and VIA Rail Corridor are Intercity (the other Intercity lines come from `service=` or the default: Empire Service, Ethan Allen Express, Maple Leaf and the rest) (OSM has no `service` tag on them, so the default would be Regional). Corridor services stay Regional: Keystone, Hiawatha, Capitol Corridor, Downeaster, Gold Runner, Hartford Line, Piedmont, Wolverine, Heartland Flyer, Carl Sandburg/Illinois Zephyr, Lincoln Service, Illini/Saluki, Pere Marquette, Blue Water, Borealis, Mardi Gras, Berkshire Flyer, Valley Flyer, Winter Park Express. The rest (Southwest Chief, California Zephyr, Empire Builder, Coast Starlight, Sunset Limited/Texas Eagle, Crescent, Cardinal, Lake Shore Limited, City of New Orleans, Capitol Limited, Palmetto, Silver Star, Carolinian, Vermonter, Pennsylvanian, Pacific Surfliner, VIA Canadian, Ocean and the other VIA routes) are Intercity from `service=` or already by default.
- **Weston Branch** (Oregon, 27 ways) is a freight line and stays Freight; it only matched "Weston" by name.
- **Mexico `Linea FA`** is part of the Interoceanico network. The pattern's `[A-Y][A-Z]{1,2}` matches it, so it is split: 100.0 km Freight (name-no-station, many short pieces), 229.5 km Intercity and 18.9 km Regional (ways that own a station, or are on a route). Undecided; the user decides whether to exclude it.
- **Tourist and heritage railways** (US, Canada): `track_type_overrides` make them Other: any operator containing Scenic, Excursion, Historical, Wine Train, Heritage or Tourist, and the named lines Texas State Railroad, Great Smoky Mountains Railroad, Napa Valley Railroad, Conway Scenic, Cuyahoga Valley Scenic, Oregon Coast Scenic / Port of Tillamook Bay Railroad, Boone and Scenic Valley, Tennessee Overhill. Their route lines in `lines.json` keep their own type (Napa Valley Wine Train is Regional). The same words are on `freight_operator_exempt_re`. `BNSF Scenic Subdivision` is a BNSF freight line and matches nothing here.
- **Name rule, Mexico.** A freight way next to a tagged station stays Intercity or Regional (a concession's ways are judged one by one; a long way owning a station is not split). A passenger service mapped under a `Linea` or `Ferrocarril` name and not in the exclusions would be hidden: check `Linea K`, `Linea Z` and new tourist trains.
- **Slow high-speed.** A route that really is fast but whose ways lack `maxspeed` shows as Intercity or Regional. Fix: `maxspeed` in OSM. Routes are High-speed only by network, service or override, so a new fast service needs one of them in the config.
- The 12306 list is today's ticketing; closed stations and very new ones are missing. Re-run `fetch_passenger_stations.py` per timetable change.

## Tried and rejected: "untagged means freight" (2026-10-14)

Simulation: every Intercity or Regional way (typed from `usage=main|branch` alone) with no passenger route relation and no passenger mark becomes Freight, instead of the rules above. Run on China, Germany, France, Switzerland and Italy against the current rules; the user looked at the pictures, tried it as the real rule, and then removed it.

| Region | Intercity + Regional km now | with the rule | Freight km now | with the rule |
| --- | --- | --- | --- | --- |
| China | 117,540 | 24,842 | 13,328 | 103,876 |
| Italy | 14,273 | 9,886 | 476 | 4,918 |
| Germany | 27,376 | 25,041 | 3,140 | 5,558 |
| France | 16,009 | 15,116 | 2,149 | 3,100 |

It hides real passenger lines wherever route relations are thin: China keeps its tagged high-speed grid but loses most of the conventional network; Italy loses Sardinia's main line, the Rome to Pisa coast line and parts of the Adriatic line. Germany and France barely change. China has about 530 `route=train` relations, so "no route" often means "not mapped". The rules above (assume passenger, find freight by evidence) stay.

## Ideas not done

- The 95306 freight station list (marks stations that handle freight only) would mark freight yards directly; no public dump was found.
- Wikidata has freight lines and heavy-haul lines (P31 and the railway line items); join by `ref` (GB/T 25344 line code) or name.
- Fix the tags in OSM (`railway:traffic_mode=freight` on the lines the rules find): the lasting fix, and it makes every other map better. Export the list of ways each rule flags per release.
- Compare the result against a reference: the data checker in `05-pilot-plan.md` should sample flagged lines and check them against Wikipedia and operator maps.
