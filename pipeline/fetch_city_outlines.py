#!/usr/bin/env python3
"""Outlines of the cities in search.json that have only a point (docs/14-city-outlines-and-highlights.md, section 1).

    rail-extract places ...                      # writes <data>/city-outlines-wanted.json: name, Wikidata id, point of every point-only city
    python3 pipeline/fetch_city_outlines.py [--data data/world] [--regions data/regions] [--refresh]
    rail-extract build <country slugs> ...       # statistics and shapes pick the new outlines up (index.json `city: true`)
    rail-extract places ...                      # the search entries get the outlines' boxes and slugs

For each city: the boundary relation with its Wikidata id is looked up on Overpass (admin_level 5 to 9; the highest level wins, so a
city beats the county that shares its id), its polygon is fetched from Nominatim (`lookup`, 40 ids per request, about 20 m
simplification), and written as data/regions/<cc>-<name>.geojson with an entry in data/regions/index.json carrying `"city": true` (the real
admin_level stays). Cities whose relation is a province or country (level 4 or less: Shanghai, Mexico City) already have an outline and are
skipped, as are polygons under 2 km2 or over 20,000 km2. Raw answers are kept in data/ext/city-outlines/ so a re-run is offline; --refresh
asks again. Network: overpass-api.de and nominatim.openstreetmap.org (usage policy: one request a second, identifying User-Agent). Needs
`curl` (the system Python's TLS is too old). Output is git-ignored with the rest of data/.
"""
import argparse, json, math, os, re, subprocess, sys, time, unicodedata

UA = "rail-globe-city-outlines/1.0 (https://github.com/lsquaredleland/rail-globe)"
MIN_KM2, MAX_KM2 = 2.0, 20000.0


def curl(args, retries=4):
    for i in range(retries):
        r = subprocess.run(["curl", "-s", "-S", "-m", "180", "-A", UA] + args, capture_output=True, text=True)
        if r.returncode == 0 and r.stdout.strip().startswith(("{", "[")):
            return json.loads(r.stdout)
        print(f"  retry {i + 1}: curl {r.returncode} {r.stderr.strip()[:120]} {r.stdout[:80]!r}", file=sys.stderr)
        time.sleep(5 * (i + 1))
    raise SystemExit("giving up")


def overpass(ids):
    q = '[out:json][timeout:150];rel["wikidata"~"^(%s)$"]["boundary"="administrative"];out tags;' % "|".join(ids)
    return curl(["--data-urlencode", "data=" + q, "https://overpass-api.de/api/interpreter"])["elements"]


def nominatim(rel_ids):
    url = "https://nominatim.openstreetmap.org/lookup?format=geojson&polygon_geojson=1&polygon_threshold=0.0002&addressdetails=1&osm_ids=" + ",".join("R%d" % i for i in rel_ids)
    return curl([url])["features"]


def km2(geom):
    polys = geom["coordinates"] if geom["type"] == "MultiPolygon" else [geom["coordinates"]]
    total = 0.0
    for p in polys:
        for k, ring in enumerate(p):
            lat0 = math.radians(sum(c[1] for c in ring) / len(ring))
            a = 0.0
            for (x1, y1), (x2, y2) in zip(ring, ring[1:] + ring[:1]):
                a += (x1 * 111.32 * math.cos(lat0)) * (y2 * 110.54) - (x2 * 111.32 * math.cos(lat0)) * (y1 * 110.54)
            total += (abs(a) / 2) * (1 if k == 0 else -1)
    return total


