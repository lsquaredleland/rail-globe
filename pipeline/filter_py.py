"""Phase 1 filter: one region extract -> features, relations side table, stop positions, QA report.

Implements docs/03-data-spec.md (v3). Usage: python3 filter.py <slug>
Reads  ../data/<slug>.osm.pbf
Writes ../data/<slug>/features.geojsonl, relations.jsonl, stop_positions.jsonl, qa.json
"""
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

import osmium

DATA = Path(__file__).resolve().parent.parent / "data"

TRACK = {"rail", "subway", "light_rail", "tram", "monorail", "funicular", "narrow_gauge", "construction", "proposed"}
SERVICE_KEEP = {"yard", "siding", "spur", "crossover"}
MODE_FLAGS = ("train", "subway", "light_rail", "tram", "monorail")
# Tags copied onto features (data spec, "Tags kept")
KEEP = [
    "railway", "public_transport", "building", "landuse", "station", "service", "usage", "railway:traffic_mode",
    "highspeed", "name", "official_name", "int_name", "ref", "wikidata", "uic_ref", "railway:ref",
    "operator", "operator:short", "network", "gauge", "electrified", "voltage", "frequency", "tracks",
    "maxspeed", "maxspeed:freight", "construction", "proposed", "bridge", "tunnel", "light_rail", "subway", "train", "start_date",
]
REL_TAGS = ["name", "ref", "colour", "operator", "network", "route", "service", "from", "to", "wikidata", "public_transport", "type", "route_master"]
REL_KINDS = {"stop_area", "stop_area_group"}
ROUTE_MODES = {"train", "subway", "light_rail", "tram", "monorail", "funicular"}


def rnd(c):
    if isinstance(c, (list, tuple)):
        return [rnd(x) for x in c]
    return round(c, 5)


def kept_tags(tags):
    out = {k: tags[k] for k in KEEP if k in tags}
    for tag in tags:
        if tag.k.startswith("name:"):
            out[tag.k] = tag.v
    return out


class Relations(osmium.SimpleHandler):
    """Pass 1: relation side table and the member lookups the feature pass needs."""

    def __init__(self):
        super().__init__()
        self.rows = []
        self.way_routes = defaultdict(set)      # way id -> route relation ids
        self.route_operator = {}                # route relation id -> operator
        self.member_area = {}                   # (type, id) -> stop_area id
        self.route_member_refs = {}             # route id -> [(type, id)] for QA

    def relation(self, r):
        t = r.tags
        typ, route, ptype = t.get("type"), t.get("route"), t.get("public_transport")
        if typ == "public_transport" and ptype == "stop_area":
            kind = "stop_area"
        elif typ == "public_transport" and ptype == "stop_area_group":
            kind = "stop_area_group"
        elif typ == "route" and route in ROUTE_MODES:
            kind = "route"
        elif typ == "route_master" and t.get("route_master") in ROUTE_MODES:
            kind = "route_master"
        elif typ == "route" and route == "railway":
            kind = "railway_line"  # named infrastructure line, not a service
        else:
            return
        rid = f"relation/{r.id}"
        tags = {k: t[k] for k in REL_TAGS if k in t}
        for tag in t:
            if tag.k.startswith("name:"):
                tags[tag.k] = tag.v
        members = [{"type": {"n": "node", "w": "way", "r": "relation"}[m.type], "id": m.ref, "role": m.role} for m in r.members
                   if not (kind == "railway_line" and m.type == "w")]
        self.rows.append({"id": rid, "kind": kind, "tags": tags, "members": members})
        if kind == "route":
            self.route_operator[rid] = t.get("operator")
            self.route_member_refs[rid] = [(m["type"], m["id"], m["role"]) for m in members]
            for m in members:
                if m["type"] == "way" and m["role"] in ("", "route"):
                    self.way_routes[m["id"]].add(rid)
        elif kind == "stop_area":
            for m in members:
                self.member_area[(m["type"], m["id"])] = rid


