"""Phase 3 build: one region -> PMTiles + side files for the app.

Usage: python3 build_tiles.py <slug>
Reads  ../data/<slug>/{features.typed.geojsonl, stations.geojsonl, complexes.geojsonl, lines.json, route_stations.json, relations.jsonl}
Writes ../app/public/data/<slug>.pmtiles
       ../app/public/data/<slug>/{lines-index.json, lines/<id>.json, stations/<id>.json}
       ../app/public/data/<slug>.boundary.geojson

Tile layers (docs/02-architecture.md): track, lines (one strand per line per way), areas, buildings,
platforms, stations, complexes. Tiles include each class one zoom before the style shows it.
"""
import json
import math
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

from shapely.geometry import Point, shape, mapping
from shapely.strtree import STRtree

from classify import speed_kmh
from names import english_name

HERE = Path(__file__).resolve().parent
DATA = HERE.parent / "data"
OUT = HERE.parent / "app" / "public" / "data"

TYPE_RANK = {t: i for i, t in enumerate(["high_speed", "intercity", "regional", "metro", "light_rail", "monorail", "funicular", "other"])}
# tile minzoom per track type (style shows it one zoom later)
TRACK_MINZOOM = {"high_speed": 0, "intercity": 3, "regional": 5, "metro": 7, "light_rail": 8, "monorail": 8, "funicular": 8,
                 "other": 8, "freight": 7, "yard": 12}
STATION_MINZOOM = {1: 5, 2: 7, 3: 9}
COMPLEX_MINZOOM = {1: 5, 2: 8, 3: 10}
CSS_NAMES = {"red", "blue", "green", "yellow", "orange", "purple", "pink", "brown", "black", "grey", "gray", "darkblue", "skyblue",
             "lightblue", "darkgreen", "lightgreen", "cyan", "magenta", "violet", "gold", "navy", "teal", "maroon", "lime",
             "olive", "silver", "turquoise", "crimson", "indigo", "darkred", "darkorange", "white"}


def colour(c):
    if not c:
        return None
    c = c.strip().lower()
    if re.fullmatch(r"#([0-9a-f]{3}|[0-9a-f]{6})", c):
        return c
    if re.fullmatch(r"([0-9a-f]{6})", c):
        return "#" + c
    return c if c in CSS_NAMES else None


def label_of(l):
    """Short on-map name of a line: the name before any ': from ↔ to' suffix, else its ref."""
    n = re.split(r"\s*[:：]\s*", l.get("name") or "")[0].strip()
    return (n or l.get("ref") or "")[:30]


def area_ha(g):
    """Polygon area in hectares from lon/lat (local equirectangular scale)."""
    s = shape(g)
    lat = s.centroid.y
    return round(s.area * 111320 * math.cos(math.radians(lat)) * 110540 / 10000, 2)


def short(rid):
    return int(rid.split("/")[1])


def haversine_len(coords):
    t = 0.0
    for (x1, y1), (x2, y2) in zip(coords, coords[1:]):
        dx = math.radians(x2 - x1) * math.cos(math.radians((y1 + y2) / 2))
        dy = math.radians(y2 - y1)
        t += math.hypot(dx, dy) * 6371.0088
    return t


def jl(path, rows):
    with open(path, "w") as f:
        for r in rows:
            f.write(json.dumps(r, ensure_ascii=False, separators=(",", ":")) + "\n")


def safe(i):
    return i.replace("/", "-")


def feat(props, geom, minzoom=None, maxzoom=None):
    f = {"type": "Feature", "properties": props, "geometry": geom}
    tp = {}
    if minzoom is not None:
        tp["minzoom"] = minzoom
    if maxzoom is not None:
        tp["maxzoom"] = maxzoom
    if tp:
        f["tippecanoe"] = tp
    return f


