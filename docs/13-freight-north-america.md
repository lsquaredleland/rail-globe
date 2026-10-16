# Freight or passenger in the US, Canada and Mexico (study, 2026-10-16)

Question (user): in the US passenger rail is probably well labelled, so should anything without passenger service be freight? This file holds the measurement, the rule that came out of it and what it costs. Nothing here is in `classify.rs` yet: it is a proposal with numbers, to be switched on by a decision. Background and the older, reverted attempt: `docs/07-freight-detection.md` ("Americas: correct freight labelling"), `docs/08-americas-passenger-qa.md`.

## Answer

Yes. In the US and Canada, mainline track with no passenger evidence is freight, and the evidence is easy to state:

1. the way is on a passenger route relation (`route=train` and the metro, tram, ... relations: `type_source = route`), or
2. it runs beside such track (second tracks, crossovers, connectors: at least 50% of its length within 25 m of another way that is on a passenger route), or
3. its `operator` tag names a passenger agency (Amtrak, Metrolink, Metra, GO Transit, VIA Rail, ... list below).

Everything else typed Intercity, Regional or High-speed from `usage=main|branch` alone becomes Freight. Today it stays passenger by default, and that default is the whole problem.

## Measurement

Ground truth is the BTS North American Rail Network (NARN, public domain, `fetch_narn.py`): the 22,200 segments with a passenger code (A Amtrak, B, C commuter, D, E, I, R, V, O; T tourist excluded; NET X, A, R, T = out of service or abandoned excluded). For each OSM way, the share of its length within 30 m of NARN passenger track (samples every 20 m). Universe: Intercity, Regional, High-speed, Freight and Other ways (metro, light rail, monorail and funicular are not judged). Precision = NARN passenger km / km we call passenger; recall = NARN passenger km we call passenger / all NARN passenger km in the universe. Scripts: `pipeline/qa/study_ways.py` (per-way table, 145 s for the US), `pipeline/qa/eval_rules.py` (scores).

| Rule | US passenger km | Precision | Recall | Not-passenger km called passenger |
| --- | --- | --- | --- | --- |
| Current typing (Intercity, Regional, High-speed) | 222,892 | 26.0% | 98.2% | 165,009 |
| On a passenger route | 56,182 | 96.4% | 91.9% | 2,033 |
| Route, or beside routed track | 59,008 | 96.1% | 96.2% | 2,279 |
| ... or a passenger agency is the operator | **59,633** | **96.0%** | **97.1%** | 2,373 |
| ... or a train station within 200 m of the way | 60,609 | 94.6% | 97.3% | 3,271 |

| Rule | Canada passenger km | Precision | Recall | Not-passenger km called passenger |
| --- | --- | --- | --- | --- |
| Current typing | 38,119 | 45.7% | 99.1% | 20,699 |
| On a passenger route | 15,608 | 94.9% | 84.2% | 799 |
| Route, or beside routed track | 16,419 | 94.8% | 88.5% | 849 |
| ... or a passenger agency is the operator | **16,448** | **94.8%** | **88.7%** | 855 |
| ... or a train station within 200 m | 16,711 | 94.2% | 89.5% | 968 |

