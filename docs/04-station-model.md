# Station model

Model stations as three levels (complex, station, stop point), keep the OSM relations that tie them together, and rank stations by physical size alone, so large mainline terminals stand out.

## The problem: one station, many objects

A station a traveller thinks of as one place is usually several OSM objects, often one set per mode or operator. Gare de Lyon is three OSM stations (mainline, metro, untagged). Consolidation means deciding which objects belong together, at which level, and what the result is called and ranked by.

| OSM object | What it tells you | Layer |
| --- | --- | --- |
| `railway=station` / `halt` node | That a station exists, its name and ids | Both |
| `public_transport=station` (node or area) | Same, in the newer public transport scheme | Both |
| `building=train_station`, `landuse=railway` area | Physical footprint and size | Infrastructure |
| Platforms (`railway=platform`, `public_transport=platform`) | Platform count and numbers (`ref`), where people board | Infrastructure |
| `public_transport=stop_position` node | Where a train halts on the track. Route relations list these as their `stop` members | Link |
| `stop_area` relation | Groups one station's node, platforms and stop positions. Usually one per mode or operator | Link |
| `stop_area_group` relation | Groups several stop areas into one interchange | Link |
| Route relation members (roles `stop`, `platform`) | Which lines call here, in what order | Service |

The hard part is coverage. There are 18,157 `stop_area` relations against 97,053 stations and 34,452 halts, so at most about 14% of rail stations have one. For most stations the grouping has to be inferred, not read.

## Model

```
                Complex  (the place a traveller names; one label at low zoom)
                from stop_area_group, else inferred from shared wikidata, name or building
                  |               |               |
        Station: mainline   Station: metro   Station: tram     (one per mode; own lines and click panel)
        stop_area, else station node
                  |               |               |
        Stop points: platforms and stop positions   (never drawn at low zoom)
        each belongs to one station; routes list them as stop / platform members
              |                                   |
   Infrastructure layer                      Service layer
   platforms, tracks, building, land         route stop members, route_master (OSM only)
   -> the size score: the ONLY ranking signal -> lines for the click panel, not ranking
```

## How to build it

Run after the infrastructure build, since stations attach to track and land areas. Every grouping records how it was made (`mapped` or `inferred`) so QA can review the inferred ones.

