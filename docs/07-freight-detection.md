# Telling freight from passenger track

OSM has no reliable "freight only" flag. The OSM wiki says lines are assumed to carry passengers unless tagged otherwise, and most freight-only track in China (and much in Europe) is not tagged. This file holds what the research found and the rules the pipeline applies, in order. Code: `pipeline/rail-extract/src/classify/` (rules 1 to 9), `cut.rs` (stations, stray track), `stations/` (rule 10). Per-region settings: `pipeline/config/<slug>.json`.

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

1. **Tags.** `usage=tourism` is Other. `railway:traffic_mode=freight`, `usage=industrial|freight|military|farm` is Freight. `highspeed=yes` or maxspeed at least 250 km/h (200 outside China) is High-speed. `usage=main` is Intercity, `branch` Regional (`unrouted_main_type` in the config). Narrow gauge (`railway=narrow_gauge`) goes through the same tag rules (2026-10-17; it was always Other before): the Bord na Móna peat railways (`usage=industrial`) are Freight, ARST / FAL / Zillertalbahn track with `usage=branch` is Regional, narrow gauge with no usage stays Other. A route's High-speed type from track alone needs half of its length on high-speed track (`HS_TRACK_SHARE`; construction track not counted); under half, a route of 150 km or more in the region is Intercity (`LONG_ROUTE_KM`), a shorter one Regional. `service=highspeed` is High-speed. Narrow gauge with no passenger evidence (rules 6 and the evidence setting) is Other, not Freight; the stations that serve it are looked for beside narrow-gauge track too.
2. **Service track** (`service=siding|yard|spur|crossover`) is Yard.
3. **Route relations.** Track on a passenger route takes the most prominent route type, whatever its own tags say.
4. **Name share** (`classify/`, `mostly_freight`). A named railway whose unrouted ways are Freight on 90% or more, with at least 5 of them, is Freight on all of them. Fixes the untagged ways of a tagged freight line (Haoji: 119 ways).
5. **Per-line overrides** (`track_type_overrides` in the config) for lines nothing else catches. China: 抚乐线 (Fujiang), 东吴支线, 来合铁路, 昆河线 (Kunming-Hekou metre gauge: passenger service stopped 2003, only freight and a short sightseeing train; 214 km Intercity before, 2026-10-17). Narrow gauge with `usage=main` and no passenger anywhere is typed by the region: `unrouted_main_type: "freight"` in `jo.json` (Jordan: Aqaba Railway freight, Hejaz line Other) and `fj.json` (Fiji sugar-cane tramways). Czechia's 760 mm Jindřichův Hradec lines (71 km, Regional) are real passenger lines and stay.
6. **Station evidence** (`freight_by_evidence`). An unrouted Intercity or Regional railway (ways of one name and ref; unnamed ways joined end to end) is Freight when it is a branch (it has a dead end), it is under 15 km, and no passenger station serves it.
   - A station belongs to the track nearest to it within 200 m, service track not counted, so a branch beside a main line does not get the main line's stations.
   - A station within 800 m of where the railway joins another track belongs to that track (a freight branch starts in a passenger yard).
   - **Passenger station:** China: its name is on the 12306 list (`config/cn-passenger-stations.txt`, from `pipeline/fetch_passenger_stations.py`). Elsewhere: `public_transport=station`, `train=yes` or a UIC code.
   - Never judged: names with 联络线, 直通线, 环线, 高铁, 高速, 客专, 客运专线, 城际, 动车 (connectors, loops, passenger lines); `service`, `traffic_mode=passenger|mixed`, tourism.
7. **Short spurs.** A railway under 3 km with a dead end that no passenger station is beside is Freight.
8. **Neighbours.** A way under 2 km, typed from its own tags, whose neighbours at both ends all have the same other type (Intercity, Regional, Freight or High-speed) takes that type. A few hundred metres of "Regional" in a freight line is a tagging gap.
9. **Stray track.** A piece under 100 m that touches no other track and is on no route is dropped (not drawn, not counted).
10. **Stations** (`stations/`). A train station with no Intercity, Regional or High-speed track near it is never a major station (not by name list, not by high-speed rule) and is at most the smallest size. Zhuhai West.

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

## Americas: correct freight labelling (requirement, 2026-10-15)

**The requirement (user).** Label freight correctly. Too many lines are drawn as passenger-related traffic. This outranks everything else in the Americas work: a freight line drawn as passenger is a worse error than a rarely used passenger line drawn as freight.

**What is wrong today (screenshots of 54 views, pass 1 in `08-americas-passenger-qa.md`).**
- US and Canada: short-line and regional freight (west of Fort Worth, Kansas, Oklahoma, Nebraska, the Dakotas, the Rust Belt) is drawn as Regional or Intercity. BTS NARN, an independent passenger-use inventory, gives no passenger code under 58% of the US Regional km and 7 to 9% of the Intercity km.
- South America, Cuba, Dominican Republic: whole freight and sugar-railway networks are drawn as Intercity or Regional (Argentina's Belgrano Cargas and Ferroexpreso Pampeano network, Brazil's MRS and FCA, Uruguay, Chile south of Chillan, Peru, Bolivia, most of Cuba, the sugar lines of the Dominican east).
- Cause: outside the US, Canada and Mexico passenger route relations are sparse, so unrouted track is typed from `usage=main` / `branch` alone (default Intercity or Regional). The existing evidence rules only demote a railway that is a short dead-end branch, or whose name matches a freight pattern; the rest stays passenger by default.

