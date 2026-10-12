"""Phase 2b: station model (docs/04-station-model.md).

Usage: python3 stations.py <slug>
Reads  ../data/<slug>/features.typed.geojsonl, relations.jsonl, stop_positions.jsonl, lines.json
Writes ../data/<slug>/stations.geojsonl, complexes.geojsonl, stations_report.json

Levels: Complex (the place a traveller names) > Station (one per mode group) > stop points.
Rank comes from physical size only. Lines are counted for the panel and never set rank.
"""
import json
import math
import re
import sys
import unicodedata
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np
import shapely
from shapely import STRtree
from shapely.geometry import Point, shape

HERE = Path(__file__).resolve().parent
DATA = HERE.parent / "data"

MODE_KEYS = {"subway": "subway", "light_rail": "light_rail", "train": "train", "monorail": "monorail", "funicular": "funicular"}
TRACK_MODE = {"metro": "subway", "light_rail": "light_rail", "tram": "tram", "monorail": "monorail", "funicular": "funicular", "other": "other"}
RADIUS = {"train": 300, "subway": 100, "light_rail": 100, "tram": 100, "monorail": 100, "funicular": 100, "other": 100}   # metres, tuned on the pilots
GROUP_RADIUS = 300
LAND_DISC = 500      # metres: railway land counts only near the station (landuse=railway polygons span whole corridors)
HS_NEAR = 80           # metres: "next to high-speed track"
HS_MAJOR_PLATFORMS = 8  # mainline platforms that make a station beside high-speed track major
TERMINUS_ZONE = 120  # metres around the station's mainline platforms
STOP_ROLES = ("stop", "platform")

# Size score weights (tuned on the answer lists; see answers/)
W = {"platform": 3.0, "track": 0.5, "track_cap": 40, "land": 3.0, "building": 4.0, "terminus": 10.0}
TIERS = [(60.0, 1), (25.0, 2)]      # score >= cut -> tier; otherwise tier 3

STOPWORDS = {"gare", "du", "de", "d", "la", "le", "les", "l", "des", "station", "paris", "stazione", "railway", "rer", "metro",
             "subway", "bart", "caltrain", "amtrak", "light", "rail", "vta", "muni", "st", "saint"}


def norm_name(name):
    if not name:
        return ""
    s = unicodedata.normalize("NFKD", name.lower())
    s = "".join(c for c in s if not unicodedata.combining(c))
    s = re.sub(r"\(.*?\)", " ", s)
    s = re.sub(r"[^\w一-鿿]+", " ", s)
    s = re.sub(r"(地铁站|火车站|客站|站)$", "", s.strip())
    toks = [t for t in s.split() if t not in STOPWORDS]
    return " ".join(toks)


def english_name(tags):
    if tags.get("name:en"):
        return tags["name:en"]
    if tags.get("int_name"):
        return tags["int_name"]
    for k, v in tags.items():
        if k.startswith("name:") and v and re.match(r"^[\x00-ɏḀ-ỿ]+$", v):
            return v
    return None


def ref_tokens(ref):
    return {t.strip() for t in re.split(r"[;,/]", ref or "") if t.strip()}


class UF:
    def __init__(self, n):
        self.p = list(range(n))

    def find(self, x):
        while self.p[x] != x:
            self.p[x] = self.p[self.p[x]]
            x = self.p[x]
        return x

    def union(self, a, b):
        a, b = self.find(a), self.find(b)
        if a != b:
            self.p[b] = a


