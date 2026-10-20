"""Budget check: compressed PMTiles bytes needed to draw one view (under 600 KB per view).
Usage, from the repo root: python3 pipeline/measure_views.py [world.pmtiles]   (default app/public/data/world/world.pmtiles; or set WORLD=<file>)
Needs `pip install pmtiles`.
A view = one screen; MapLibre uses 512 px tiles and loads tile zoom floor(map zoom). Prints the named views, then the worst view over a 1 degree grid."""
import math
import os
import sys
from pathlib import Path
from pmtiles.reader import Reader, MmapSource

ROOT = Path(__file__).resolve().parent.parent
DEFAULT = ROOT / "app" / "public" / "data" / "world" / "world.pmtiles"
SCREENS = {"laptop 1440x900": (1440, 900), "phone 390x844": (390, 844)}


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
    path = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("WORLD") or str(DEFAULT)
    main_world(path)


if __name__ == "__main__":
    main()
