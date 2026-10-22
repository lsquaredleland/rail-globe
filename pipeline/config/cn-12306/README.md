# 12306 timetable inputs

Tracked inputs of `rail-extract cn12306` (China services from a static 12306 snapshot, `docs/00-overview.md` row 2026-10-20-china-12306-services).

| File | What it holds | Size |
| --- | --- | --- |
| `timetable.json` | One day of long-distance trains: per train `code`, `class` (`G`, `D`, `Z`, `T`, `K`, `plain`) and `stops` as `[name, arrive, depart]` ("HH:MM", empty when 12306 gives none) | 2.0 MB |
| `stations.tsv` | 12306 station name to `lon`, `lat`, `source` (tab-separated; a name with several rows has several candidate places) | 68 KB |
| `build_inputs.py` | Writes both files from china-rail | |

Source: 12306 (China Railway), harvested live on 2026-10-21 (5,043 trains with full stop lists) by the `china-rail` project. Credit 12306 as the timetable source wherever the services are shown; OSM attribution is unchanged.

`stations.tsv` sources: `list` (china-rail's station list), `fix` and `fix+list` (a name whose first point put a stop pair more than 3x + 3 km off china-rail's own chord: moved to the known place of that name that fits its neighbours; 61 names), `resolution` (china-rail's pick between several places), `osm` (the only place of that name), `candidate` (several places and no pick: the build takes the one nearest the stops on both sides), `dropped` (no place fits; 13 names).

Refresh (needs the china-rail checkout and `data/world/cn/stations.geojsonl` from a world build):

```
python3 pipeline/config/cn-12306/build_inputs.py [--china-rail DIR] [--timetable FILE] [--world DIR]
```

It takes the newest `deliverable/china-rail-timetables-*.json`. Both files are written in place; check the diff, then run `rail-extract cn12306` (`pipeline/README.md`).
