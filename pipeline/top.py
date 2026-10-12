"""Dev helper: show the top stations of a region by size score. Usage: python3 top.py <slug> [n]"""
import json, sys
slug, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 25
rows = [json.loads(l)["properties"] for l in open(f"../data/{slug}/stations.geojsonl")]
rows.sort(key=lambda p: -p["score"])
for p in rows[:n]:
    print(f'{p["score"]:6.1f} T{p["tier"]} {"TERM" if p["terminus"] else "    "} pl={p["mainline_platforms"]:3d}/{p["platform_count"]:3d} trk={p["mainline_track_ways"]:3d} land={p["land_ha"]:5.1f} bld={p["building_ha"]:5.2f} lines={p["line_count"]:2d} {",".join(p["modes"])} | {p["name"]} / {p["name_en"]}')
