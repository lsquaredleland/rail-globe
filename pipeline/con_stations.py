"""Phase 3b: stations that are still being built.

Usage: python3 con_stations.py <slug>      (run after stations.py)
Reads  ../data/<slug>/features.typed.geojsonl (nodes labelled station:construction by the filter), stations.geojsonl
Appends one station and one complex per under-construction station to stations.geojsonl and complexes.geojsonl,
marked `construction: true`. Safe to run twice (earlier appended rows are dropped first).

Rule (docs/00-overview.md): a named node is an under-construction station if it has railway=construction with
construction=station|halt, or construction:railway=station|halt|stop, or construction:public_transport=station|stop_position.
Nodes with the same name within 400 m are one station (a stop usually has a node per platform edge). A station is skipped when an
open station of the same name already stands within 600 m. There is no physical size yet, so the tier is inferred from what is being built around it (see TIER RULE below).

TIER RULE (provisional, docs/00-overview.md). A station with mainline modes (not subway or light rail) is
  hsr      if high-speed track under construction runs within 200 m (the track's own tags), or its name says 高铁站 / high-speed railway
  major    if hsr and at least 4 pieces of mainline track under construction lie within 200 m, or its name matches the region's main_station_re
  tier 1 = major, tier 2 = hsr but not major, tier 3 = everything else (metro, light rail, plain mainline).
Platforms and land do not exist yet, so track pieces stand in for them; line counts are never used.
"""
import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

from shapely.geometry import Point, shape
from shapely.strtree import STRtree

from names import english_name

HERE = Path(__file__).resolve().parent

DATA = Path(__file__).resolve().parent.parent / "data"
CLUSTER_DEG = 0.0037    # about 400 m
OPEN_DEG = 0.0055       # about 600 m
TRACK_DEG = 0.0015      # about 150 m
TIER_DEG = 0.0018       # about 200 m
MAINLINE_TYPES = {"high_speed", "intercity", "regional"}
MAJOR_TRACKS = 4
MODE_TYPES = {"train": {"high_speed", "intercity", "regional"}, "subway": {"metro"}, "light_rail": {"light_rail"}, "tram": {"light_rail"}, "monorail": {"monorail"}, "funicular": {"funicular"}, "other": {"other"}}
DEFAULT_TYPE = {"train": "regional", "subway": "metro", "light_rail": "light_rail", "tram": "light_rail", "monorail": "monorail", "funicular": "funicular", "other": "other"}
MODE_OF_TRACK = {"subway": "subway", "light_rail": "light_rail", "tram": "tram", "monorail": "monorail", "funicular": "funicular", "rail": "train"}


def norm(name):
    n = re.sub(r"\s+", "", (name or "").lower())
    return re.sub(r"(站|station)$", "", n)


def is_con(p):
    return (p.get("railway") == "construction" and p.get("construction") in ("station", "halt")) or \
        p.get("construction:railway") in ("station", "halt", "stop") or \
        p.get("construction:public_transport") in ("station", "stop_position")


