"""Phase 3 addition: airport outlines (aeroway=aerodrome) and terminals (aeroway=terminal) from a region extract.

Usage: python3 airports.py <slug>
Reads  ../data/<slug>.rail.osm.pbf (from filter.py), else ../data/<slug>.osm.pbf
Writes ../data/<slug>/airports.geojsonl  (polygons: id, k = aerodrome|terminal, name, name_en, iata, icao, operator)
Aerodromes are kept when they have an IATA or ICAO code; terminals when they carry any name, iata or ref.
"""
import json
import sys
from pathlib import Path

import osmium

from names import english_name

DATA = Path(__file__).resolve().parent.parent / "data"


def rnd(c):
    return [rnd(x) for x in c] if isinstance(c, (list, tuple)) else round(c, 5)


class H(osmium.SimpleHandler):
    def __init__(self, out):
        super().__init__()
        self.out = out
        self.f = osmium.geom.GeoJSONFactory()
        self.n = {"aerodrome": 0, "terminal": 0}

    def area(self, a):
        t = {x.k: x.v for x in a.tags}
        kind = t.get("aeroway")
        if kind not in ("aerodrome", "terminal"):
            return
        if kind == "aerodrome" and not (t.get("iata") or t.get("icao")):
            return
        if kind == "terminal" and not (t.get("name") or t.get("iata") or t.get("ref")):
            return
        try:
            g = json.loads(self.f.create_multipolygon(a))
        except Exception:
            return
        g["coordinates"] = rnd(g["coordinates"])
        props = {"id": ("way/" if a.from_way() else "relation/") + str(a.orig_id()), "k": kind}
        for k_in, k_out in (("name", "name"), ("iata", "iata"), ("icao", "icao"), ("operator", "operator"), ("aerodrome:type", "atype"), ("military", "military")):
            if t.get(k_in):
                props[k_out] = t[k_in]
        en = english_name(t)
        if en and en != t.get("name"):
            props["name_en"] = en
        self.out.write(json.dumps({"type": "Feature", "properties": props, "geometry": g}, ensure_ascii=False, separators=(",", ":")) + "\n")
        self.n[kind] += 1


if __name__ == "__main__":
    slug = sys.argv[1]
    with open(DATA / slug / "airports.geojsonl", "w") as out:
        h = H(out)
        # the prefiltered file (written by filter.py) holds the aeroway objects and is far smaller than the extract
        pre = DATA / f"{slug}.rail.osm.pbf"
        h.apply_file(str(pre if pre.exists() else DATA / f"{slug}.osm.pbf"), locations=True)
    print(slug, h.n)