def slugify(s):
    s = unicodedata.normalize("NFKD", s).encode("ascii", "ignore").decode()
    return re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", default="data/world")
    ap.add_argument("--regions", default="data/regions")
    ap.add_argument("--raw", default="data/ext/city-outlines")
    ap.add_argument("--refresh", action="store_true")
    a = ap.parse_args()
    wanted = json.load(open(os.path.join(a.data, "city-outlines-wanted.json")))
    os.makedirs(a.raw, exist_ok=True)
    index_path = os.path.join(a.regions, "index.json")
    index = json.load(open(index_path))
    ids = sorted({w["wikidata"] for w in wanted if re.fullmatch(r"Q\d+", w["wikidata"])})
    print(f"{len(wanted)} point cities, {len(ids)} Wikidata ids")

    # 1. wikidata -> relation (highest admin_level 5..9 wins)
    rel_file = os.path.join(a.raw, "relations.json")
    if os.path.exists(rel_file) and not a.refresh:
        rels = json.load(open(rel_file))
    else:
        rels = {}
        for i in range(0, len(ids), 120):
            for e in overpass(ids[i:i + 120]):
                t = e["tags"]
                lvl = t.get("admin_level", "")
                if lvl.isdigit() and int(lvl) <= 9:
                    cur = rels.get(t["wikidata"])
                    if cur is None or int(lvl) > int(cur["admin_level"]):
                        rels[t["wikidata"]] = {"id": e["id"], "admin_level": lvl, "name": t.get("name"), "name_en": t.get("name:en")}
            print(f"  overpass {min(i + 120, len(ids))}/{len(ids)}")
            time.sleep(2)
        json.dump(rels, open(rel_file, "w"), ensure_ascii=False)
    todo = {w["wikidata"]: w for w in wanted if w["wikidata"] in rels and int(rels[w["wikidata"]]["admin_level"]) >= 5}
    print(f"{len(rels)} relations found; {len(todo)} at admin level 5 to 9")

    # 2. relation -> polygon
    poly_file = os.path.join(a.raw, "polygons.json")
    polys = json.load(open(poly_file)) if os.path.exists(poly_file) and not a.refresh else {}
    need = [rels[q]["id"] for q in todo if str(rels[q]["id"]) not in polys]
    for i in range(0, len(need), 40):
        for f in nominatim(need[i:i + 40]):
            p = f["properties"]
            polys[str(p["osm_id"])] = {"geometry": f["geometry"], "country": (p.get("address") or {}).get("country_code", "").upper(), "name": p.get("name")}
        print(f"  nominatim {min(i + 40, len(need))}/{len(need)}")
        json.dump(polys, open(poly_file, "w"), ensure_ascii=False)
        time.sleep(1.5)

    # 3. write the outlines
    taken = set(index)
    made, skipped = 0, {}
    for q, w in sorted(todo.items(), key=lambda kv: -kv[1]["stations"]):
        r = rels[q]
        pol = polys.get(str(r["id"]))
        why = None
        if not pol or not pol["geometry"] or pol["geometry"]["type"] not in ("Polygon", "MultiPolygon"):
            why = "no polygon"
        elif not pol["country"]:
            why = "no country"
        else:
            area = km2(pol["geometry"])
            if not MIN_KM2 <= area <= MAX_KM2:
                why = f"area {area:.0f} km2"
        if why:
            skipped.setdefault(why.split(" ")[0] + " " + why.split(" ")[1] if why.startswith("area") else why, []).append(w["name_en"])
            continue
        osm = "relation/%d" % r["id"]
        old = next((k for k, v in index.items() if v.get("osm_id") == osm), None)
        if old and not index[old].get("city"):
            skipped.setdefault("already an outline", []).append(w["name_en"])
            continue
        en = r["name_en"] or w["name_en"] or r["name"] or pol["name"]
        cc = pol["country"].lower()
        slug = old or f"{cc}-{slugify(en)}"
        if not old:
            base, n = slug, 1
            while slug in taken:   # New York is also a state; Sofia has a province and a city-province
                n += 1
                slug = base + "-city" + ("" if n == 2 else str(n - 1))
        taken.add(slug)
        props = {"slug": slug, "name": r["name"] or w["name"], "name_en": en, "admin_level": r["admin_level"], "country": pol["country"], "iso": None, "osm_id": osm, "city": True}
        with open(os.path.join(a.regions, slug + ".geojson"), "w") as f:
            json.dump({"type": "Feature", "properties": props, "geometry": pol["geometry"]}, f, ensure_ascii=False, separators=(",", ":"))
        index[slug] = {k: v for k, v in props.items() if k != "slug"}
        made += 1
    json.dump(index, open(index_path, "w"), ensure_ascii=False, indent=2)
    print(f"wrote {made} city outlines; skipped:", {k: (len(v), v[:6]) for k, v in skipped.items()})
    no_rel = [w["name_en"] for w in wanted if w["wikidata"] not in rels]
    print(f"{len(no_rel)} cities with no boundary relation of level 5 to 9 under their Wikidata id, e.g. {no_rel[:8]}")


main()
