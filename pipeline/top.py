"""Dev helper: show the top stations of a region by size score. Usage, from the repo root: python3 pipeline/top.py <slug> [n]  (pilots read data/<slug>; world slugs read data/world/<slug>)"""
import json, sys
from pathlib import Path
slug, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 25
root = Path(__file__).resolve().parent.parent / "data"
d = root / slug if (root / slug).is_dir() else root / "world" / slug
rows = [json.loads(l)["properties"] for l in open(d / "stations.geojsonl")]
rows.sort(key=lambda p: -p["score"])
for p in rows[:n]:
    print(f'{p["score"]:6.1f} T{p["tier"]} {"TERM" if p["terminus"] else "    "} pl={p["mainline_platforms"]:3d}/{p["platform_count"]:3d} trk={p["mainline_track_ways"]:3d} land={p["land_ha"]:5.1f} bld={p["building_ha"]:5.2f} lines={p["line_count"]:2d} {",".join(p["modes"])} | {p["name"]} / {p["name_en"]}')