class Features(osmium.SimpleHandler):
    """Pass 2: geometry features, stop positions, QA counters."""

    def __init__(self, rel, feat_out, stop_out):
        super().__init__()
        self.rel = rel
        self.feat_out = feat_out
        self.stop_out = stop_out
        self.wkb = osmium.geom.GeoJSONFactory()
        self.counts = Counter()
        self.stations = []          # (osm_id, name, has_stop_area)
        self.way_ids = set()        # ways emitted as features, for the route QA
        self.used_areas = set()     # stop_area ids touched by a kept feature or stop position

    # ------------------------------------------------------------------ helpers
    def emit(self, osm_id, tags, geom_json, kind_count):
        props = kept_tags(tags)
        props["osm_id"] = osm_id
        typ, _, num = osm_id.partition("/")
        if typ == "way":
            self.way_ids.add(int(num))
        routes = self.rel.way_routes.get(int(num), ()) if typ == "way" else ()
        if routes:
            props["_routes"] = sorted(routes)
            ops = sorted({self.rel.route_operator[r] for r in routes if self.rel.route_operator.get(r)})
            if ops:
                props["_operators"] = ops
        sa = self.rel.member_area.get((typ, int(num)))
        if sa and (props.get("railway") in ("station", "halt", "platform") or props.get("public_transport") in ("station", "platform")):
            props["_stop_area"] = sa
            self.used_areas.add(sa)
        geom = json.loads(geom_json)
        geom["coordinates"] = rnd(geom["coordinates"])
        self.feat_out.write(json.dumps({"type": "Feature", "properties": props, "geometry": geom}, ensure_ascii=False, separators=(",", ":")) + "\n")
        self.counts[kind_count] += 1
        return props

    @staticmethod
    def is_passenger_mode(tags):
        return any(tags.get(m) == "yes" for m in MODE_FLAGS)

    @staticmethod
    def classify_poly(tags):
        """Return a count label if this tag set is an area we keep, else None."""
        if tags.get("landuse") == "railway":
            return "area:landuse_railway"
        if tags.get("railway") == "yard":
            return "area:yard"
        if tags.get("building") == "train_station":
            return "area:station_building"
        if tags.get("public_transport") == "station" and ("railway" in tags or Features.is_passenger_mode(tags)):
            return "area:station"
        if tags.get("railway") in ("station", "halt"):
            return "area:station"
        if tags.get("railway") == "platform" or (tags.get("public_transport") == "platform" and Features.is_passenger_mode(tags)):
            return "area:platform"
        return None

    # ------------------------------------------------------------------ nodes
    def node(self, n):
        t = n.tags
        if not t:
            return
        if "entrance" in t:
            return
        if t.get("public_transport") == "stop_position" or t.get("railway") == "stop":
            if self.is_passenger_mode(t) or t.get("railway") == "stop":
                modes = {m: t[m] for m in MODE_FLAGS if m in t}
                row = {"osm_id": f"node/{n.id}", "lon": round(n.location.lon, 5), "lat": round(n.location.lat, 5),
                       "name": t.get("name"), "modes": modes, "ref": t.get("ref")}
                self.stop_out.write(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n")
                sa = self.rel.member_area.get(("node", n.id))
                if sa:
                    self.used_areas.add(sa)
                self.counts["stop_position"] += 1
            return
        if t.get("railway") in ("station", "halt") or (t.get("public_transport") == "station" and (t.get("railway") or self.is_passenger_mode(t))):
            if t.get("railway") in ("abandoned", "disused", "proposed"):
                return
            props = self.emit(f"node/{n.id}", t, self.wkb.create_point(n), "station:" + (t.get("railway") or "pt"))
            self.stations.append((props["osm_id"], t.get("name"), "_stop_area" in props))

    # ------------------------------------------------------------------ ways
    def way(self, w):
        t = w.tags
        if not t or "entrance" in t:
            return
        rw = t.get("railway")
        if rw == "platform_edge":
            return
        if rw in TRACK:
            svc = t.get("service")
            try:
                g = self.wkb.create_linestring(w)
            except osmium.InvalidLocationError:
                self.counts["dropped:bad_geometry"] += 1
                return
            self.emit(f"way/{w.id}", t, g, f"track:{rw}" + (f":{svc}" if svc in SERVICE_KEEP else ""))
            return
        if rw in ("siding", "spur", "crossover"):
            try:
                g = self.wkb.create_linestring(w)
            except osmium.InvalidLocationError:
                return
            self.emit(f"way/{w.id}", t, g, f"track:{rw}")
            return
        label = self.classify_poly(t)
        if not label:
            return
        closed = w.is_closed() and len(w.nodes) >= 4
        try:
            if closed and (label != "area:platform" or t.get("area") == "yes") and t.get("area") != "no":
                ring = [[n.lon, n.lat] for n in w.nodes]
                geom = json.dumps({"type": "Polygon", "coordinates": [ring]})
            elif label == "area:platform":
                geom = self.wkb.create_linestring(w)
                label = "platform:line"
            else:
                return
        except osmium.InvalidLocationError:
            return
        self.emit(f"way/{w.id}", t, geom, label)

    # ------------------------------------------------------------------ areas from relations
    def area(self, a):
        if a.from_way():
            return
        t = a.tags
        label = self.classify_poly(t)
        if not label:
            return
        try:
            g = self.wkb.create_multipolygon(a)
        except Exception:
            self.counts["dropped:bad_multipolygon"] += 1
            return
        self.emit(f"relation/{a.orig_id()}", t, g, label)


def main(slug):
    src = DATA / f"{slug}.osm.pbf"
    out = DATA / slug
    out.mkdir(exist_ok=True)

    rel = Relations()
    rel.apply_file(str(src))
    with open(out / "features.geojsonl", "w") as ff, open(out / "stop_positions.jsonl", "w") as sf:
        feat = Features(rel, ff, sf)
        feat.apply_file(str(src), locations=True)

    # bus-only stop areas are dropped: keep a stop_area only if a kept feature or stop position belongs to it,
    # and a group only if it holds a kept stop_area
    kept_areas = feat.used_areas
    rows = [r for r in rel.rows if r["kind"] in ("route", "route_master", "railway_line") or r["id"] in kept_areas]
    rows += [r for r in rel.rows if r["kind"] == "stop_area_group"
             and any(f"{m['type']}/{m['id']}" in kept_areas for m in r["members"])]
    with open(out / "relations.jsonl", "w") as f:
        for row in rows:
            f.write(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n")
    rel.rows = rows
    kinds = Counter(r["kind"] for r in rel.rows)
    # QA: routes with no way members / empty, stations outside any stop_area
    # a route is broken when some of its way members are not in the kept data (outside the region, or dropped)
    broken = []
    for r in rel.rows:
        if r["kind"] != "route":
            continue
        ways = [m["id"] for m in r["members"] if m["type"] == "way" and m["role"] in ("", "route")]
        miss = sum(1 for w in ways if w not in feat.way_ids)
        if not ways or miss:
            broken.append({"id": r["id"], "name": r["tags"].get("name"), "ways": len(ways), "missing": miss})
    broken.sort(key=lambda b: -b["missing"])
    orphans = [s for s in feat.stations if not s[2]]
    qa = {
        "region": slug,
        "feature_counts": dict(sorted(feat.counts.items())),
        "relation_counts": dict(kinds),
        "broken_routes": {"count": len(broken), "of": kinds["route"], "worst": broken[:25]},
        "stations_total": len(feat.stations),
        "stations_without_stop_area": len(orphans),
        "orphan_station_sample": [{"osm_id": o[0], "name": o[1]} for o in orphans[:25]],
        "stations_without_name": sum(1 for s in feat.stations if not s[1]),
    }
    json.dump(qa, open(out / "qa.json", "w"), indent=2, ensure_ascii=False)
    print(json.dumps(qa, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main(sys.argv[1])