def main(slug):
    out = DATA / slug
    cfg = json.load(open(HERE / "config" / f"{slug}.json"))
    main_re = re.compile(cfg["main_station_re"]) if cfg.get("main_station_re") else None
    feats = [json.loads(l) for l in open(out / "features.typed.geojsonl")]
    st_rows = [json.loads(l) for l in open(out / "stations.geojsonl")]
    cx_rows = [json.loads(l) for l in open(out / "complexes.geojsonl")]
    st_rows = [r for r in st_rows if not r["properties"].get("construction")]
    cx_rows = [r for r in cx_rows if not r["properties"].get("construction")]

    nodes = [f for f in feats if f["geometry"]["type"] == "Point" and is_con(f["properties"]) and f["properties"].get("name")]
    con_tracks = [(shape(f["geometry"]), f["properties"]) for f in feats
                  if f["geometry"]["type"] == "LineString" and f["properties"].get("railway") == "construction"]
    track_tree = STRtree([g for g, _ in con_tracks]) if con_tracks else None
    open_pts = [(shape(r["geometry"]), norm(r["properties"]["name"])) for r in st_rows]
    open_tree = STRtree([g for g, _ in open_pts])

    # cluster nodes: same name, or one name ending with the other (深圳机场东 / 机场东), within 400 m
    pts = [shape(f["geometry"]) for f in nodes]
    nms = [norm(f["properties"]["name"]) for f in nodes]
    parent = list(range(len(nodes)))
    def find(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i
    for i in range(len(nodes)):
        for j in range(i + 1, len(nodes)):
            a_, b_ = nms[i], nms[j]
            compat = a_ == b_ or (min(len(a_), len(b_)) >= 3 and (a_.endswith(b_) or b_.endswith(a_)))
            if compat and pts[i].distance(pts[j]) <= CLUSTER_DEG:
                parent[find(i)] = find(j)
    groups = defaultdict(list)
    for i in range(len(nodes)):
        groups[find(i)].append(nodes[i])
    clusters = [(max((norm(f["properties"]["name"]) for f in g), key=len), g) for g in groups.values()]

    skipped = Counter()
    added = []
    for nm, fs in clusters:
        pts = [shape(f["geometry"]) for f in fs]
        cx, cy = sum(p.x for p in pts) / len(pts), sum(p.y for p in pts) / len(pts)
        c = Point(cx, cy)
        if any(open_pts[i][1] == nm for i in open_tree.query(c.buffer(OPEN_DEG)) if open_pts[i][0].distance(c) <= OPEN_DEG):
            skipped["already open"] += 1
            continue
        props = {}
        for f in fs:
            for k, v in f["properties"].items():
                props.setdefault(k, v)
        modes, hsr = set(), False
        for f in fs:
            for k, m in (("subway", "subway"), ("light_rail", "light_rail"), ("train", "train")):
                if f["properties"].get(k) == "yes":
                    modes.add(m)
        if not modes and track_tree is not None:
            best = None
            for i in track_tree.query(c.buffer(TRACK_DEG)):
                d = con_tracks[i][0].distance(c)
                if d <= TRACK_DEG and (best is None or d < best[0]):
                    best = (d, con_tracks[i][1])
            if best:
                modes.add(MODE_OF_TRACK.get(best[1].get("construction_type") or "rail", "train"))
                hsr = best[1].get("type") == "high_speed"
        if re.search(r"高铁|高速铁路|high.speed", (props.get("name") or "") + " " + (english_name(props) or ""), re.I):
            hsr, modes = True, {"train"}
        if not modes:
            modes.add("train")
        mainline = bool(modes & {"train"})
        hs_ways = main_ways = 0
        if track_tree is not None and mainline:
            for i in track_tree.query(c.buffer(TIER_DEG)):
                if con_tracks[i][0].distance(c) <= TIER_DEG:
                    t = con_tracks[i][1].get("type")
                    hs_ways += t == "high_speed"
                    main_ways += t in MAINLINE_TYPES
        label = (props.get("name") or "") + " " + (english_name(props) or "")
        hsr = hsr or (mainline and (hs_ways > 0 or bool(re.search(r"高铁|高速铁路|high.speed", label, re.I))))
        major = mainline and ((hsr and main_ways >= MAJOR_TRACKS) or bool(main_re and main_re.search(props.get("name") or "")))
        tier = 1 if major else 2 if (mainline and hsr) else 3
        # rail types this station belongs to, so hiding a type hides it: nearby construction track of a matching kind, else a default per mode
        allowed = set().union(*(MODE_TYPES.get(m, set()) for m in modes))
        rail_types = set()
        if track_tree is not None:
            for i in track_tree.query(c.buffer(TIER_DEG)):
                if con_tracks[i][0].distance(c) <= TIER_DEG and con_tracks[i][1].get("type") in allowed:
                    rail_types.add(con_tracks[i][1]["type"])
        if not rail_types:
            rail_types = {"high_speed"} if (hsr and mainline) else {DEFAULT_TYPE.get(m, "other") for m in modes}
        elif hsr and mainline:
            rail_types = {"high_speed"} | (rail_types & {"high_speed"})
        sid = sorted(f["properties"]["osm_id"] for f in fs)[0]
        pr = {
            "id": sid, "complex": sid, "name": props.get("name"), "name_en": english_name(props), "modes": sorted(modes), "tier": tier,
            "score": 0, "terminus": False, "mainline_platforms": 0, "platform_count": 0, "land_ha": 0, "building_ha": 0,
            "mainline_track_ways": 0, "dead_ends": 0, "hsr": hsr, "major": major, "platform_refs": [], "platform_refs_other": [],
            "line_count": 0, "line_counts": {}, "line_ids": [], "wikidata": [props["wikidata"]] if props.get("wikidata") else None,
            "grouping": "construction", "rail_types": sorted(rail_types), "source_ids": [f["properties"]["osm_id"] for f in fs], "construction": True,
        }
        geom = {"type": "Point", "coordinates": [round(cx, 5), round(cy, 5)]}
        added.append((pr, geom))

    with open(out / "stations.geojsonl", "w") as f:
        for r in st_rows:
            f.write(json.dumps(r, ensure_ascii=False, separators=(",", ":")) + "\n")
        for pr, geom in added:
            f.write(json.dumps({"type": "Feature", "properties": pr, "geometry": geom}, ensure_ascii=False, separators=(",", ":")) + "\n")
    with open(out / "complexes.geojsonl", "w") as f:
        for r in cx_rows:
            f.write(json.dumps(r, ensure_ascii=False, separators=(",", ":")) + "\n")
        for pr, geom in added:
            cp = {"id": pr["id"], "name": pr["name"], "name_en": pr["name_en"], "tier": pr["tier"], "terminus": False, "station_count": 1,
                  "modes": pr["modes"], "platform_count": 0, "line_count": 0, "grouping": "construction", "station_ids": [pr["id"]], "construction": True}
            f.write(json.dumps({"type": "Feature", "properties": cp, "geometry": geom}, ensure_ascii=False, separators=(",", ":")) + "\n")
    print(f"{slug}: {len(nodes)} construction station nodes -> {len(added)} stations added, skipped {dict(skipped)}, tiers {dict(sorted(Counter(pr['tier'] for pr, _ in added).items()))}")


if __name__ == "__main__":
    main(sys.argv[1])