def main(slug):
    out = DATA / slug
    feats = [json.loads(l) for l in open(out / "features.typed.geojsonl")]
    rels = [json.loads(l) for l in open(out / "relations.jsonl")]
    stops = [json.loads(l) for l in open(out / "stop_positions.jsonl")]
    lines = {l["id"]: l for l in json.load(open(out / "lines.json"))}

    # local metric projection
    lats = [f["geometry"]["coordinates"][1] for f in feats if f["geometry"]["type"] == "Point"]
    lat0 = float(np.mean(lats)) if lats else 40.0
    kx, ky = 111320.0 * math.cos(math.radians(lat0)), 110540.0

    def proj(g):
        return shapely.transform(g, lambda c: c * np.array([kx, ky]))

    def unproj(x, y):
        return [round(x / kx, 5), round(y / ky, 5)]

    # ---------------------------------------------------------------- sort features
    tracks, platforms, buildings, lands, station_feats = [], [], [], [], []
    for f in feats:
        p = f["properties"]
        g = proj(shape(f["geometry"]))
        if not g.is_valid:
            g = shapely.make_valid(g)
        if g.is_empty:
            continue
        rw, pt = p.get("railway"), p.get("public_transport")
        item = {"p": p, "g": g, "id": p["osm_id"]}
        if p.get("type") and f["geometry"]["type"] == "LineString" and p.get("type") not in ("yard",) and rw != "proposed":
            tracks.append(item)
        elif rw == "platform" or (pt == "platform"):
            platforms.append(item)
        elif p.get("building") == "train_station":
            buildings.append(item)
        elif p.get("landuse") == "railway":
            lands.append(item)
        elif rw in ("station", "halt", "tram_stop") or pt == "station":
            if rw in ("construction",):
                continue
            station_feats.append(item)

    track_tree = STRtree([t["g"] for t in tracks])
    land_tree = STRtree([x["g"] for x in lands])
    bld_tree = STRtree([x["g"] for x in buildings])

    def nearest_track_mode(g, r):
        idx = track_tree.query(g.buffer(r))
        best, bd = None, 1e9
        for i in idx:
            d = tracks[i]["g"].distance(g)
            if d < bd:
                best, bd = tracks[i], d
        if not best or bd > r:
            return None
        return TRACK_MODE.get(best["p"]["type"], "train")

    def feature_modes(p, g):
        ms = set()
        st = p.get("station")
        if st in MODE_KEYS:
            ms.add(MODE_KEYS[st])
        if p.get("railway") == "tram_stop":
            ms.add("tram")
        for k in ("subway", "light_rail", "train"):
            if p.get(k) == "yes":
                ms.add(MODE_KEYS[k])
        if not ms:
            m = nearest_track_mode(g, 150)
            ms.add(m or "train")
        return ms

    # ---------------------------------------------------------------- relation lookups
    member_area = {}                    # (type, id) -> stop_area id
    area_rows = {}
    group_rows = []
    for r in rels:
        if r["kind"] == "stop_area":
            area_rows[r["id"]] = r
            for m in r["members"]:
                member_area[(m["type"], m["id"])] = r["id"]
        elif r["kind"] == "stop_area_group":
            group_rows.append(r)

    # ---------------------------------------------------------------- step 1+2: stations
    S = []   # station dicts
    by_area = {}
    for it in station_feats:
        p = it["p"]
        sa = p.get("_stop_area")
        modes = feature_modes(p, it["g"])
        if sa and sa in by_area:
            s = S[by_area[sa]]
            s["feats"].append(it)
            s["modes"] |= modes
            continue
        s = {"feats": [it], "modes": set(modes), "areas": {sa} if sa else set(), "platforms": [], "stops": [], "grouping": "mapped" if sa else "inferred"}
        S.append(s)
        if sa:
            by_area[sa] = len(S) - 1

    def station_point(s):
        pts = [f["g"] for f in s["feats"] if f["g"].geom_type == "Point"]
        if pts:
            return pts[0]
        if s["feats"]:
            return s["feats"][0]["g"].representative_point()
        return None

    for s in S:
        s["pt"] = station_point(s)
    S_tree = STRtree([s["pt"] for s in S])

    # platforms: mapped via stop_area first; the rest go to the station whose footprint (point + mapped platforms)
    # is nearest and within its radius, else the station whose railway land holds them
    plat_station = {}
    for it in platforms:
        p = it["p"]
        pmodes = {MODE_KEYS[k] for k in ("subway", "light_rail", "train") if p.get(k) == "yes"}
        if not pmodes:
            m = nearest_track_mode(it["g"], 25)
            pmodes = {m} if m else set()
        it["modes"] = pmodes
        sa = p.get("_stop_area")
        if sa and sa in by_area:
            S[by_area[sa]]["platforms"].append(it)
            plat_station[it["id"]] = by_area[sa]
    anchors, anchor_owner = [], []
    for i, s in enumerate(S):
        anchors.append(s["pt"])
        anchor_owner.append(i)
        for x in s["platforms"]:
            anchors.append(x["g"])
            anchor_owner.append(i)
    anchor_tree = STRtree(anchors)
    for it in platforms:
        if it["id"] in plat_station:
            continue
        pmodes = it["modes"]
        c = it["g"].representative_point()
        best, bd = None, 1e9
        for ai in anchor_tree.query(c.buffer(300)):
            i = anchor_owner[ai]
            s = S[i]
            if pmodes and not (pmodes & s["modes"]):
                continue
            d = anchors[ai].distance(c)
            if d <= max(RADIUS[m] for m in s["modes"]) * (1 if pmodes else 0.5) and d < bd:
                best, bd = i, d
        if best is None:
            for li in land_tree.query(c):
                if lands[li]["g"].contains(c):
                    near = [i for i in S_tree.query(c.buffer(200)) if lands[li]["g"].contains(S[i]["pt"]) and (not pmodes or pmodes & S[i]["modes"])]
                    if near:
                        best = min(near, key=lambda i: S[i]["pt"].distance(c))
                        break
        if best is not None:
            S[best]["platforms"].append(it)
            plat_station[it["id"]] = best

    # ---------------------------------------------------------------- step 3: merge duplicates of the same mode
    uf = UF(len(S))
    for i, s in enumerate(S):
        s["norm"] = norm_name(next((f["p"].get("name") for f in s["feats"] if f["p"].get("name")), ""))
        ids = defaultdict(set)
        for f in s["feats"]:
            for k in ("wikidata", "uic_ref", "railway:ref"):
                if f["p"].get(k):
                    ids[k].add(f["p"][k])
        s["keys"] = {(k, v) for k, vs in ids.items() for v in vs}
    key_index = defaultdict(list)
    for i, s in enumerate(S):
        for k in s["keys"]:
            key_index[k].append(i)
    for k, idxs in key_index.items():
        for a in idxs[1:]:
            if S[a]["modes"] & S[idxs[0]]["modes"]:
                uf.union(idxs[0], a)
    for i, s in enumerate(S):
        if not s["norm"]:
            continue
        r = max(RADIUS[m] for m in s["modes"])
        for j in S_tree.query(s["pt"].buffer(r)):
            if j <= i or S[j]["norm"] != s["norm"] or not (S[j]["modes"] & s["modes"]):
                continue
            if s["pt"].distance(S[j]["pt"]) <= r:
                uf.union(i, j)
    merged = defaultdict(list)
    for i in range(len(S)):
        merged[uf.find(i)].append(i)
    ST = []          # final stations
    old_to_new = {}
    for root, members in merged.items():
        ms = [S[i] for i in members]
        st = {
            "feats": [f for m in ms for f in m["feats"]], "modes": set().union(*[m["modes"] for m in ms]),
            "areas": set().union(*[m["areas"] for m in ms]), "platforms": [x for m in ms for x in m["platforms"]],
            "grouping": "mapped" if all(m["grouping"] == "mapped" for m in ms) and len(ms) == 1 else ("inferred" if len(ms) > 1 and not any(m["grouping"] == "mapped" for m in ms) else "mapped"),
            "merged_from": len(ms),
        }
        # dedupe platforms by id
        seen, uniq = set(), []
        for x in st["platforms"]:
            if x["id"] not in seen:
                seen.add(x["id"])
                uniq.append(x)
        st["platforms"] = uniq
        st["pt"] = max(ms, key=lambda m: len(m["platforms"]))["pt"]
        st["norm"] = max(ms, key=lambda m: len(m["platforms"]))["norm"]
        for i in members:
            old_to_new[i] = len(ST)
        ST.append(st)
    st_tree = STRtree([s["pt"] for s in ST])
    area_to_station = {sa: old_to_new[i] for sa, i in by_area.items()}
    for pid, i in list(plat_station.items()):
        plat_station[pid] = old_to_new[i]

    # ---------------------------------------------------------------- size inputs
    # track dead ends (for the terminus flag): end coordinates used by exactly one mainline way
    main_types = {"high_speed", "intercity", "regional", "freight"}
    main_tracks = [t for t in tracks if t["p"].get("railway") == "rail" and t["p"]["type"] in main_types - {"freight"}]
    ends = Counter()
    for t in main_tracks:
        cs = list(t["g"].coords)
        if len(cs) >= 2:
            ends[(round(cs[0][0]), round(cs[0][1]))] += 1
            ends[(round(cs[-1][0]), round(cs[-1][1]))] += 1
    dead_pts = []
    for t in main_tracks:
        cs = list(t["g"].coords)
        for c in (cs[0], cs[-1]):
            if ends[(round(c[0]), round(c[1]))] == 1:
                dead_pts.append(Point(c))
    dead_tree = STRtree(dead_pts)
    mt_tree = STRtree([t["g"] for t in main_tracks])

    def area_ha(g):
        return g.area / 10000.0

    # ---- major station rules (see docs/00-overview.md, 2026-10-11 "Major station rules")
    cfg_path = HERE / "config" / f"{slug}.json"
    cfg = json.load(open(cfg_path)) if cfg_path.exists() else {}
    main_re = re.compile(cfg["main_station_re"]) if cfg.get("main_station_re") else None
    main_norms = {norm_name(n) for n in cfg.get("main_terminals", [])}
    # high-speed track by its OWN tags (a classic line that TGVs merely use does not count)
    hs_geoms = [t["g"] for t in tracks if t["p"].get("own_type") == "high_speed" and t["p"].get("railway") != "construction"]
    hs_tree = STRtree(hs_geoms)

    for st in ST:
        mp = [x for x in st["platforms"] if "train" in x["modes"]]
        toks, unreffed = set(), 0
        for x in mp:
            t = ref_tokens(x["p"].get("ref"))
            if t:
                toks |= t
            else:
                unreffed += 1
        st["mainline_platforms"] = len(toks) + unreffed
        st["platform_count"] = len({t for x in st["platforms"] for t in ref_tokens(x["p"].get("ref"))}) + sum(1 for x in st["platforms"] if not ref_tokens(x["p"].get("ref")))
        # railway land and building containing the station's points / platforms
        probes = [st["pt"]] + [x["g"].representative_point() for x in st["platforms"][:60]]
        land_ids, bld_ids = set(), set()
        for pr in probes:
            for li in land_tree.query(pr):
                if lands[li]["g"].contains(pr):
                    land_ids.add(li)
            for bi in bld_tree.query(pr):
                if buildings[bi]["g"].contains(pr):
                    bld_ids.add(bi)
        for f in st["feats"]:
            if f["g"].geom_type != "Point":
                bld_ids.update(bi for bi in bld_tree.query(f["g"]) if buildings[bi]["g"].intersects(f["g"]) and False)
        disc = st["pt"].buffer(LAND_DISC)
        local = shapely.unary_union([lands[i]["g"] for i in land_ids]).intersection(disc) if land_ids else None
        st["land_ha"] = round(area_ha(local), 2) if local is not None and not local.is_empty else 0.0
        st["building_ha"] = round(sum(area_ha(buildings[i]["g"]) for i in bld_ids), 3)
        # mainline tracks serving the platforms (or near the point when no platforms are mapped)
        near = set()
        if mp and "train" in st["modes"]:
            for x in mp:
                for ti in mt_tree.query(x["g"].buffer(20)):
                    if main_tracks[ti]["g"].distance(x["g"]) <= 20:
                        near.add(main_tracks[ti]["id"])
        elif "train" in st["modes"]:
            for ti in mt_tree.query(st["pt"].buffer(80)):
                if main_tracks[ti]["g"].distance(st["pt"]) <= 80:
                    near.add(main_tracks[ti]["id"])
        st["mainline_track_ways"] = len(near)
        # terminus: at least two mainline track dead ends close to the station's own mainline platforms
        dead = 0
        if "train" in st["modes"]:
            zone = (shapely.unary_union([x["g"] for x in mp] + [st["pt"]])).buffer(TERMINUS_ZONE)
            for di in dead_tree.query(zone):
                if zone.contains(dead_pts[di]):
                    dead += 1
        st["dead_ends"] = dead
        st["terminus"] = dead >= 2
        st["score"] = round(
            W["platform"] * st["mainline_platforms"] + W["track"] * min(st["mainline_track_ways"], W["track_cap"])
            + W["land"] * math.sqrt(st["land_ha"]) + W["building"] * math.sqrt(st["building_ha"]) + (W["terminus"] if st["terminus"] else 0), 1)
        st["tier"] = next((t for cut, t in TIERS if st["score"] >= cut), 3)

        # Major stations become tier 1. Three ways in, all from the infrastructure or a hand-kept list, never line counts:
        #  1. name pattern (Chinese main high-speed stations: city + optional direction)
        #  2. canonical terminal list kept per region
        #  3. a mainline station next to high-speed track that is a terminus or has many platforms
        st["hsr"] = any(hs_geoms[ti].distance(st["pt"]) <= HS_NEAR for ti in hs_tree.query(st["pt"].buffer(HS_NEAR)))
        names = [f["p"].get("name") or "" for f in st["feats"]]
        norms = {norm_name(n) for n in names} | {st["norm"]}
        major = None
        if "train" in st["modes"]:
            if main_re and any(main_re.search(n) for n in names):
                major = "name_pattern"
            elif norms & main_norms - {""}:
                major = "canonical_list"
            elif st["hsr"] and (st["terminus"] or st["mainline_platforms"] >= HS_MAJOR_PLATFORMS):
                major = "hsr_infrastructure"
        st["major"] = major
        if major:
            st["tier"] = 1

    # ---------------------------------------------------------------- stop positions -> stations
    node_station = {}
    for s_i, st in enumerate(ST):
        for f in st["feats"]:
            t, _, n = f["id"].partition("/")
            node_station[(t, int(n))] = s_i
    for (t, n), sa in member_area.items():
        if sa in area_to_station:
            node_station.setdefault((t, n), area_to_station[sa])
    for pid, s_i in plat_station.items():
        t, _, n = pid.partition("/")
        node_station.setdefault((t, int(n)), s_i)
    stop_unmapped = []
    for sp in stops:
        key = ("node", int(sp["osm_id"].split("/")[1]))
        if key in node_station:
            continue
        pt = proj(Point(sp["lon"], sp["lat"]))
        pm = {MODE_KEYS[k] for k in ("subway", "light_rail", "train") if sp["modes"].get(k) == "yes"}
        nn = norm_name(sp.get("name"))
        best, bd = None, 1e9
        for i in st_tree.query(pt.buffer(300)):
            st = ST[i]
            if pm and not (pm & st["modes"]):
                continue
            d = st["pt"].distance(pt)
            if d > max(RADIUS[m] for m in st["modes"]):
                continue
            score = d - (500 if nn and nn == st["norm"] else 0)
            if score < bd:
                best, bd = i, score
        if best is not None:
            node_station[key] = best
        else:
            stop_unmapped.append(sp["osm_id"])

    # ---------------------------------------------------------------- step 4: lines per station
    route_line = {}
    for l in lines.values():
        for rid in l["routes"]:
            route_line[rid] = l["id"]
    def sid_of(i):
        st = ST[i]
        return sorted(st["areas"])[0] if st["areas"] else st["feats"][0]["id"]

    st_lines = defaultdict(set)
    route_unmapped = Counter()
    route_order = {}        # route id -> station ids in stop order
    routes_total = 0
    for r in rels:
        if r["kind"] != "route":
            continue
        routes_total += 1
        lid = route_line.get(r["id"])
        order = []
        for m in r["members"]:
            if not m["role"].startswith(STOP_ROLES):
                continue
            s_i = node_station.get((m["type"], m["id"]))
            if s_i is None:
                route_unmapped[r["id"]] += 1
            else:
                st_lines[s_i].add(lid)
                if not order or order[-1] != s_i:
                    order.append(s_i)
        route_order[r["id"]] = [sid_of(i) for i in order]

    # named infrastructure lines (route=railway) that list the station as a member; kept apart from services
    st_rail = defaultdict(dict)
    for r in rels:
        if r["kind"] != "railway_line":
            continue
        for m in r["members"]:
            s_i = node_station.get((m["type"], m["id"]))
            if s_i is not None:
                st_rail[s_i][r["id"]] = {"id": r["id"], "name": r["tags"].get("name"), "name_en": english_name(r["tags"])}

    # ---------------------------------------------------------------- step 5: complexes
    cuf = UF(len(ST))
    mapped_pairs = 0
    for g in group_rows:
        idx = sorted({area_to_station[f"{m['type']}/{m['id']}"] for m in g["members"] if f"{m['type']}/{m['id']}" in area_to_station})
        for a in idx[1:]:
            cuf.union(idx[0], a)
            mapped_pairs += 1
    for i, st in enumerate(ST):
        wd = {f["p"].get("wikidata") for f in st["feats"] if f["p"].get("wikidata")}
        st["wd"] = wd
    # inferred: different modes, shared wikidata, or same normalised name within 300 m, or the same station building
    bld_of = {}
    for i, st in enumerate(ST):
        for bi in bld_tree.query(st["pt"]):
            if buildings[bi]["g"].contains(st["pt"]):
                bld_of[i] = bi
                break
    by_bld = defaultdict(list)
    for i, b in bld_of.items():
        by_bld[b].append(i)
    for b, idxs in by_bld.items():
        for a in idxs[1:]:
            if ST[a]["modes"] != ST[idxs[0]]["modes"]:
                cuf.union(idxs[0], a)
    for i, st in enumerate(ST):
        for j in st_tree.query(st["pt"].buffer(GROUP_RADIUS)):
            if j <= i:
                continue
            o = ST[j]
            if o["modes"] == st["modes"]:
                continue
            same_name = st["norm"] and st["norm"] == o["norm"] and st["pt"].distance(o["pt"]) <= GROUP_RADIUS
            if (st["wd"] & o["wd"]) or same_name:
                cuf.union(i, j)
    complexes = defaultdict(list)
    for i in range(len(ST)):
        complexes[cuf.find(i)].append(i)

    # ---------------------------------------------------------------- write
    def stn_name(st):
        best = max(st["feats"], key=lambda f: (bool(f["p"].get("name")), f["g"].geom_type == "Point"))
        for sa in st["areas"]:
            nm = area_rows[sa]["tags"].get("name")
            if nm:
                return nm, area_rows[sa]["tags"]
        return best["p"].get("name"), best["p"]

    def name_tags(st):
        tags = {}
        for f in st["feats"]:
            tags.update({k: v for k, v in f["p"].items() if k.startswith("name:") or k in ("int_name", "official_name")})
        for sa in st["areas"]:
            tags.update({k: v for k, v in area_rows[sa]["tags"].items() if k.startswith("name:")})
        return tags

    n_out, c_out = 0, 0
    complex_of = {}
    cplx_records = []
    for root, idxs in complexes.items():
        biggest = max(idxs, key=lambda i: (ST[i]["score"], ST[i]["mainline_platforms"]))
        # complex id: stop_area_group id if one exists, else id of its largest station
        gid = None
        for g in group_rows:
            have = {area_to_station.get(f"{m['type']}/{m['id']}") for m in g["members"]}
            if have & set(idxs):
                gid = g["id"]
                break
        sid = (sorted(ST[biggest]["areas"])[0] if ST[biggest]["areas"] else ST[biggest]["feats"][0]["id"])
        cid = gid or sid
        for i in idxs:
            complex_of[i] = cid
        cplx_records.append((cid, idxs, biggest, "mapped" if gid else "inferred"))

    json.dump(route_order, open(out / "route_stations.json", "w"), ensure_ascii=False)
    with open(out / "stations.geojsonl", "w") as fs:
        for i, st in enumerate(ST):
            nm, tg = stn_name(st)
            nt = name_tags(st)
            lids = sorted(l for l in st_lines.get(i, ()) if l)
            by_type = Counter(lines[l]["type"] for l in lids if l in lines)
            sid = sid_of(i)
            pp = st["pt"]
            hsr = st["hsr"]
            props = {
                "id": sid, "complex": complex_of[i], "name": nm, "name_en": english_name({**tg, **nt}),
                "modes": sorted(st["modes"]), "tier": st["tier"], "score": st["score"], "terminus": st["terminus"],
                "mainline_platforms": st["mainline_platforms"], "platform_count": st["platform_count"],
                "land_ha": st["land_ha"], "building_ha": st["building_ha"], "mainline_track_ways": st["mainline_track_ways"],
                "dead_ends": st["dead_ends"], "hsr": hsr, "major": st["major"],
                "rail_lines": sorted(st_rail.get(i, {}).values(), key=lambda x: (x["name"] or "", x["id"])),
                "platform_refs": sorted({t for x in st["platforms"] if "train" in x["modes"] for t in ref_tokens(x["p"].get("ref"))}, key=lambda r: (not r.isdigit(), int(r) if r.isdigit() else 0, r)),
                "platform_refs_other": sorted({t for x in st["platforms"] if "train" not in x["modes"] for t in ref_tokens(x["p"].get("ref"))}, key=lambda r: (not r.isdigit(), int(r) if r.isdigit() else 0, r)), "line_count": len(lids), "line_counts": dict(by_type), "line_ids": lids,
                "wikidata": sorted(st["wd"])[:1] if st.get("wd") else None, "grouping": st["grouping"],
                "source_ids": [f["id"] for f in st["feats"]] + sorted(st["areas"]),
            }
            fs.write(json.dumps({"type": "Feature", "properties": props, "geometry": {"type": "Point", "coordinates": unproj(pp.x, pp.y)}}, ensure_ascii=False, separators=(",", ":")) + "\n")
            n_out += 1
    with open(out / "complexes.geojsonl", "w") as fc:
        for cid, idxs, biggest, how in cplx_records:
            st = ST[biggest]
            nm, tg = stn_name(st)
            tier = min(ST[i]["tier"] for i in idxs)
            lids = set()
            for i in idxs:
                lids |= {l for l in st_lines.get(i, ()) if l}
            props = {
                "id": cid, "name": nm, "name_en": english_name({**tg, **name_tags(st)}), "tier": tier,
                "terminus": any(ST[i]["terminus"] for i in idxs), "station_count": len(idxs),
                "modes": sorted(set().union(*[ST[i]["modes"] for i in idxs])),
                "platform_count": sum(ST[i]["platform_count"] for i in idxs), "line_count": len(lids), "grouping": how,
                "station_ids": [(sorted(ST[i]["areas"])[0] if ST[i]["areas"] else ST[i]["feats"][0]["id"]) for i in idxs],
            }
            fc.write(json.dumps({"type": "Feature", "properties": props, "geometry": {"type": "Point", "coordinates": unproj(st["pt"].x, st["pt"].y)}}, ensure_ascii=False, separators=(",", ":")) + "\n")
            c_out += 1

    tier_counts = Counter(st["tier"] for st in ST)
    report = {
        "region": slug, "stations": len(ST), "complexes": c_out, "merged_duplicates": sum(1 for st in ST if st["merged_from"] > 1),
        "tiers": dict(sorted(tier_counts.items())), "terminus_flagged": sum(1 for st in ST if st["terminus"]),
        "stop_positions_unmapped": len(stop_unmapped), "stop_positions_total": len(stops),
        "routes_with_unmapped_stops": len(route_unmapped), "routes_total": routes_total,
        "mapped_group_unions": mapped_pairs,
    }
    json.dump(report, open(out / "stations_report.json", "w"), indent=2)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main(sys.argv[1])
