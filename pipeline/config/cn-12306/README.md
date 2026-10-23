# 12306 timetable inputs

Tracked inputs of `rail-extract cn12306` (China services from a static 12306 snapshot, `docs/00-overview.md` row 2026-10-20-china-12306-services).

| File | What it holds | Size |
| --- | --- | --- |
| `timetable.json` | 7,253 long-distance trains of two days: per train `code`, `class` (`G`, `D`, `Z`, `T`, `K`, `plain`, `C`, `S`) and `stops` as `[name, arrive, depart]` ("HH:MM", empty when 12306 gives none) | 2.6 MB |
| `stations.tsv` | 12306 station name to `lon`, `lat`, `source`, English name (tab-separated; a name with several rows has several candidate places) | 81 KB |
| `meta.json` | The days of the timetable: `snapshots`, one per day with `date`, `classes`, `trains`, `source`; the build writes `[{ date, classes }]` into `shards.json` | |
| `city_en.json` | English names of cities and stations (1,509 entries), from the corridor-v3 project; used after the 12306 list and OSM and before a name made from the city | 33 KB |
| `build_inputs.py` | Writes these files from china-rail and the corridor-v3 handoff | |

Source: 12306 (China Railway), harvested live by the `china-rail` project: the trunk trains (G, D, Z, T, K, plain; 5,043) for 2026-10-21 and the intercity trains (C, S; 2,210) for 2026-10-28, with full stop lists. The two days do not share a train number (checked; the script drops a repeat anyway). Credit 12306 as the timetable source wherever the services are shown, and show both days; OSM attribution is unchanged.

`stations.tsv` sources: `list` (china-rail's station list), `fix` and `fix+list` (a name whose first point put a stop pair more than 3x + 3 km off china-rail's own chord: moved to the known place of that name that fits its neighbours; 61 names), `resolution` (china-rail's pick between several places), `osm` (the only place of that name), `candidate` (several places and no pick: the build takes the one nearest the stops on both sides), `dropped` (no place fits; 13 names).

Refresh (needs the china-rail checkout, the corridor-v3 handoff `~/Downloads/china-rail-handoff-v3` for the intercity trains, its pair distances and station resolution, and the gazetteer, and `data/world/cn/stations.geojsonl` from a world build):

```
python3 pipeline/config/cn-12306/build_inputs.py [--china-rail DIR] [--timetable FILE] [--intercity FILE|none] [--handoff DIR] [--world DIR]
```

It takes the newest `deliverable/china-rail-timetables-*.json` and the newest `output/china-rail-intercity-*.json` of the handoff. The files are written in place; check the diff, then run `rail-extract cn12306` (`pipeline/README.md`).
