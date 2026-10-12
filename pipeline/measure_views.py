"""Phase 3 budget check: compressed PMTiles bytes needed to draw one view (docs: under 600 KB per view).
Usage: python3 measure_views.py   (reads ../app/public/data/*.pmtiles)
A view = one screen; MapLibre uses 512 px tiles and loads tile zoom floor(map zoom)."""
import math
from pathlib import Path
from pmtiles.reader import Reader, MmapSource

DATA = Path(__file__).resolve().parent.parent / "app" / "public" / "data"
VIEWS = {  # name -> (slug, lon, lat)
    "Paris centre": ("paris", 2.3553, 48.8785),
    "Paris default": ("paris", 2.35, 48.86),
    "SF Bay Area": ("sf-bay-area", -122.22, 37.7),
    "SF downtown": ("sf-bay-area", -122.40, 37.78),
    "Chengdu": ("chengdu", 104.07, 30.66),
    "Chengdu East": ("chengdu", 104.14, 30.63),
    "GBA region": ("greater-bay-area", 113.55, 22.75),
    "Shenzhen": ("greater-bay-area", 114.06, 22.58),
    "Hong Kong": ("greater-bay-area", 114.17, 22.30),
    "Guangzhou": ("greater-bay-area", 113.27, 23.13),
}
SCREENS = {"laptop 1440x900": (1440, 900), "phone 390x844": (390, 844)}
ZOOMS = [6, 8, 9.5, 10.6, 12, 13, 14]


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


def main():
    worst = 0
    readers = {}
    print(f"{'view':16} {'screen':16} " + " ".join(f"z{z:<5}" for z in ZOOMS) + "   (KB, compressed)")
    for name, (slug, lon, lat) in VIEWS.items():
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
