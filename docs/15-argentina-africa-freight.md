# Freight or passenger in Argentina and Africa (study and rule, 2026-10-16)

Same question as `docs/13-freight-north-america.md`, for a part of the world where most track is freight and there is no BTS-style passenger inventory to score against. The method is the same (passenger needs evidence), the evidence differs by how well each country's passenger services are mapped.

## What the data looks like

Before this change 97% or more of the track in Argentina and in every African country with rail was typed Intercity or Regional (typed from `usage` alone, with no route relation): 98,500 km of "passenger" track in the 41 countries of the table below, against a real passenger network of perhaps a third of that. Operator tags do not help: they are mostly absent in Africa (Tanzania: 3,750 of 4,035 km have none), and in Argentina they name the old freight concessions (Ferrocarriles Argentinos 7,850 km, FEPSA 2,700, Nuevo Central Argentino 2,600, Ferro Sur Roca 1,700). Stations do not help in Argentina: a station sits within 200 m of 8,200 of its 19,100 km, freight lines included.

What does exist is route relations, and they are good where it matters:
- **Argentina**: 59 lines, every real Trenes Argentinos service (the Buenos Aires lines, long distance on the Roca to Mar del Plata and on the Mitre, San Martin and Sarmiento, Tren de las Sierras, Chaco, Salta, Tren del Valle), Tren Patagonico and the metros. 4,300 km of route against 19,100 km typed passenger.
- **Africa**: South Africa 61 lines (Shosholoza Meyl, Metrorail), Kenya (Madaraka Express, Nairobi commuter), Nigeria (Lagos-Ibadan, Abuja-Kaduna, Port Harcourt-Kano), Algeria 37 (SNTF), Ethiopia (Addis-Djibouti, AA LRT), Zimbabwe, Namibia, Morocco (98% of typed track is routed: ONCF is nearly all passenger).
- **Thin**: Tanzania (2 lines; TAZARA and the Central Line are not mapped as routes), Sudan (3), Egypt (6 lines, 41% of typed km routed), Zambia (2).

## The rule

`passenger_needs_evidence` (config key, already used for the US, Canada and Mexico), now with two modes. Main-line track typed from its tags alone (no route, not an exemption) is Freight unless:
- **`"routes"`** (Argentina and the countries with good route mapping): it runs beside passenger-routed track (25 m, half its length) or its operator is a passenger-only agency (Trenes Argentinos, Ferrovias, Metrovias, Tren de la Costa, Metrorail, Shosholoza Meyl, Gautrain). The national incumbents (NRZ, Kenya Railways, NRC, ENR) are not on the list because they run freight too.
- **`"stations"`** (Egypt, Tanzania, Sudan, Zambia, Senegal, Ghana, Gabon, Malawi, Mali, Uganda, Guinea, Eritrea: the countries where routed km are under half of the km with a station beside): the same, or a station within 200 m of the way. Here a bare `railway=station` or `halt` counts as a passenger station (in these countries stations are mapped without `public_transport` or `train` tags, so the tags find almost none). Chosen because "routed km / km with a route or a station" is 17% for Tanzania, 3% to 6% for Sudan, 49% for Egypt, against 57% to 100% elsewhere.

Config without a file per country: `pipeline/config/_groups.json` lists the slugs of each group; a region with no `<slug>.json` whose slug is listed uses that group's template (`_evidence-routes.json`, `_evidence-stations.json`, both `"extends": "_default"`), otherwise `_default.json` as before. 41 countries use it. Exemptions (`docs/12`) still apply on top.

## Result (passenger-typed km, world build 2026-10-16)

| Country | before | after | mode |
| --- | --- | --- | --- |
| Argentina | 19,106 | 4,325 | routes |
| South Africa | 18,869 | 8,824 | routes |
| Algeria | 6,419 | 4,509 | routes |
| Egypt | 4,804 | 4,056 | stations |
| DR Congo | 4,139 | 2,855 | routes |
| Tanzania | 4,035 | 2,267 | stations |
| Nigeria | 3,922 | 1,901 | routes |
| Sudan | 3,615 | 1,953 | stations |
| Morocco | 3,206 | 3,142 | routes |
| Namibia | 2,621 | 1,155 | routes |
| Kenya | 2,465 | 1,189 | routes |
| Zambia | 2,086 | 1,744 | stations |
| All 41 countries | 98,463 | 54,657 | |

Liberia, Niger, South Sudan and Eswatini now have no passenger track at all and drop out of the world build (142 countries, was 146): their rail is iron-ore or freight lines with no passenger route and no station. `world.pmtiles` rebuilt; the pilot regions are byte-identical.

## Side effects to know

- In `"stations"` countries a bare `railway=station` counts as a passenger station everywhere in the classifier, so the older branch rules (no passenger station on a dead-end branch, short spurs) also demote less there. Station areas tagged `station=freight` are not excluded (`cut.rs`, as before).
- A station within 200 m keeps any way near it, also a freight line that crosses another line's station.
- Short ways between demoted neighbours can be smoothed to Freight again ("neighbour" source); not measured for Tanzania.

## What is not verified

There is no ground truth here: the figures say how much track has evidence, not how much is truly passenger. Spot checks only: Argentina's 35 passenger relations are all the real services; South Africa's and Kenya's relations match the real networks. Probable errors:
- **Too little passenger**: Tanzania (TAZARA and the Central Line have no route relation; the station mode keeps 2,267 of 4,035 km), Sudan, Nigeria's narrow-gauge Lagos-Kano line beyond what is routed, Zambia's Mulobezi line (tourist).
- **Too much passenger** in "stations" countries: freight lines with named stations (Egypt's phosphate lines, Guinea) stay passenger.
- Mauritania (SNIM ore railway, a free passenger train) and Angola (Benguela and Luanda passenger lines) are as routed.
Next step to close this: a short list of real services per country (as for the Americas in `docs/07`), checked on screenshots, and `exemptions` where a service is missing.

## Reproduce

```
python3 pipeline/qa/study_ways.py ar za tz ...     # per-way tables (km, route, beside, station) into data/qa/freight-study
rail-extract cut --regions data/regions --data data/world <slugs>
rail-extract world-list --data data/world --slugs-file data/world/slugs.txt
```

## Added 2026-10-17: Brazil, Chile, Colombia, Uruguay, Venezuela, Dominican Republic

User decision after the Americas QA (`docs/qa/americas-africa-asia.md`): the same "routes" mode applies to these six. Each has its own `pipeline/config/<slug>.json` extending `_evidence-routes` (they are not in `_groups.json`), with stat names set explicitly and the exemptions listed there. Passenger-typed open way-km before and after (scratch cut, not yet in the world build): Brazil 14,044 to 4,001, Chile 3,701 to 1,143, Colombia 1,561 to 729 (after the Turistren exemptions), Uruguay 1,391 to 118, Venezuela 380 to 38, Dominican Republic 238 to 0. Uruguay has no running passenger service to preserve (the Montevideo-Progreso service is planned, not running). Peru, Bolivia and Cuba keep the old default. The world build needs a re-cut of the six countries to show it.