def main(slug):
    src = DATA / slug
    tmp = src / "tiles_tmp"
    tmp.mkdir(exist_ok=True)
    OUT.mkdir(parents=True, exist_ok=True)
    side = OUT / slug
    for d in ("lines", "stations"):
        (side / d).mkdir(parents=True, exist_ok=True)

    lines = {l["id"]: l for l in json.load(open(src / "lines.json"))}
    route_line = {r: l["id"] for l in lines.values() for r in l["routes"]}
    feats = [json.loads(l) for l in open(src / "features.typed.geojsonl")]
    stations = [json.loads(l) for l in open(src / "stations.geojsonl")]
    complexes = [json.loads(l) for l in open(src / "complexes.geojsonl")]
    rels = {}
    for l in open(src / "relations.jsonl"):
        r = json.loads(l)
        rels[r["id"]] = r
    route_order = json.load(open(src / "route_stations.json"))

    # ------------------------------------------------------------ track + strands
    track, strands, areas, buildings, plats = [], [], [], [], []
    way_len = {}
    named = {}   # (name, type) -> aggregate of unrouted, named passenger track
    for f in feats:
        p, g = f["properties"], f["geometry"]
        t = p.get("type")
        if g["type"] == "LineString" and t:
            if p["osm_id"].startswith("way/"):
                way_len[int(p["osm_id"].split("/")[1])] = haversine_len(g["coordinates"])
            prop = p.get("railway") == "proposed"
            con = p.get("railway") == "construction" or prop   # proposed track rides on the construction layers, flagged `prop`
            lids = sorted({route_line[r] for r in p.get("_routes", ()) if r in route_line},
                          key=lambda l: (TYPE_RANK.get(lines[l]["type"], 9), short(l)))
            props = {"id": p["osm_id"], "type": t}
            for k_in, k_out in (("usage", "usage"), ("electrified", "el"), ("voltage", "v"), ("maxspeed", "ms"), ("gauge", "g"),
                                ("tracks", "tr"), ("name", "n"), ("frequency", "fq")):
                if p.get(k_in):
                    props[k_out] = p[k_in]
            kmh = speed_kmh(p.get("maxspeed"))
            if kmh:
                props["kmh"] = round(kmh)
            en = english_name(p) if p.get("name") else None
            if en and en != p.get("name"):
                props["ne"] = en
            if p.get("service"):
                props["sv"] = p["service"]
            if p.get("tunnel") and p["tunnel"] != "no":
                props["tun"] = 1
            if p.get("bridge") and p["bridge"] != "no":
                props["br"] = 1
            if con:
                props["con"] = 1
            if prop:
                props["prop"] = 1
            if lids and not con:
                props["ln"] = ",".join(str(short(l)) for l in lids)
                props["nl"] = len(lids)
                # one colour for the track below z12: first line (most prominent type) that has a colour
                for l in lids:
                    c = colour(lines[l].get("colour"))
                    if c:
                        props["oc"] = c
                        break
            mz = TRACK_MINZOOM.get(t, 8)
            if p.get("name") and not lids and not con and t not in ("yard", "freight"):
                tr = p.get("tracks", "")
                div = min(4, int(tr)) if tr.isdigit() and int(tr) > 0 else 1
                a = named.setdefault((p["name"], t), {"n": p["name"], "ne": props.get("ne"), "t": t, "km": 0.0, "ways": 0, "bbox": [180, 90, -180, -90]})
                a["km"] += haversine_len(g["coordinates"]) / div
                a["ways"] += 1
                for x, y in g["coordinates"]:
                    b = a["bbox"]
                    b[0], b[1], b[2], b[3] = min(b[0], x), min(b[1], y), max(b[2], x), max(b[3], y)
            track.append(feat(props, g, mz))
            if lids and not con and t != "yard":
                n = len(lids)
                for i, l in enumerate(lids):
                    c = colour(lines[l].get("colour"))
                    sp = {"lid": short(l), "t": lines[l]["type"], "i": i, "n": n, "lb": label_of(lines[l])}
                    if lines[l].get("name_en"):
                        sp["lbe"] = label_of({"name": lines[l]["name_en"], "ref": lines[l].get("ref")})
                    if c:
                        sp["c"] = c
                    strands.append(feat(sp, g, 11))
        elif g["type"] in ("Polygon", "MultiPolygon"):
            ap = {"id": p["osm_id"], "ha": area_ha(g)}
            if p.get("name"):
                ap["n"] = p["name"]
            if p.get("operator"):
                ap["op"] = p["operator"]
            if p.get("landuse") == "railway":
                areas.append(feat(dict(ap, k="land"), g, 10))
            elif p.get("railway") == "yard":
                areas.append(feat(dict(ap, k="yard"), g, 10))
            elif p.get("building") == "train_station":
                buildings.append(feat(dict(ap, k="building"), g, 12))
            elif p.get("railway") == "platform" or p.get("public_transport") == "platform":
                plats.append(feat(dict(ap, k="platform", ref=p.get("ref", "")), g, 13))
        elif g["type"] == "LineString" and (p.get("railway") == "platform" or p.get("public_transport") == "platform"):
            plats.append(feat({"id": p["osm_id"], "ref": p.get("ref", ""), "k": "platform"}, g, 13))

    # ------------------------------------------------------------ airports (outline + terminals) and stations that serve them
    airports = []
    airport_pts = []
    aero_geoms, aero_props = [], []
    ap_file = src / "airports.geojsonl"
    if ap_file.exists():
        for line in open(ap_file):
            f = json.loads(line)
            p, g = f["properties"], f["geometry"]
            lb = p.get("iata") or p.get("name") or ""
            props = {"id": p["id"], "k": p["k"], "lb": lb, "ha": area_ha(g)}
            for k_in, k_out in (("name", "n"), ("name_en", "ne"), ("iata", "iata"), ("icao", "icao"), ("operator", "op")):
                if p.get(k_in):
                    props[k_out] = p[k_in]
            if p["k"] == "aerodrome":
                mil = p.get("military") or "military" in (p.get("atype") or "")
                if p.get("iata") and not mil and props["ha"] >= 300:
                    props["com"] = 1
                    # one icon point per commercial airport, so tile boundaries cannot repeat it
                    airport_pts.append(feat({"id": p["id"], "lb": lb}, mapping(shape(g).representative_point()), 8))
                airports.append(feat(props, g, 8 if p.get("iata") else 10))
                # stations get the airport icon only at commercial airports: an IATA code, not military, and 300 ha or more
                mil = p.get("military") or "military" in (p.get("atype") or "")
                if p.get("iata") and not mil and props["ha"] >= 300:
                    aero_geoms.append(shape(g).buffer(0))
                    aero_props.append(p)
            else:
                airports.append(feat(props, g, 12))
    aero_tree = STRtree(aero_geoms) if aero_geoms else None
    AIRPORT_WORDS = re.compile(r"airport|a[ée]roport|flughafen|aeropuerto|机场|航站楼", re.I)
    BUFFER_DEG = 0.003  # about 300 m: stations sit at the terminal edge, often just outside the mapped aerodrome

    def airport_of(station):
        """Short airport label for a station inside (or at the edge of) an aerodrome, or whose name says airport."""
        pt = Point(station["geometry"]["coordinates"])
        if aero_tree is not None:
            for i in aero_tree.query(pt.buffer(BUFFER_DEG)):
                if aero_geoms[i].distance(pt) <= BUFFER_DEG:
                    return aero_props[i].get("iata") or aero_props[i].get("name") or "Airport"
        nm = " ".join(filter(None, (station["properties"].get("name"), station["properties"].get("name_en"))))
        return "Airport" if AIRPORT_WORDS.search(nm) else None

    # ------------------------------------------------------------ stations / complexes
    st_rows, cx_rows = [], []
    # rail types serving each station (from its lines and from track beside it), so hiding a type can hide its stations
    MODE_TYPES = {"train": {"high_speed", "intercity", "regional"}, "subway": {"metro"}, "light_rail": {"light_rail"},
                  "tram": {"light_rail"}, "monorail": {"monorail"}, "funicular": {"funicular"}, "other": {"other"}}
    pax = [(shape(r["geometry"]), r["properties"]["type"]) for r in track
           if r["properties"]["type"] in TYPE_RANK and "con" not in r["properties"]]
    pax_tree = STRtree([g for g, _ in pax])
    NEAR = 0.0007  # about 70 m
    cx_types = defaultdict(set)

    def station_types(s, p):
        if p.get("construction"):
            return set(p["rail_types"])
        ty = set(p["line_counts"])
        allowed = set().union(*(MODE_TYPES.get(m, set()) for m in p["modes"])) if p["modes"] else set()
        pt = shape(s["geometry"])
        for i in pax_tree.query(pt.buffer(NEAR)):
            if pax[i][1] in allowed and pax[i][0].distance(pt) <= NEAR:
                ty.add(pax[i][1])
        return ty

    for s in stations:
        p = s["properties"]
        ty = station_types(s, p)
        cx_types[p["complex"]] |= ty
        lct = ",".join(f"{k}:{v}" for k, v in sorted(p["line_counts"].items(), key=lambda kv: TYPE_RANK.get(kv[0], 9)))
        st_rows.append(feat({
            "id": p["id"], "cx": p["complex"], "n": p["name"] or "", "ne": p["name_en"] or "", "tier": p["tier"],
            "term": int(p["terminus"]), "hsr": int(bool(p["hsr"]) or p["line_counts"].get("high_speed", 0) > 0), "mp": p["mainline_platforms"], "pc": p["platform_count"],
            "lc": p["line_count"], "lct": lct, "ty": "," + ",".join(sorted(ty)) + "," if ty else "", "modes": ",".join(p["modes"]),
            **({"ap": ap} if (ap := airport_of(s)) else {}),
            **({"con": 1} if p.get("construction") else {}),
        }, s["geometry"], STATION_MINZOOM[p["tier"]]))
    for c in complexes:
        p = c["properties"]
        cx_rows.append(feat({"id": p["id"], "n": p["name"] or "", "ne": p["name_en"] or "", "tier": p["tier"],
                             "term": int(p["terminus"]), "ns": p["station_count"], **({"con": 1} if p.get("construction") else {}),
                             "ty": "," + ",".join(sorted(cx_types[p["id"]])) + "," if cx_types[p["id"]] else ""}, c["geometry"], COMPLEX_MINZOOM[p["tier"]]))

    layers = {"track": track, "lines": strands, "areas": areas, "buildings": buildings, "platforms": plats,
              "stations": st_rows, "complexes": cx_rows, "airports": airports, "airport_pts": airport_pts}
    args = []
    for name, rows in layers.items():
        jl(tmp / f"{name}.geojsonl", rows)
        args += ["-L", f"{name}:{tmp / (name + '.geojsonl')}"]
    pm = OUT / f"{slug}.pmtiles"
    cmd = ["tippecanoe", "-o", str(pm), "-f", "-Z0", "-z14", "--no-tile-size-limit", "--no-feature-limit",
           "--drop-rate=0", "--simplification=2", "--read-parallel", "-q"] + args
    subprocess.run(cmd, check=True)
    print(f"{slug}: {pm.stat().st_size/1e6:.2f} MB; " + ", ".join(f"{k}={len(v)}" for k, v in layers.items()))

    # ------------------------------------------------------------ side files
    idx = {}
    for l in lines.values():
        km = 0.0
        for rid in l["routes"]:
            r = rels.get(rid)
            if r:
                km = max(km, sum(way_len.get(m["id"], 0) for m in r["members"] if m["type"] == "way" and m["role"] in ("", "route")))
        idx[short(l["id"])] = {"n": l["name"], "r": l["ref"], "c": colour(l["colour"]), "t": l["type"], "o": l["operator"],
                               "f": l["from"], "to": l["to"], "km": round(km, 1), "ne": l.get("name_en")}
    json.dump(idx, open(side / "lines-index.json", "w"), ensure_ascii=False, separators=(",", ":"))
    nl = sorted((dict(v, km=round(v["km"], 1), bbox=[round(x, 4) for x in v["bbox"]]) for v in named.values() if v["km"] >= 1.5),
                key=lambda v: -v["km"])
    json.dump(nl, open(side / "named-lines.json", "w"), ensure_ascii=False, separators=(",", ":"))

    # raw (filtered) OSM tags plus Rail Atlas derived fields, sharded by id for the app's Inspect mode
    props_by_id = {f["properties"]["osm_id"]: f["properties"] for f in feats}
    shards = defaultdict(dict)
    for oid, pr in props_by_id.items():
        shards[int(oid.split("/")[1]) % 256][oid] = {k: v for k, v in pr.items() if k != "osm_id"}
    (side / "tags").mkdir(exist_ok=True)
    for i in range(256):
        json.dump(shards.get(i, {}), open(side / "tags" / f"{i}.json", "w"), ensure_ascii=False, separators=(",", ":"))

    st_by_id = {s["properties"]["id"]: s["properties"] for s in stations}
    cx_members = defaultdict(list)
    for s in stations:
        cx_members[s["properties"]["complex"]].append(s["properties"])

    for s in stations:
        p = s["properties"]
        groups = defaultdict(list)
        for lid in p["line_ids"]:
            if lid in lines:
                groups[lines[lid]["type"]].append(short(lid))
        other_modes = [{"id": o["id"], "name": o["name"], "name_en": o["name_en"], "modes": o["modes"]}
                       for o in cx_members[p["complex"]] if o["id"] != p["id"]]
        cx_name = next((c["properties"]["name"] for c in complexes if c["properties"]["id"] == p["complex"]), None)
        rec = {
            "id": p["id"], "name": p["name"], "name_en": p["name_en"], "tier": p["tier"], "terminus": p["terminus"],
            "hsr": bool(p["hsr"]) or p["line_counts"].get("high_speed", 0) > 0, "modes": p["modes"], "platforms": p["mainline_platforms"], "platform_count": p["platform_count"],
            "platform_refs": p["platform_refs"], "platform_refs_other": p["platform_refs_other"],
            "rail_lines": p.get("rail_lines", []),
            "line_count": p["line_count"], "lines": {t: sorted(v) for t, v in sorted(groups.items(), key=lambda kv: TYPE_RANK.get(kv[0], 9))},
            "complex": {"id": p["complex"], "name": cx_name, "others": other_modes} if other_modes else None,
            "construction": bool(p.get("construction")), "airport": airport_of(s), "osm": p["source_ids"][0] if p["source_ids"] else None, "wikidata": p["wikidata"], "pt": s["geometry"]["coordinates"],
            "tags": [{"id": i, "tags": {k: v for k, v in props_by_id[i].items() if k != "osm_id"}} for i in p["source_ids"] if i in props_by_id]
                    + [{"id": i, "tags": rels[i]["tags"]} for i in p["source_ids"] if i in rels],
            "model": {k: p[k] for k in ("score", "tier", "major", "terminus", "dead_ends", "mainline_platforms", "platform_count",
                                        "mainline_track_ways", "land_ha", "building_ha", "grouping", "hsr")},
        }
        json.dump(rec, open(side / "stations" / f"{safe(p['id'])}.json", "w"), ensure_ascii=False, separators=(",", ":"))

    for lid, l in lines.items():
        routes = []
        for rid in l["routes"]:
            r = rels.get(rid)
            if not r:
                continue
            wl = sum(way_len.get(m["id"], 0) for m in r["members"] if m["type"] == "way" and m["role"] in ("", "route"))
            order = route_order.get(rid, [])
            routes.append({"id": rid, "name": r["tags"].get("name"), "from": r["tags"].get("from"), "to": r["tags"].get("to"),
                           "km": round(wl, 1), "stations": order})
        routes.sort(key=lambda r: (-len(r["stations"]), -r["km"]))
        used = {sid for r in routes[:8] for sid in r["stations"]}
        names = {sid: {"n": st_by_id[sid]["name"], "ne": st_by_id[sid]["name_en"]} for sid in used if sid in st_by_id}
        rec = {
            "id": short(lid), "name": l["name"], "ref": l["ref"], "type": l["type"], "colour": colour(l["colour"]),
            "operator": l["operator"], "network": l["network"], "from": l["from"], "to": l["to"],
            "km": max((r["km"] for r in routes), default=0), "osm": lid,
            "routes": routes[:8], "station_names": names, "tags": l.get("tags") or {}, "name_en": l.get("name_en"),
            "route_ids": l["routes"][:30],
        }
        json.dump(rec, open(side / "lines" / f"{short(lid)}.json", "w"), ensure_ascii=False, separators=(",", ":"))

    # boundary (simplified) for the faint region outline
    b = json.load(open(HERE / "regions" / f"{slug}.geojson"))
    g = shape(b["geometry"]).simplify(0.0008)
    json.dump({"type": "Feature", "properties": {"slug": slug}, "geometry": mapping(g)}, open(OUT / f"{slug}.boundary.geojson", "w"), separators=(",", ":"))
    print(f"{slug}: side files {len(stations)} stations, {len(lines)} lines, {len(nl)} named railway lines")


if __name__ == "__main__":
    main(sys.argv[1])
