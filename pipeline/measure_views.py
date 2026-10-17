"""Phase 3 budget check: compressed PMTiles bytes needed to draw one view (docs: under 600 KB per view).
Usage: python3 measure_views.py   (reads ../app/public/data/<slug>.pmtiles per view; use TILES=pilot to read every view from pilot.pmtiles, the file the app draws from: `build` no longer writes per-region files for the pilot sources)
A view = one screen; MapLibre uses 512 px tiles and loads tile zoom floor(map zoom)."""
import math
import os
from pathlib import Path
from pmtiles.reader import Reader, MmapSource

DATA = Path(__file__).resolve().parent.parent / "app" / "public" / "data"
VIEWS = {  # name -> (tile source, lon, lat); Chengdu and the Greater Bay Area are drawn from China's tiles
    "Paris centre": ("paris", 2.3553, 48.8785),
    "Paris default": ("paris", 2.35, 48.86),
    "SF Bay Area": ("sf-bay-area", -122.22, 37.7),
    "SF downtown": ("sf-bay-area", -122.40, 37.78),
    "Chengdu": ("china", 104.07, 30.66),
    "Chengdu East": ("china", 104.14, 30.63),
    "GBA region": ("china", 113.55, 22.75),
    "Shenzhen": ("china", 114.06, 22.58),
    "Hong Kong": ("china", 114.17, 22.30),
    "Guangzhou": ("china", 113.27, 23.13),
    "Germany": ("de", 10.45, 51.1),
    "Ruhr": ("de", 7.2, 51.45),
    "Berlin": ("de", 13.40, 52.52),
    "Frankfurt": ("de", 8.66, 50.11),
    "France": ("fr", 2.6, 46.6),
    "Lyon": ("fr", 4.85, 45.76),
    "Switzerland": ("ch", 8.2, 46.8),
    "Zurich": ("ch", 8.54, 47.38),
    "Italy": ("it", 12.5, 42.4),
    "Milan": ("it", 9.19, 45.46),
    "Rome": ("it", 12.5, 41.9),
}
SCREENS = {"laptop 1440x900": (1440, 900), "phone 390x844": (390, 844)}
ZOOMS = [4, 5, 6, 8, 9.5, 10.6, 12, 13, 14]


def world_px(lon, lat, z):
    s = 512 * 2 ** z
    x = (lon + 180) / 360 * s
    y = (1 - math.log(math.tan(math.radians(lat)) + 1 / math.cos(math.radians(lat))) / math.pi) / 2 * s
    return x, y


def tiles_for(lon, lat, z, w, h):
    cx, cy = world_px(lon, lat, z)
    tz = min(int(math.floor(z)), 14)
    scale = 2 ** (z - tz)
    tpx = 512 * scale  # tile size on screen in px
    x0, x1 = (cx - w / 2) / tpx, (cx + w / 2) / tpx
    y0, y1 = (cy - h / 2) / tpx, (cy + h / 2) / tpx
    return tz, range(int(math.floor(x0)), int(math.floor(x1)) + 1), range(int(math.floor(y0)), int(math.floor(y1)) + 1)


WORLD_VIEWS = {"Europe": (10, 50), "World": (0, 20), "Moscow": (37.62, 55.75), "Cairo": (31.24, 30.04), "Delhi": (77.2, 28.6), "Mumbai": (72.87, 19.07),
               "Seoul": (126.98, 37.57), "Beijing": (116.4, 39.9), "Shanghai": (121.47, 31.23)}  # the views of docs/16-low-zoom-tile-weight-plan.md
WORLD_ZOOMS = [2, 3, 4, 5, 6, 7, 8]


def main_world(path):
    """WORLD=<world.pmtiles>: compressed bytes of the named views at integer zooms 2 to 8, then the worst view over a 1 degree grid of centres inside
    the file's bounds (docs/16-low-zoom-tile-weight-plan.md). A view whose tiles are not in the file (a subset build) reads 0."""
    rd = Reader(MmapSource(open(path, "rb")))
    hdr = rd.header()
    b = [hdr["min_lon_e7"] / 1e7, hdr["min_lat_e7"] / 1e7, hdr["max_lon_e7"] / 1e7, hdr["max_lat_e7"] / 1e7]

    def view_kb(lon, lat, z, w, h):
        tz, xs, ys = tiles_for(lon, lat, z, w, h)
        n = 2 ** tz
        return sum(len(rd.get(tz, x, y) or b"") for x in xs for y in ys if 0 <= y < n and 0 <= x < n) / 1024

    print(f"{'view':10} {'screen':16} " + " ".join(f"z{z:<5}" for z in WORLD_ZOOMS) + "   (KB, compressed)")
    for name, (lon, lat) in WORLD_VIEWS.items():
        for sname, (w, h) in SCREENS.items():
            print(f"{name:10} {sname:16} " + " ".join(f"{view_kb(lon, lat, z, w, h):6.0f}" for z in WORLD_ZOOMS))
    print(f"\nworst over a 1 degree grid of centres inside the file's bounds {b}")
    for sname, (w, h) in SCREENS.items():
        row = []
        for z in WORLD_ZOOMS:
            row.append(max(view_kb(lon, lat, z, w, h) for lon in range(math.floor(b[0]), math.ceil(b[2]) + 1) for lat in range(math.floor(b[1]), math.ceil(b[3]) + 1)))
        print(f"{'worst':10} {sname:16} " + " ".join(f"{v:6.0f}" for v in row))


def main():
    if os.environ.get("WORLD"):
        return main_world(os.environ["WORLD"])
    worst = 0
    readers = {}
    if os.environ.get("TILES"):
        print(f"measuring {DATA / (os.environ['TILES'] + '.pmtiles')} for every view")
    print(f"{'view':16} {'screen':16} " + " ".join(f"z{z:<5}" for z in ZOOMS) + "   (KB, compressed)")
    for name, (slug, lon, lat) in VIEWS.items():
        slug = os.environ.get("TILES") or slug
        if slug not in readers:
            f = open(DATA / f"{slug}.pmtiles", "rb")
            readers[slug] = (Reader(MmapSource(f)), f)
        rd = readers[slug][0]
        for sname, (w, h) in SCREENS.items():
            row = []
            for z in ZOOMS:
                tz, xs, ys = tiles_for(lon, lat, z, w, h)
                total = 0
                for x in xs:
                    for y in ys:
                        # raw (still compressed) tile bytes; get() returns the stored bytes
                        t = rd.get(tz, x, y)
                        total += len(t) if t else 0
                row.append(total / 1024)
                worst = max(worst, total / 1024)
            print(f"{name:16} {sname:16} " + " ".join(f"{v:6.0f}" for v in row))
    print(f"\nworst view: {worst:.0f} KB (budget 600 KB)")


if __name__ == "__main__":
    main()