1. **Stations from stop areas.** Each `stop_area` becomes a Station. Its members give the node, platforms and stop positions.
2. **Stations without a stop area.** Each remaining station or halt node becomes a Station. Attach nearby platforms and stop positions of the same mode whose name matches, or that sit inside the same `landuse=railway` area. Starting radius about 300 m for mainline and 100 m for metro and tram, tuned on the pilot regions.
3. **Merge duplicates.** Two Stations of the same mode become one when they share a `wikidata` or `uic_ref` AND lie within 1 km, or have the same normalised name within the radius. `railway:ref` is not a key (it is a per-network abbreviation: `Ch` is Chociszew, Ciechanów and Chybie), and ids copied between far-apart objects never merge across the country (Poland, Russia, Hungary, Greece). A station polygon with no stop area (usually a building also tagged `railway=station`) joins the nearest same-mode station within 100 m of it, preferring one with platforms, when the two names are equal once normalised (`駅`/`站` suffix, `JR` prefix, `Bahnhof`, `Stazione di` dropped) or the polygon is unnamed; a differently named polygon (New York "Pennsylvania Station") joins only the single station with platforms inside it. The key merge reaches 3 km instead of 1 km when the normalised names are equal. This fixes the untagged third object at Gare de Lyon.

   **Second dot of a station (2026-10-17, `stations/` step 3b).** After the merge above, merged stations of one mode within 100 m (point to point) become one when (a) their names are equal once the decoration is off: operator initials from the `operator`/`network` tags (`Neckarsulm AVG` beside `Neckarsulm`), the words `Hp`, `Bf`, `Hbf`, `Transit Center`, `Kleinbahn`, `Landesbahn`, `Fermata`, the place before an ASCII hyphen (`Berlin-Köpenick`, `München-Pasing`), the place that an `official_name` puts in front (`Berlin Jungfernheide` for `Jungfernheide`), a gloss in a second script behind a CJK name; or (b) one of them is a single feature or two with no stop area of its own and repeats the other's name with a Japanese operator in front (`阪急西院駅`, `ＪＲ北海道　森駅`, `津軽鉄道 五所川原駅`, `JR 亀崎駅`) or a Chinese station type or exit behind (`诏安站站房`, `寿县高铁站`, `科宁路地铁站3号口`, `小杉駅南口`); or (c) one is nameless and the other named, within 60 m (nearest named one only). Rules (b) and (c) do not merge when both carry a `wikidata` and they differ; rule (a) does (DB and S-Bahn Jungfernheide carry different Q ids). Two stations that each have a stop area and differ by an operator (`京阪石山` beside `石山`, `近鉄名古屋` beside `名鉄名古屋`), a direction (`红安`/`红安西`, `临高`/`临高南`), a line-and-street name (`34th Street–Herald Square` beside `PATH 33rd Street`, `Lexington Avenue–59th Street` beside `59th Street`, the en dash is not a hyphen) or a place word (`Paignton Queens Park`) stay two. Different modes never merge here (Gare de Lyon train and metro stay separate stations in one complex).

   **Station vs building.** The station tags (`railway=station`/`halt`/`tram_stop`, `public_transport=station`) are tested before `building=train_station` and `landuse=railway`; a way that is both is a Station and also stays in the building / land lists for the size score.

   **Mode.** Mode comes from the mode tags (`station=`, `subway=yes`, `light_rail=yes`, `train=yes`, `tram_stop`). With none, it is guessed from the nearest track within 150 m, and then replaced by the modes of the station's own mapped platforms when it has any (Basel SBB and Düsseldorf Hbf are main-line stations, not the tram in front). A platform's own mode is the nearest track within 25 m, except that a surface platform (no `tunnel`/`layer<0`) with a surface heavy-rail track beside it and only metro or light-rail tunnel track otherwise near is a train platform (Changsha South, New Delhi: the metro runs under the station).

   **Name.** A Station is labelled with its own `name` (best-named feature, points first), not the stop_area's; the stop_area name is used only when the station has none, when it is the own name with a place in front, or when the own name is a bare `Station`. Cleaning (`clean_name`): the first item of a `;` list (`黄福柵原駅;黄福柵原` is `黄福柵原駅`), a trailing `[...]` tag (`[Aug 2026]`, `[Nowa]`), a trailing `（在建）` / `(建设中)` / `(under construction)` / `(en construcción)` suffix, whitespace collapsed (the ideographic space too); always a leading `Bf. `/`Bhf. `/`Hp. `, a trailing `, Bahnhof`, the `City, U/S/S+U ` form; only when the bare name is a station within 300 m, a trailing bracket that names a line, mode or operator (`(National Rail)`, `(Green line)`, `(東武東上線)`, `(A,C,E)`) and the Korean type words 기차역/기차. Place and direction brackets (`(Cheshire)`, `(West)`, `(Beverly Hills)`) are kept. `name_en` is cleaned the same way. Names are never invented. English names come from `rules::english_name_kv`, which skips the placeholders `待定`, `TBD` and `unknown`.

   **Bare names.** A name that is only `Hauptbahnhof`, `Bahnhof`, `Gare`, `Stazione` or `Station` is qualified with a place when the build has one: `addr:city` / `addr:place` on the station or its stop area, else the place in front of the same word in the nearest station name within 800 m (`Bremen Hbf` gives `Bremen Hauptbahnhof`; the VBB form `Berlin, S Hauptbahnhof` gives `Berlin`; only for `Hauptbahnhof` and `Bahnhof`). No place nodes exist at this stage (`places` runs after the station model), so a bare name with no such neighbour stays as it is (Berlin's Parkeisenbahn `Hauptbahnhof`). A bare `Station` is never kept: it falls back to the stop area's name or another feature's name, else the station has no name.

   **English name.** `name_en` is built from the label's own source: when the label is in Latin script, only from the features and stop areas that carry that same name (`Dadar` no longer gets the stop area's `Dadar (Central)`). A label in another script still takes the station's other `name:*` tags, since that is where its English comes from (`北京` gets `Beijing Railway Station` from its stop area).

   **Under construction.** A station tagged `railway=station`/`halt` or `public_transport=station` is not an open station when `construction` is `yes` or `station`, when `construction:railway` or `construction:public_transport` is present, or when its name ends in `（在建）`, `(建设中)`, `(under construction)` and the like (`陶然桥(在建)`). The dashed construction track stays; `construction.rs` draws the station as one being built when it qualifies as one of its nodes.
