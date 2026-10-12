"""Phase 1 filter: one region extract -> features, relations side table, stop positions, QA report.

Implements docs/03-data-spec.md (v3). Usage: python3 filter.py <slug>
Reads  ../data/<slug>.osm.pbf
Writes ../data/<slug>.rail.osm.pbf        (prefilter: only rail, station, route and airport objects, ~3% of the extract)
       ../data/<slug>/features.geojsonl, relations.jsonl, stop_positions.jsonl, qa.json

Two steps: `osmium tags-filter` cuts the extract down, then the C++ program in cpp/filter.cpp (libosmium) does the
filtering. The old pure-Python version is kept as filter_py.py for checking the two agree.
"""
import json
import subprocess
import sys
import time
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
DATA = HERE.parent / "data"
CPP = HERE / "cpp"

MODES = "train,subway,light_rail,tram,monorail,funicular"
# everything filter.cpp or airports.py can keep; referenced nodes and ways come along automatically
PREFILTER = ["nwr/railway", "nwr/public_transport", "nwr/landuse=railway", "nwr/building=train_station",
             f"r/route={MODES},railway", f"r/route_master={MODES}", "nwr/aeroway=aerodrome,terminal",
             "n/construction:railway", "n/construction:public_transport"]


def build_binary():
    src, exe = CPP / "filter.cpp", CPP / "rail_filter"
    if exe.exists() and exe.stat().st_mtime >= src.stat().st_mtime:
        return exe
    inc = subprocess.run(["brew", "--prefix"], capture_output=True, text=True).stdout.strip() or "/opt/homebrew"
    print("compiling cpp/filter.cpp (needs libosmium and protozero: brew install libosmium protozero)")
    subprocess.run(["clang++", "-std=c++17", "-O2", f"-I{inc}/include", "-o", str(exe), str(src), f"-L{inc}/lib",
                    "-lz", "-lbz2", "-lexpat", "-lpthread"], check=True)
    return exe


def main(slug):
    src = DATA / f"{slug}.osm.pbf"
    pre = DATA / f"{slug}.rail.osm.pbf"
    out = DATA / slug
    out.mkdir(exist_ok=True)

    t0 = time.time()
    subprocess.run(["osmium", "tags-filter", "-o", str(pre), "--overwrite", str(src)] + PREFILTER, check=True)
    t1 = time.time()
    print(f"prefilter: {src.stat().st_size/1e6:.0f} MB -> {pre.stat().st_size/1e6:.1f} MB in {t1-t0:.1f} s")
    subprocess.run([str(build_binary()), str(pre), str(out)], check=True)
    print(f"filter (C++): {time.time()-t1:.1f} s")

    meta = json.load(open(out / "filter_meta.json"))
    way_ids = set(meta["way_ids"])
    kept_areas = set(meta["used_areas"])
    stations = meta["stations"]
    all_rows = [json.loads(l) for l in open(out / "relations_all.jsonl")]

    # bus-only stop areas are dropped: keep a stop_area only if a kept feature or stop position belongs to it,
    # and a group only if it holds a kept stop_area
    rows = [r for r in all_rows if r["kind"] in ("route", "route_master", "railway_line") or r["id"] in kept_areas]
    rows += [r for r in all_rows if r["kind"] == "stop_area_group"
             and any(f"{m['type']}/{m['id']}" in kept_areas for m in r["members"])]
    with open(out / "relations.jsonl", "w") as f:
        for row in rows:
            f.write(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n")
    kinds = Counter(r["kind"] for r in rows)
    broken = []
    for r in rows:
        if r["kind"] != "route":
            continue
        ways = [m["id"] for m in r["members"] if m["type"] == "way" and m["role"] in ("", "route")]
        miss = sum(1 for w in ways if w not in way_ids)
        if not ways or miss:
            broken.append({"id": r["id"], "name": r["tags"].get("name"), "ways": len(ways), "missing": miss})
    broken.sort(key=lambda b: -b["missing"])
    orphans = [s for s in stations if not s[2]]
    qa = {
        "region": slug,
        "feature_counts": dict(sorted(meta["counts"].items())),
        "relation_counts": dict(kinds),
        "broken_routes": {"count": len(broken), "of": kinds["route"], "worst": broken[:25]},
        "stations_total": len(stations),
        "stations_without_stop_area": len(orphans),
        "orphan_station_sample": [{"osm_id": o[0], "name": o[1]} for o in orphans[:25]],
        "stations_without_name": sum(1 for s in stations if not s[1]),
    }
    json.dump(qa, open(out / "qa.json", "w"), indent=2, ensure_ascii=False)
    print(json.dumps(qa, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main(sys.argv[1])
