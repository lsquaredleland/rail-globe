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
2. **Service track** (`service=siding|yard|spur|crossover`) is Yard.
3. **Route relations.** Track on a passenger route takes the most prominent route type, whatever its own tags say.
4. **Name share** (`classify.rs`, `mostly_freight`). A named railway whose unrouted ways are Freight on 90% or more, with at least 5 of them, is Freight on all of them. Fixes the untagged ways of a tagged freight line (Haoji: 119 ways).
5. **Per-line overrides** (`track_type_overrides` in the config) for lines nothing else catches. China: 抚乐线 (Fujiang), 东吴支线, 来合铁路.
6. **Station evidence** (`freight_by_evidence`). An unrouted Intercity or Regional railway (ways of one name and ref; unnamed ways joined end to end) is Freight when it is a branch (it has a dead end), it is under 15 km, and no passenger station serves it.
   - A station belongs to the track nearest to it within 200 m, service track not counted, so a branch beside a main line does not get the main line's stations.
   - A station within 800 m of where the railway joins another track belongs to that track (a freight branch starts in a passenger yard).
   - **Passenger station:** China: its name is on the 12306 list (`config/cn-passenger-stations.txt`, from `pipeline/fetch_passenger_stations.py`). Elsewhere: `public_transport=station`, `train=yes` or a UIC code.
   - Never judged: names with 联络线, 直通线, 环线, 高铁, 高速, 客专, 客运专线, 城际, 动车 (connectors, loops, passenger lines); `service`, `traffic_mode=passenger|mixed`, tourism.
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

Known failure modes, so check these when a line looks wrong:

- A **passenger branch whose stations are not mapped, or are mapped without a passenger mark, is hidden as Freight.** Fix: a station tag in OSM, a `track_type_overrides` entry with type `intercity` or `regional`, or the name on a never-judged list.
- A **freight line with a passenger station beside it** (Fujiang: 抚州北, 崇仁 are real passenger stations) is not caught. Fix: an override.
- A **dead end made by the region edge** (a way cut at the border) can make a through piece look like a branch. The 15 km limit contains it.
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
