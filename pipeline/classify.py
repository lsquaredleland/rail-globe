"""Phase 2a: rail type classification (docs/01-features.md, section 2).

Usage: python3 classify.py <slug>
Reads  ../data/<slug>/features.geojsonl, relations.jsonl and config/<slug>.json
Writes ../data/<slug>/features.typed.geojsonl  (adds `type`, `type_source`, `construction_type`)
       ../data/<slug>/lines.json                (one record per line: route_master, else route)
       ../data/<slug>/classify_report.json
"""
import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

from names import english_name

HERE = Path(__file__).resolve().parent
DATA = HERE.parent / "data"

# Most prominent first. Track takes the highest type that runs on it.
ORDER = ["high_speed", "intercity", "regional", "metro", "light_rail", "monorail", "funicular", "other"]  # regional includes commuter (merged 2026-10-12)
RANK = {t: i for i, t in enumerate(ORDER)}
ROUTE_MODE_TYPE = {"subway": "metro", "light_rail": "light_rail", "tram": "light_rail", "monorail": "monorail", "funicular": "funicular"}
RAILWAY_TYPE = {"subway": "metro", "light_rail": "light_rail", "tram": "light_rail", "monorail": "monorail", "funicular": "funicular", "narrow_gauge": "other"}
SERVICE_TYPE = {
    "high_speed": "high_speed", "long_distance": "intercity", "night": "intercity",
    "regional": "regional", "commuter": "regional", "tourism": "other",
}
HS_NAME = re.compile(r"高速[鐵铁]路|high[ -]speed rail|express rail link", re.I)
SERVICE_TRACK = {"yard", "siding", "spur", "crossover"}
FREIGHT_USAGE = {"industrial", "freight", "military", "farm"}


def speed_kmh(v):
    m = re.match(r"\s*(\d+)\s*(mph)?", v or "")
    if not m:
        return None
    s = int(m.group(1))
    return s * 1.609 if m.group(2) else s


def highest(types):
    types = [t for t in types if t]
    return min(types, key=RANK.get) if types else None


def matches(rule, tags):
    for k, v in rule.items():
        if k == "network":
            if tags.get("network") != v:
                return False
        elif k == "network_re":
            if not re.search(v, tags.get("network") or ""):
                return False
        elif k == "name_re":
            if not re.search(v, tags.get("name") or ""):
                return False
        elif k == "operator_re":
            if not re.search(v, tags.get("operator") or ""):
                return False
        elif k == "osm_id":
            if tags.get("osm_id") not in ([v] if isinstance(v, str) else v):
                return False
        else:
            raise ValueError(f"unknown match key {k}")
    return True


def route_type(tags, cfg, track_hs):
    """Type of one route relation. Returns (type, source). First matching rule wins."""
    for o in cfg.get("type_overrides", []):
        if matches(o["match"], tags):
            return o["type"], "override"
    mode = tags.get("route")
    if mode in ROUTE_MODE_TYPE:
        return ROUTE_MODE_TYPE[mode], "mode"
    # route=train from here
    net = tags.get("network") or ""
    svc = tags.get("service")
    # commuter is merged into regional for now, so the per-region commuter_networks lists are not used
    if svc in SERVICE_TYPE:
        return SERVICE_TYPE[svc], "service"
    is_hs_net = any(re.search(p, net) for p in cfg.get("high_speed_networks_re", []))
    if svc in ("national", "international"):
        return ("high_speed" if is_hs_net else "intercity"), "service+network"
    if is_hs_net:
        return "high_speed", "network"
    if cfg.get("china") and re.match(r"^C\d", tags.get("ref") or ""):
        return "intercity", "train_number"
    if track_hs:
        return "high_speed", "track"
    return "regional", "default"


def own_tag_type(p, cfg):
    """Track type from its own tags only. Returns (type, source) or (None, None) for freight/unknown."""
    rw = p.get("railway")
    if rw in ("construction", "proposed"):
        rw = p.get(rw)  # the mode lives in construction=* / proposed=*
        if rw is None:
            # no mode given: rail-like tags mean standard rail, otherwise an unknown grey-dashed line
            if not (p.get("usage") or p.get("electrified") or p.get("gauge")):
                return "other", "default"
            rw = "rail"
    if rw in RAILWAY_TYPE:
        return RAILWAY_TYPE[rw], "tags"
    if rw not in ("rail", "narrow_gauge"):
        return "other", "default"
    if p.get("usage") == "tourism":
        return "other", "tags"
    ms = speed_kmh(p.get("maxspeed"))
    name = p.get("name") or ""
    if cfg.get("china"):
        # per-region exceptions: named 200 km/h lines that are still counted as high-speed (see docs/00-overview.md)
        if any(matches(r, p) for r in cfg.get("high_speed_track_overrides", [])):
            return "high_speed", "override"
        # the name check also covers traditional characters and English names, e.g. Hong Kong's
        # 廣深港高速鐵路 Guangzhou-Shenzhen-Hong Kong High Speed Rail, whose speed limit is 200 km/h or less inside Hong Kong
        if (ms and ms >= 250) or "高速线" in name or "客专" in name or "客运专线" in name or HS_NAME.search(name):
            return "high_speed", "tags"
        if p.get("highspeed") == "yes" or "城际" in name or (ms and ms >= 200):
            return "intercity", "tags"
    elif p.get("highspeed") == "yes" or (ms and ms >= 200):
        return "high_speed", "tags"
    usage = p.get("usage")
    if usage in FREIGHT_USAGE or p.get("railway:traffic_mode") == "freight":
        return "freight", "tags"
    if usage == "main":
        # per-region override: in the US most unrouted main line is freight, not Intercity
        return cfg.get("unrouted_main_type", "intercity"), "tags"
    if usage == "branch":
        return "regional", "tags"
    return "regional", "default"


