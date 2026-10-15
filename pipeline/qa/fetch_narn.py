#!/usr/bin/env python3
"""Fetch NTAD North American Rail Network passenger segments + Amtrak stations (public domain).
Usage: python3 -I pipeline/qa/fetch_narn.py [outdir]   (default data/ext/ntad, must be empty or hold only our files)
Downloaded data is untrusted: only parsed as JSON, never executed."""
import json, os, sys, time, datetime, urllib.request, urllib.parse

HOST = "https://services.arcgis.com/xOi1kZaI0eWDREZv/ArcGIS/rest/services"
LAYERS = {
    "narn_passenger.geojson": (f"{HOST}/NTAD_North_American_Rail_Network_Lines/FeatureServer/0",
                               "PASSNGR IS NOT NULL", "PASSNGR,NET,SUBDIV,BRANCH,RROWNER1,TRACKS,COUNTRY,STATEAB,KM"),
    "amtrak_stations.geojson": (f"{HOST}/NTAD_Amtrak_Stations/FeatureServer/0", "1=1", "*"),
}
PAGE = 2000

def get(url, params, tries=6):
    q = url + "?" + urllib.parse.urlencode(params)
    for i in range(tries):
        try:
            with urllib.request.urlopen(urllib.request.Request(q, headers={"User-Agent": "rail-globe-qa"}), timeout=120) as r:
                d = json.loads(r.read())
            if "error" in d:
                raise RuntimeError(d["error"])
            return d
        except Exception as e:
            if i == tries - 1:
                raise
            print(f"  retry {i+1}: {e}", file=sys.stderr)
            time.sleep(2 ** i)

def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "data/ext/ntad"
    os.makedirs(out, exist_ok=True)
    info = {"date": datetime.datetime.now(datetime.timezone.utc).isoformat(), "layers": {}}
    for fn, (url, where, fields) in LAYERS.items():
        total = get(url + "/query", {"where": where, "returnCountOnly": "true", "f": "json"})["count"]
        feats, off = [], 0
        while True:
            d = get(url + "/query", {"where": where, "outFields": fields, "outSR": 4326, "f": "geojson",
                                     "resultOffset": off, "resultRecordCount": PAGE, "orderByFields": "OBJECTID"})
            fs = d.get("features", [])
            feats += fs
            print(f"{fn}: {len(feats)}/{total}")
            if not fs:
                break
            off += len(fs)
            if len(feats) >= total:
                break
        with open(os.path.join(out, fn), "w") as f:
            json.dump({"type": "FeatureCollection", "features": feats}, f)
        info["layers"][fn] = {"url": url, "where": where, "outFields": fields, "reported_count": total,
                              "fetched_count": len(feats), "bytes": os.path.getsize(os.path.join(out, fn))}
    with open(os.path.join(out, "fetched.json"), "w") as f:
        json.dump(info, f, indent=2)
    print(json.dumps(info, indent=2))

main()