**Principle to implement.** Passenger needs evidence. Unrouted track in the Americas is Freight unless: it is on a passenger route relation; a passenger station serves its railway; it runs beside routed passenger track (second tracks, crossovers); or an explicit override says so. Metro, light rail, tram, monorail and funicular are not affected. When evidence is too thin to decide (a country with almost no tagged stations), prefer Freight and record the cost here.

**Acceptance tests.**
- US and Canada, measured with the BTS NARN passenger segments (public domain, NTAD North American Rail Network Lines, `PASSNGR` A, B, C, D, E, I, R, V, O; T tourist is not passenger): our Intercity, Regional and High-speed km with under 10% NARN overlap falls sharply (Regional first), while NARN passenger km still covered by our passenger track stays at or above 95% (US) and 90% (Canada), per class A, B, C, V, R.
- Screenshots: no short-line freight drawn as Regional in the Plains and Texas views; South America, Cuba and the Dominican Republic show only plausible passenger lines.
- Real services that must survive: Amtrak (all routes), Metra, Metro-North, LIRR, NJ Transit, SEPTA, MBTA, MARC, VRE, Caltrain, ACE, Coaster, Metrolink, Sounder, FrontRunner, TRE, Brightline, Alaska Railroad, Rail Runner; VIA, GO, exo, West Coast Express; Mexico Tren Maya, Interoceanico, Suburbano, Interurbano, Chepe; Argentina Roca, Sarmiento, Mitre, San Martin, Belgrano Sur and Norte, Urquiza, Tucuman, Mar del Plata, Bahia Blanca, Rosario, Tren del Valle; Chile EFE Metrotren, Biotren, Tren Central to Chillan, Merval; Brazil EFVM, EFC Carajas, CPTM, SuperVia, CBTU Recife, Natal and BH, VLTs; Peru Lima metro (PeruRail and Ferrocarril Central are tourist: Other); Colombia Medellin; Panama Canal Railway; Uruguay AFE suburban; Cuba Havana-Santa Clara-Camaguey-Santiago, Havana-Pinar del Rio, Hershey.
- Regression: the 9 pilot regions (sf-bay-area, paris, chengdu, greater-bay-area, china, de, fr, ch, it) stay byte-identical unless a rule is deliberately switched on for them.

**What was tried and reverted on 2026-10-15 (kept in the git history of branch `americas-qa`, not in the code).** `high_speed_min_kmh`; `freight_name_re` with a 5 km station shield and operator exemptions; a beside-passenger-route rule (20 m); Amtrak and VIA long-distance overrides; tourist railways as Other; Mexico per-way name rule; generic configs for 26 Americas countries; `build --combined`; the Americas as a second tile source in the app. They removed about 145,000 km of US freight from passenger types but were rule-by-rule patches on a "passenger unless proven freight" default, which is the opposite of the principle above. Pass-2 experiment options coded but never run: `freight_unrouted_no_station`, `freight_isolated_km`, `passenger_station_level`.

**Findings worth keeping.**
- Real Amtrak trackage over freight railroads (UP Beaumont, Dallas, Austin, Coast; BNSF Spokane, Fort Worth, Seligman; about 400 km) has no route relation and no station of its own: an evidence rule must not turn it to Freight (route members, beside-route and NARN-style trackage rights matter).
- NARN `C` (commuter) is inherited by freight lines that share track with commuter trains; `T` is tourist; Canadian Ontario Northland, Algoma Central and Gaspe codes are probably stale (suspended service).
- Acela's network tag is `Amtrak`, so a `^Acela` network match never fires; a route-derived High-speed type applied to every slow way on a route (NEC at 30 to 125 mph, Metro-North) is wrong: high-speed should need the track's own speed.
- Rocky Mountaineer (Mountain, Laggan, Prince George Subdivisions), Texas State Railroad, Great Smoky Mountains Railroad, Napa Valley and Cuyahoga Valley are tourist railways, not passenger service.

Also (2026-10-17, A21): a route's `service=car_shuttle` (Eurotunnel Le Shuttle) is Other, not a passenger type. Where a long-distance route shares suburban track (Perth Midland Line with the Indian Pacific, Wellington to Waikanae with the Capital Connection), the track takes the most prominent route type, so the suburban line was part Intercity; `au.json` and `nz.json` carry exemptions (`perth-midland-line`, `wellington-kapiti-line`, the latter by way id because the main trunk keeps one name) that set that track to Regional. The Ghan is Intercity like the Indian Pacific (`au.json` `type_overrides`, decision A2). French `TER ` routes are Regional whatever their length (`fr.json` `type_overrides`, after exceptions that keep TER K92+/K94+ High-speed, K90+ Intercity and the tram-train TER 21-23 Lyon, Aléop T1/T2 Light rail).

## Ideas not done

- The 95306 freight station list (marks stations that handle freight only) would mark freight yards directly; no public dump was found.
- Wikidata has freight lines and heavy-haul lines (P31 and the railway line items); join by `ref` (GB/T 25344 line code) or name.
- Fix the tags in OSM (`railway:traffic_mode=freight` on the lines the rules find): the lasting fix, and it makes every other map better. Export the list of ways each rule flags per release.
- Compare the result against a reference: the data checker in `05-pilot-plan.md` should sample flagged lines and check them against Wikipedia and operator maps.