4. **Lines at each station.** For every route relation, map each `stop` and `platform` member to its Station through the step 1–3 membership table. Count distinct `route_master`s, not relations. A stop that maps to no Station goes to the nearest Station of the same mode with a matching name, or is reported in QA.
5. **Complexes.** Each `stop_area_group` becomes a Complex. Otherwise group Stations of different modes that share a `wikidata`, or share a normalised name within about 300 m, or sit in the same station building. Distance alone never groups: that is how Magenta borrowed Gare du Nord's lines in the Paris test.
6. **Points.** A Station sits at its station node, else the centroid of its platforms. A Complex sits at its largest Station.
7. **Size score.** Rank by physical size only: mainline platforms (distinct platform numbers: `1a`/`1b` count as platform 1; platform ways without a ref count one each), mainline tracks (within 20 m of the platforms; within 80 m of the station point when no platform has a heavy-rail or heritage track beside it) ending in or passing through the station, building and railway land area. Metro and tram platforms don't count, so a heavily interlined subway hub can't outrank a mainline terminal. A terminus flag (mainline track ends inside the station's railway land) adds weight. The score becomes tiers for zoom and label order. Line counts stay in the record for the click panel but never set rank, since services are under-mapped in China and inflated by interlining in places like New York. Weights and tier cut-offs are tuned on the answer lists. Major stations (name pattern, canonical list, or a terminus or large mainline station beside high-speed track; see `00-overview.md`) are then raised to tier 1.

Check it on the three pilot regions (SF Bay Area, Chengdu, Paris) against a short hand-made answer list (e.g. Gare du Nord: one complex holding mainline, RER and metro stations; Chengdu East: top tier) before any larger run.

## Schema

Ids reuse OSM where it has one, so they survive monthly rebuilds.

| Record | Id | Key fields |
| --- | --- | --- |
| **Complex** | `stop_area_group` id, else the id of its largest Station | name, names, point, station ids, wikidata, grouping (`mapped` / `inferred`) |
| **Station** | `stop_area` id, else the station node id | complex id, name, names, modes, operator, network, point, uic_ref, railway_ref, wikidata, platform count, size score and tier, terminus flag, line ids (panel only), grouping, source OSM ids |
| **Stop point** | OSM id of the platform or stop position | station id, kind (platform / stop position), platform ref, mode, route ids that use it |

Size and rank come only from the infrastructure layer. Lines come from the service layer through stop points and only fill the click panel, so a Station with no mapped services ranks the same. That is the normal case for intercity China.

## Output

| Output | Contents | Zooms |
| --- | --- | --- |
| `complexes` tile layer | One point per Complex: name, English name, size tier, modes, terminus, platform count, line count | Labels from z5 by tier |
| `stations` tile layer | One point per Station: id, name, English name, modes, size tier, terminus, platform count, line count by type (for tooltips) | From z10, or z8 for top tiers |
| `platforms` tile layer | Platform areas and lines with ref | From z14 |
| `stations/<id>.json` | Full record: lines in order, platforms, names, ids, links | Loaded on click |

Every view sorts labels by the size tier through MapLibre's `symbol-sort-key`.

## Changes of 2026-10-17: label pass, second dots, stations under construction

Measured on cloned inputs of the current `data/world` (same inputs before and after; the scratch build writes nothing to `data/world`, `app/public` or `world.pmtiles`). Station counts include the stations `construction.rs` adds.

| Country | Stations before | after | Duplicate pairs (same mode, within 100 m) before | after | Groups merged by step 3b |
| --- | --- | --- | --- | --- | --- |
| jp | 8,882 | 8,857 | 84 | 57 | 24 |
| fr | 4,000 | 3,996 | 13 | 10 | 3 |
| de | 9,107 | 9,082 | 54 | 31 | 24 |
| it | 3,908 | 3,902 | 28 | 23 | 4 |
| us | 4,898 | 4,893 | 97 | 92 | 5 |
| gb | 3,694 | 3,687 | 31 | 28 | 3 |
| cn | 18,734 | 18,428 | 59 | 16 | 30 |

Most of the pairs left are not one station: operator-prefixed Japanese interchange stations with their own stop area (`石山` / `京阪石山`), exits and platform floors mapped as stations, fantasy and miniature railways in the US and Germany, and places that sit 60 to 100 m apart under different names. China lost 355 open stations to the construction rule (158 `construction=yes|station`, 51 `construction:railway`, 146 name suffix); `construction.rs` re-adds 49 of them as stations being built. Elsewhere: th 41, fr 48, kr 12, it 8, de 5, gb 5, au 5, paris 5 (the th and fr ones are mostly `construction:railway` and show up as stations being built).

Label scan over de jp gb us fr cn pl (before, after): semicolon lists 3, 0; `[...]` tags 4, 1 (a `construction.rs` station); double or ideographic spaces 2, 0; `待定` 2, 0; `Hp. ` 55, 0 (`Hp Haus am See` without the dot is left); nameless 411, 368. Names of stations drawn by `construction.rs` (grouping `construction`) still carry `（在建）` or `[Nowa]`: that file builds its own names.
