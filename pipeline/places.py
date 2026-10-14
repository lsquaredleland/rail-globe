"""Place names for the map: countries with data, and cities and towns that have a passenger station.
Usage: python3 places.py     (reads ../app/public/data/pilot.parts.json and ../data/<slug>/complexes.geojsonl; fetches OpenFreeMap place tiles)
Writes ../app/public/data/places.json: [[lon, lat, class, rank, name, name_en, n_stations], ...] (class: country, city, town).
A city or town is kept when a station lies within CITY_KM / TOWN_KM of its point. Names come from OpenStreetMap through OpenFreeMap (ODbL)."""
import gzip, json, math, sys, urllib.request
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import mapbox_vector_tile as mvt

HERE = Path(__file__).resolve().parent
DATA = HERE.parent / "data"
OUT = HERE.parent / "app" / "public" / "data"
Z = 7
CITY_KM, TOWN_KM = 12.0, 5.0
COUNTRIES = {"Germany", "France", "Switzerland", "Italy", "China"}   # countries drawn whole


UA = {"User-Agent": "rail-globe-pipeline/0.1 (places.py)"}


def get(url):
    return urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=30)


def tile_url():
    with get("https://tiles.openfreemap.org/planet") as r:
        return json.load(r)["tiles"][0]


def fetch(url):
    for _ in range(3):
        try:
            with get(url) as r:
                b = r.read()
            return gzip.decompress(b) if b[:2] == b"\x1f\x8b" else b
        except Exception:
            pass
    return b""


def tiles_for(bounds):
    w, s, e, n = bounds
    n_ = 2 ** Z
    x0, x1 = int((w + 180) / 360 * n_), int((e + 180) / 360 * n_)
    y = lambda lat: int((1 - math.asinh(math.tan(math.radians(lat))) / math.pi) / 2 * n_)
    return [(x, yy) for x in range(x0, x1 + 1) for yy in range(y(n), y(s) + 1)]


def main():
    parts = json.load(open(OUT / "pilot.parts.json"))
    # stations: every complex point of every region that has tiles
    stations = []
    for p in parts:
        f = DATA / p["slug"] / "complexes.geojsonl"
        if not f.exists():
            continue
        for l in open(f):
            c = json.loads(l)["geometry"]["coordinates"]
            stations.append((c[0], c[1]))
    grid = defaultdict(list)
    for x, y in stations:
        grid[(int(x / 0.1), int(y / 0.1))].append((x, y))

    def n_near(x, y, km):
        k = (int(x / 0.1), int(y / 0.1))
        r = int(km / 8) + 2
        c = 0
        for dx in range(-r, r + 1):
            for dy in range(-r, r + 1):
                for sx, sy in grid.get((k[0] + dx, k[1] + dy), ()):
                    if math.hypot((sx - x) * math.cos(math.radians(y)), sy - y) * 111.2 <= km:
                        c += 1
        return c

    url = tile_url()
    todo = sorted({t for p in parts for t in tiles_for(p["bounds"])})
    print(len(todo), "tiles", file=sys.stderr)
    seen, out = set(), []
    with ThreadPoolExecutor(8) as ex:
        blobs = list(ex.map(lambda t: (t, fetch(url.format(z=Z, x=t[0], y=t[1]))), todo))
    for (tx, ty), b in blobs:
        if not b:
            continue
        try:
            layer = mvt.decode(b).get("place")
        except Exception:
            continue
        if not layer:
            continue
        ext = layer.get("extent", 4096)
        for f in layer["features"]:
            p = f["properties"]
            cls = p.get("class")
            if cls not in ("city", "town", "country"):
                continue
            px, py = f["geometry"]["coordinates"]
            lon = (tx + px / ext) / 2 ** Z * 360 - 180
            lat = math.degrees(math.atan(math.sinh(math.pi * (1 - 2 * (ty + 1 - py / ext) / 2 ** Z))))
            name = p.get("name") or p.get("name:latin") or p.get("name_en")
            en = p.get("name_en") or p.get("name:latin") or name
            if not name:
                continue
            key = (cls, name, round(lon, 1), round(lat, 1))
            if key in seen:
                continue
            seen.add(key)
            if cls == "country":
                if en not in COUNTRIES:
                    continue
                out.append([round(lon, 3), round(lat, 3), cls, 0, name, en, 0])
                continue
            n = n_near(lon, lat, CITY_KM if cls == "city" else TOWN_KM)
            if n:
                out.append([round(lon, 3), round(lat, 3), cls, p.get("rank") or 99, name, en, n])
    json.dump(out, open(OUT / "places.json", "w"), ensure_ascii=False, separators=(",", ":"))
    c = defaultdict(int)
    for o in out:
        c[o[2]] += 1
    print(dict(c), file=sys.stderr)


if __name__ == "__main__":
    main()