Reading:
- Today about three quarters of the US "passenger" track (165,000 of 223,000 km) and over half of Canada's (20,700 of 38,100 km) has no passenger service under it. The rule keeps 97% of the US passenger km and drops that.
- The beside rule adds 2,800 km (US) and 800 km (Canada) of real passenger track that no relation covers; the agency rule adds 600 km in the US (Metrolink's subdivisions, 500 km) and almost nothing in Canada. Both are cheap and safe.
- The beside threshold does not matter: 20% to 90% of the length changes precision by 0.5 points. The 25 m distance was not varied.
- A station within 200 m of the way costs 1.4 points of precision for 0.2 points of recall: not worth it as a way-level rule. (The existing rule 6 uses stations to demote branches; that stays.)
- "A railway whose routed share is at least X% is passenger on all its ways" (gap filling by name) adds 0.4 points of recall and costs 4 to 10 points of precision: rejected.
- Station evidence was the older idea ("passenger needs a station"): US trackage rights (Amtrak over UP and BNSF) have none, which is why they were mislabelled in the first place.

### What the rule still gets wrong

Missed passenger km (NARN passenger, not kept): 1,700 in the US (after the agency rule), 2,000 in Canada.
- US: Amtrak over freight railroads with no route members in OSM (UP Beaumont 125 km, Dallas 100, Austin 43, Coast 28, BNSF Spokane 23, Seligman 19, CSX Trenton 20 and Boston 20, Northeast Corridor pieces 35), CN Chicago Subdivision 53, and 144 km of unnamed track. These need an `exemptions` entry each (about 400 km in all) or a better OSM route relation.
- Canada: Algoma Central (392 km), Ontario Northland Temagami, Newmarket and Ramore (570 km), Gaspé (195 km), QNS&L (136 km). NARN still codes them passenger; the services are suspended or are not public timetable trains. These are probably correct as Freight; treat the recall figure for Canada as about 95% once stale codes are set aside.

Called passenger without NARN support (2,300 km US, 850 km Canada): partly NARN gaps (UP Elko, Houston and Austin Subdivisions carry the California Zephyr, the Sunset Limited and the Texas Eagle; DART and TEXRail are real), partly tourist and heritage routes that are routed (Grand Canyon Railway 104 km, Durango and Silverton 64 km, Cape May Seashore Lines, St. Croix Valley). Tourist lines are an existing `track_type_overrides` job (type Other); the list in `_north-america.json` can grow.

### Mexico

No NARN. Today 15,400 km of Intercity and Regional; with the route/beside rule 1,350 km; Tren Maya is 1,640 km with `operator=Tren Maya` and is mostly not on route relations, so it needs the agency list (`Tren Maya`, `FONADIN`, `Suburbano`, `Interurbano`, `El Insurgente`, `Felipe Angeles`, `Chepe`). With those: **3,030 km passenger**, 14,650 km freight (Ferromex 7,100, CPKC 4,100, Ferrosur 1,300, Coahuila Durango 880, Ferroistmo 780). Open: Ferroistmo's Interoceanico passenger lines (Linea Z is routed, Linea K and FA are not) and Linea Morelos and Linea Juarez; decide by hand.

## Why this works in North America and not everywhere

OSM mappers in the US, Canada and Mexico map passenger services as route relations with operators (Amtrak, commuter agencies, VIA, GO), and freight lines as plain `usage=main|branch` track. That is the opposite of the situation in China, where passenger routes are rarely mapped (docs/07). The rule is only as good as route mapping: for any other country, first measure "share of passenger stations served by a mapped route" and "km of track on routes against km of the national passenger network" before using it. South America, Cuba and the Dominican Republic look like the same problem (freight drawn as passenger) but have thin route mapping, so the rule can drop real services; they need a per-country look (the list of real services in docs/07).

## Proposed implementation

In `classify.rs`, after rules 1 to 3 (tags, service track, route relations), behind a config key in `_north-america.json` (`"passenger_needs_evidence": true`; Mexico adds its agency names):

- A way whose type came from tags alone (`usage=main` or `branch`, no route) and whose type is Intercity, Regional or High-speed becomes Freight unless (a) at least 50% of its length is within 25 m of a way on a passenger route (the existing neighbour index in `classify.rs` can be reused; mind that two parallel freight tracks beside one passenger track are all "beside"), or (b) an operator tag matches `passenger_operator_re`.
- Exemptions (`docs/12-config-and-exemptions.md`) fix the 400 km of US trackage rights and anything found later; `expect_min` and stale reporting already guard them.
- High-speed stays judged by speed (rule 1), not by this rule.
- Rules 6 to 8 (branches, spurs, neighbours) run after it and keep working; the older `freight_name_re` and `freight_operator_exempt_re` become redundant for these countries and can stay.
- Pilot regions must stay byte-identical (the key is off elsewhere). Then re-cut `us`, `ca`, `mx`, rebuild, re-run `eval_rules.py` on the real output (expect the figures above), run the data checker, and look at the Plains, Texas, Northeast and Southern California views.
- Cost of a world rebuild: cut 7 s, side files 37 s, tiles about 1 minute. Tiles for the three countries get lighter (about 195,000 km of track moves from the passenger layers to Freight, hidden by default).

## Decisions for the user

1. Switch the rule on for US and Canada (and Mexico with the agency list)? It reverses the "assume passenger" default for these three countries only.
2. Canada's suspended services (Algoma, Ontario Northland, Gaspé): Freight (recommended) or exemptions to keep them as passenger.
3. Tourist railways: Other (current) or keep them with their routes.

## Reproduce

```
python3 -I pipeline/qa/fetch_narn.py                 # NARN and Amtrak stations into data/ext/ntad (1 min, network)
python3 pipeline/qa/study_ways.py us ca mx           # per-way tables into data/qa/freight-study (US 145 s; needs numpy, shapely 2)
python3 pipeline/qa/eval_rules.py us ca              # the tables above
```