def proposed_area_test(cfg):
    """Returns a test for 'is this geometry inside the area where proposed track is kept'. Nothing is kept without a `proposed_region`."""
    ref = cfg.get("proposed_region")
    if not ref:
        return lambda g: False
    from shapely.geometry import Point, shape
    from shapely.prepared import prep
    area = prep(shape(json.load(open(HERE / "regions" / ref))[0]["geojson"]))
    return lambda g: area.contains(Point(*g["coordinates"][0][:2]))


def main(slug):
    out = DATA / slug
    cfg = json.load(open(HERE / "config" / f"{slug}.json"))
    rels = [json.loads(l) for l in open(out / "relations.jsonl")]
    routes = {r["id"]: r for r in rels if r["kind"] == "route"}
    masters = {r["id"]: r for r in rels if r["kind"] == "route_master"}
    route_master = {}
    for m in masters.values():
        for mem in m["members"]:
            if mem["type"] == "relation":
                route_master[f"relation/{mem['id']}"] = m["id"]

    # pass A: which routes run on track that is tagged high-speed (used only for routes with no service info)
    hs_routes = set()
    feats = [json.loads(l) for l in open(out / "features.geojsonl")]
    in_proposed_area = proposed_area_test(cfg)
    for f in feats:
        p = f["properties"]
        if p.get("highspeed") == "yes":
            hs_routes.update(p.get("_routes", ()))

    rtype = {}
    for rid, r in routes.items():
        rtype[rid] = route_type(r["tags"], cfg, rid in hs_routes)

    # lines: route_master where one exists, else the route. Master type = most prominent of its routes.
    lines = {}
    for rid, r in routes.items():
        lid = route_master.get(rid, rid)
        lines.setdefault(lid, []).append(rid)
    line_records = {}
    for lid, rids in lines.items():
        src = masters.get(lid) or routes[lid]
        t = highest([rtype[x][0] for x in rids])
        # a master-level override (e.g. BART) beats the member routes
        for o in cfg.get("type_overrides", []):
            if matches(o["match"], {**routes[rids[0]]["tags"], **src["tags"]}):
                t = o["type"]
                break
        first = routes[rids[0]]["tags"]
        tags = src["tags"]
        line_records[lid] = {
            "id": lid, "name": tags.get("name") or first.get("name"), "ref": tags.get("ref") or first.get("ref"),
            "name_en": english_name(tags) or english_name(first), "tags": tags,
            "colour": tags.get("colour") or first.get("colour"), "type": t,
            "type_source": rtype[rids[0]][1], "operator": tags.get("operator") or first.get("operator"),
            "network": tags.get("network") or first.get("network"), "from": first.get("from"), "to": first.get("to"),
            "routes": rids,
        }
    json.dump(list(line_records.values()), open(out / "lines.json", "w"), ensure_ascii=False)
    route_line_type = {rid: line_records[route_master.get(rid, rid)]["type"] for rid in routes}

    counts = Counter()
    sources = Counter()
    length_by_type = Counter()
    with open(out / "features.typed.geojsonl", "w") as fo:
        for f in feats:
            p = f["properties"]
            rw = p.get("railway")
            is_track = (rw in ("rail", "subway", "light_rail", "tram", "monorail", "funicular", "narrow_gauge", "construction", "proposed")
                        and f["geometry"]["type"] == "LineString")
            if is_track and rw == "proposed" and not in_proposed_area(f["geometry"]):
                is_track = False  # proposed track is only kept inside the region's `proposed_region` (Hong Kong for now)
            if is_track:
                own = None
                if p.get("service") in SERVICE_TRACK:
                    t, s = "yard", "service"
                else:
                    own, s = own_tag_type(p, cfg)
                    ptypes = [route_line_type[r] for r in p.get("_routes", ()) if r in route_line_type]
                    best = highest(ptypes + ([own] if own == "high_speed" else []))
                    if best:
                        t, s = best, "route"
                    else:
                        t = own
                if rw == "construction":
                    p["construction_type"] = p.get("construction")
                p["type"], p["type_source"] = t, s
                p["own_type"] = own if rw not in ("construction", "proposed") else None
                counts[t] += 1
                sources[(t, s)] += 1
            fo.write(json.dumps(f, ensure_ascii=False, separators=(",", ":")) + "\n")

    report = {
        "region": slug,
        "track_by_type": dict(counts.most_common()),
        "track_by_type_and_source": {f"{t}/{s}": n for (t, s), n in sorted(sources.items())},
        "lines": len(line_records),
        "lines_by_type": dict(Counter(l["type"] for l in line_records.values()).most_common()),
        "lines_by_type_source": dict(Counter(f"{l['type']}/{l['type_source']}" for l in line_records.values()).most_common()),
    }
    json.dump(report, open(out / "classify_report.json", "w"), indent=2, ensure_ascii=False)
    print(json.dumps(report, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main(sys.argv[1])
