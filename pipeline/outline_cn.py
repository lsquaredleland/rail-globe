"""Outlines for the China regions: one Nominatim lookup per area, timed. Writes regions/china.geojson (PRC + Hong Kong + Macao)
and regions/greater-bay-area.geojson (9 Pearl River Delta cities + Hong Kong + Macao)."""
import json, subprocess, time, urllib.parse
from shapely.geometry import shape, mapping
from shapely.ops import unary_union

def lookup(q, thr):
    t = time.time()
    url = f"https://nominatim.openstreetmap.org/search?q={urllib.parse.quote(q)}&format=json&limit=1&polygon_geojson=1&polygon_threshold={thr}"
    r = subprocess.run(["curl", "-s", "-m", "120", "-A", "rail-globe/0.1", url], capture_output=True, text=True).stdout
    d = json.loads(r)[0]
    print(f"  {q}: {d['osm_type']} {d['osm_id']} {d['geojson']['type']} {len(r)/1e3:.0f} KB in {time.time()-t:.1f} s", flush=True)
    time.sleep(1.1)
    return d

T0 = time.time()
sar = {n: lookup(q, 0.0003) for n, q in (("hk", "Hong Kong"), ("macau", "Macau"))}
cities = {n: lookup(f"{n}, 广东省, 中国", 0.0003) for n in ("广州市", "深圳市", "珠海市", "佛山市", "惠州市", "东莞市", "中山市", "江门市", "肇庆市")}
cn = lookup("中国", 0.005)
for k, d in {**sar, **cities, "china": cn}.items():
    json.dump([d], open(f"regions/cn/{k}.json", "w"), ensure_ascii=False)

g = lambda d: shape(d["geojson"]).buffer(0)
gba = unary_union([g(d) for d in list(sar.values()) + list(cities.values())]).buffer(0)
china = unary_union([g(cn), g(sar["hk"]), g(sar["macau"])]).buffer(0.01).buffer(0)   # +1 km so simplified coasts do not clip rail
for slug, geom in (("greater-bay-area", gba), ("china", china)):
    json.dump({"type": "Feature", "properties": {"slug": slug}, "geometry": mapping(geom)}, open(f"regions/{slug}.geojson", "w"))
    print(slug, geom.geom_type, [round(x, 2) for x in geom.bounds], f"{len(json.dumps(mapping(geom)))/1e3:.0f} KB")
print(f"outlines total {time.time()-T0:.1f} s")
