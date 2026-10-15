#!/usr/bin/env python3
"""Cross-check the pipeline's passenger classification against NTAD North American Rail Network
(PASSNGR codes). Measurement only. Usage:
  python3 pipeline/qa/narn_check.py (not -I: numpy/shapely live in the user site; sys.path is only pipeline/qa, never data/) [slug ...]        (default: us ca)
Reads data/ext/ntad/narn_passenger.geojson (see fetch_narn.py) and data/<slug>/features.typed.geojsonl.
Writes data/qa/narn/<slug>/{summary.md, disagreements.geojson, groups_*.csv}.
Geometry: Web Mercator, distances corrected per sample by cos(lat) (no pyproj needed)."""
import csv, json, math, os, sys, time, collections
import numpy as np, shapely
from shapely import STRtree

NARN = "data/ext/ntad/narn_passenger.geojson"
STEP, NEAR = 20.0, 30.0
PAX_CODES = set("ABCDEIRVO")
BAD_NET = set("XART")                 # out of service / abandoned / trail: not live passenger track
RAIL_RAILWAYS = {"rail", "light_rail", "subway", "tram", "narrow_gauge", "monorail", "funicular"}
PAX_TYPES = ["intercity", "regional", "high_speed", "metro", "light_rail", "monorail", "funicular"]
MAIN_PAX = ["intercity", "regional", "high_speed"]
COUNTRY = {"us": "US", "ca": "CA"}
R = 6378137.0
T0 = time.time()

def log(*a):
    print(f"[{time.time()-T0:6.1f}s]", *a, flush=True)

def merc(c):
    c = np.asarray(c, float)[:, :2]
    x = R * np.radians(c[:, 0])
    y = R * np.log(np.tan(np.pi / 4 + np.radians(np.clip(c[:, 1], -85, 85)) / 2))
    return np.column_stack([x, y])

def lat_cos(y):                       # cos(lat) from mercator y
    return 1.0 / np.cosh(y / R)

def explode_lines(geom):
    if geom["type"] == "LineString":
        return [geom["coordinates"]]
    if geom["type"] == "MultiLineString":
        return geom["coordinates"]
    return []

def build(parts):
    """parts: list of (key, coords) -> (geoms array, owner index array)."""
    g, o = [], []
    for k, c in parts:
        if len(c) >= 2:
            g.append(shapely.linestrings(merc(c))); o.append(k)
    return np.array(g, dtype=object), np.array(o)

def samples(geoms, chunk=300000):
    """yield (way_idx array, points array, weight_m array, cos array) in chunks."""
    b = shapely.bounds(geoms)
    cs = lat_cos((b[:, 1] + b[:, 3]) / 2)
    L = shapely.length(geoms) * cs
    n = np.maximum(1, np.ceil(L / STEP)).astype(int)
    i, N = 0, len(geoms)
    while i < N:
        j, tot = i, 0
        while j < N and (tot < chunk or j == i):
            tot += n[j]; j += 1
        idx = np.repeat(np.arange(i, j), n[i:j])
        start = np.cumsum(np.r_[0, n[i:j][:-1]])
        k = np.arange(len(idx)) - np.repeat(start, n[i:j])
        frac = (k + 0.5) / np.repeat(n[i:j], n[i:j])
        pts = shapely.line_interpolate_point(geoms[idx], frac, normalized=True)
        yield idx, pts, (L / n)[idx], cs[idx]
        i = j

def near_mask(tree, pts, cs):
    """bool per point: within NEAR metres of tree geometries."""
    out = np.zeros(len(pts), bool)
    if tree is None or len(pts) == 0:
        return out
    md = NEAR / max(cs.min(), 0.05) * 1.02
    (qi, _), d = tree.query_nearest(pts, max_distance=md, return_distance=True, all_matches=False)
    out[qi] = d * cs[qi] <= NEAR
    return out

def share(geoms, trees, prefilter):
    """per geom: weighted share of length within NEAR of each tree. prefilter: tree to cut candidates."""
    G = len(geoms)
    res = {k: np.zeros(G) for k in trees}
    tot = np.zeros(G)
    b = shapely.bounds(geoms)
    pad = NEAR * 1.5 / lat_cos((b[:, 1] + b[:, 3]) / 2)
    boxes = shapely.box(b[:, 0] - pad, b[:, 1] - pad, b[:, 2] + pad, b[:, 3] + pad)
    cand = np.unique(prefilter.query(boxes)[0]) if len(geoms) else np.array([], int)
    log(f"  candidates {len(cand)}/{G}")
    L = shapely.length(geoms) * lat_cos((b[:, 1] + b[:, 3]) / 2)
    tot[:] = L
    sub = geoms[cand]
    for idx, pts, w, cs in samples(sub):
        gi = cand[idx]
        for k, t in trees.items():
            m = near_mask(t, pts, cs)
            np.add.at(res[k], gi[m], w[m])
    return {k: np.minimum(v / np.maximum(tot, 1e-9), 1.0) for k, v in res.items()}, tot

def load_narn(country):
    d = json.load(open(NARN))
    parts = {"pax": [], "tour": []}
    meta = []                         # for pax: per part
    allp = []
    for f in d["features"]:
        p = f["properties"]
        code, net = p.get("PASSNGR"), p.get("NET")
        for c in explode_lines(f["geometry"]):
            allp.append((code, net, p, c))
    return allp, country

def km(x):
    return x / 1000.0

def grp_csv(path, rows, cols):
    with open(path, "w", newline="") as f:
        w = csv.writer(f); w.writerow(cols)
        for r in rows:
            w.writerow([r[c] for c in cols])

def md_table(head, rows):
    s = "| " + " | ".join(head) + " |\n|" + "---|" * len(head) + "\n"
    for r in rows:
        s += "| " + " | ".join(str(x) for x in r) + " |\n"
    return s + "\n"

def run(slug, narn_all):
    t_slug = time.time()
    out = f"data/qa/narn/{slug}"; os.makedirs(out, exist_ok=True)
    cc = COUNTRY.get(slug)
    # NARN geometry sets
    pax, tour, pax_meta = [], [], []
    for code, net, p, c in narn_all:
        if code in PAX_CODES and net not in BAD_NET:
            pax.append(c); pax_meta.append((code, net, p, c))
        elif code == "T":
            tour.append(c)
    log(f"{slug}: NARN passenger parts {len(pax)}, tourist parts {len(tour)}")
    pg, _ = build([(i, c) for i, c in enumerate(pax)]); ptree = STRtree(pg)
    tg, _ = build([(i, c) for i, c in enumerate(tour)]); ttree = STRtree(tg)
    # pipeline ways
    ways, meta = [], []
    path = f"data/{slug}/features.typed.geojsonl"
    skipped = collections.Counter()
    for line in open(path):
        if '"LineString"' not in line:
            continue
        f = json.loads(line); p = f["properties"]; g = f["geometry"]
        if g["type"] != "LineString" or p.get("railway") not in RAIL_RAILWAYS:
            continue
        t = p.get("type")
        if t is None or t == "yard":
            skipped[t or "none"] += 1; continue
        ways.append(g["coordinates"])
        meta.append((p.get("osm_id"), t, p.get("name") or "", p.get("operator") or "", p.get("railway"), p.get("type_source") or ""))
    wg, wo = build([(i, c) for i, c in enumerate(ways)])
    meta = [meta[i] for i in wo]; ways = [ways[i] for i in wo]
    log(f"{slug}: {len(wg)} ways loaded (skipped {dict(skipped)})")
    types = np.array([m[1] for m in meta])
    # --- pipeline -> NARN
    both = STRtree(np.concatenate([pg, tg])) if len(tg) else ptree
    sh, L = share(wg, {"pax": ptree, "tour": ttree}, both)
    ps, ts = sh["pax"], sh["tour"]
    log(f"{slug}: way->NARN done")
    tlist = [t for t in PAX_TYPES + ["freight", "other"] if (types == t).any()]
    tlist += [t for t in sorted(set(types)) if t not in tlist]
    rows1, conf = [], []
    for t in tlist:
        m = types == t
        km_t = km(L[m].sum())
        hi = km(L[m & (ps >= .6)].sum()); mid = km(L[m & (ps >= .1) & (ps < .6)].sum()); lo = km(L[m & (ps < .1)].sum())
        tour_km = km(L[m & (ps < .6) & (ts >= .6)].sum())
        rows1.append([t, int(m.sum()), f"{km_t:,.0f}", f"{hi:,.0f} ({hi/max(km_t,1e-9):.0%})",
                      f"{mid:,.0f} ({mid/max(km_t,1e-9):.0%})", f"{lo:,.0f} ({lo/max(km_t,1e-9):.0%})", f"{tour_km:,.0f}"])
    # confusion: pipeline passenger-types vs NARN passenger
    pax_m = np.isin(types, PAX_TYPES)
    main_m = np.isin(types, MAIN_PAX)
    def cm(m):
        return [km(L[m & (ps >= .6)].sum()), km(L[m & (ps >= .1) & (ps < .6)].sum()), km(L[m & (ps < .1)].sum())]
    a, b_, c_ = cm(pax_m); a2, b2, c2 = cm(main_m)
    # --- disagreement classes
    key = lambda i: (meta[i][1], meta[i][2], meta[i][3])
    A = np.where(pax_m & (ps < .1))[0]
    B = np.where(np.isin(types, ["freight", "other"]) & (ps >= .6))[0]
    def groups(ix):
        g = collections.defaultdict(lambda: [0.0, 0, collections.Counter(), collections.Counter()])
        for i in ix:
            e = g[key(i)]; e[0] += L[i]; e[1] += 1; e[2][meta[i][5]] += 1
            e[3][round(ps[i], 1)] += 1
        rows = [dict(type=k[0], name=k[1], operator=k[2], km=round(km(v[0]), 1), ways=v[1],
                     type_source=v[2].most_common(1)[0][0],
                     median_pax_overlap=round(float(np.median([ps[i] for i in ix if key(i) == k])), 2) if False else "")
                for k, v in g.items()]
        rows.sort(key=lambda r: -r["km"])
        for r in rows:
            r.pop("median_pax_overlap")
        return rows
    gA, gB = groups(A), groups(B)
    cols = ["type", "name", "operator", "km", "ways", "type_source"]
    grp_csv(f"{out}/groups_pipeline_pax_narn_none.csv", gA[:60], cols)
    grp_csv(f"{out}/groups_pipeline_freight_narn_pax.csv", gB[:60], cols)
    # --- NARN -> pipeline (missing passenger)
    if cc:
        sel = [i for i, (code, net, p, c) in enumerate(pax_meta) if p.get("COUNTRY") == cc]
    else:
        sel = list(range(len(pax_meta)))
    ng = pg[sel]; nmeta = [pax_meta[i] for i in sel]
    tpax = STRtree(wg[pax_m]) if pax_m.any() else None
    nonpax = lambda ts_: STRtree(wg[np.isin(types, ts_)]) if np.isin(types, ts_).any() else None
    # yard ways are not loaded; nearby class for diagnostics uses freight/other only
    tfrt, toth = nonpax(["freight"]), nonpax(["other"])
    nb = ng.shape[0]
    covered = np.zeros(nb); tot = np.zeros(nb); near_f = np.zeros(nb); near_o = np.zeros(nb)
    cs_all = lat_cos((shapely.bounds(ng)[:, 1] + shapely.bounds(ng)[:, 3]) / 2)
    Ln = shapely.length(ng) * cs_all
    for idx, pts, w, cs in samples(ng):
        m = near_mask(tpax, pts, cs)
        np.add.at(covered, idx[m], w[m])
        np.add.at(tot, idx, w)
        um = ~m
        if um.any():
            for arr, t in ((near_f, tfrt), (near_o, toth)):
                mm = near_mask(t, pts[um], cs[um])
                np.add.at(arr, idx[um][mm], w[um][mm])
    unc = 1 - covered / np.maximum(tot, 1e-9)
    miss = np.where(unc >= .6)[0]
    narn_total = km(Ln.sum()); narn_cov = km(covered.sum())
    gm = collections.defaultdict(lambda: [0.0, 0.0, 0.0, 0])
    for i in miss:
        p = nmeta[i][2]; k = (p.get("PASSNGR"), p.get("SUBDIV") or p.get("BRANCH") or "", p.get("RROWNER1") or "")
        e = gm[k]; e[0] += Ln[i] * unc[i]; e[1] += near_f[i]; e[2] += near_o[i]; e[3] += 1
    gC = sorted([dict(passngr=k[0], subdiv=k[1], owner=k[2], uncovered_km=round(km(v[0]), 1), segs=v[3],
                      pipeline_freight_nearby_km=round(km(v[1]), 1), pipeline_other_nearby_km=round(km(v[2]), 1))
                 for k, v in gm.items()], key=lambda r: -r["uncovered_km"])
    grp_csv(f"{out}/groups_narn_pax_no_pipeline_pax.csv", gC[:60],
            ["passngr", "subdiv", "owner", "uncovered_km", "segs", "pipeline_freight_nearby_km", "pipeline_other_nearby_km"])
    by_code = collections.defaultdict(lambda: [0.0, 0.0])
    for i in range(nb):
        c = nmeta[i][0]; by_code[c][0] += Ln[i]; by_code[c][1] += Ln[i] * (1 - unc[i])
    # --- geojson
    feats = []
    for i in A:
        feats.append({"type": "Feature", "geometry": {"type": "LineString", "coordinates": ways[i]},
                      "properties": {"class": "pipeline_pax_narn_none", "osm_id": meta[i][0], "type": meta[i][1], "name": meta[i][2],
                                     "operator": meta[i][3], "type_source": meta[i][5], "narn_pax_share": round(float(ps[i]), 2)}})
    for i in B:
        feats.append({"type": "Feature", "geometry": {"type": "LineString", "coordinates": ways[i]},
                      "properties": {"class": "pipeline_freight_narn_pax", "osm_id": meta[i][0], "type": meta[i][1], "name": meta[i][2],
                                     "operator": meta[i][3], "type_source": meta[i][5], "narn_pax_share": round(float(ps[i]), 2)}})
    for i in miss:
        code, net, p, c = nmeta[i]
        feats.append({"type": "Feature", "geometry": {"type": "LineString", "coordinates": c},
                      "properties": {"class": "narn_pax_no_pipeline_pax", "passngr": code, "net": net, "subdiv": p.get("SUBDIV"),
                                     "owner": p.get("RROWNER1"), "uncovered_share": round(float(unc[i]), 2)}})
    json.dump({"type": "FeatureCollection", "features": feats}, open(f"{out}/disagreements.geojson", "w"))
    log(f"{slug}: disagreements written ({len(feats)} features)")
    # --- summary
    s = f"# NARN passenger cross-check: {slug}\n\nNARN passenger = PASSNGR in A,B,C,D,E,I,R,V,O with NET not in X,A,R,T; tourist = T. Near = within {NEAR:.0f} m (samples every {STEP:.0f} m). Pipeline ways: railway in {sorted(RAIL_RAILWAYS)}, type not yard; construction/proposed excluded.\n\n"
    s += f"Pipeline km considered: {km(L.sum()):,.0f}. NARN passenger km ({cc or 'all'}): {narn_total:,.0f}.\n\n"
    s += "## Pipeline type vs NARN passenger overlap\n\n" + md_table(
        ["pipeline type", "ways", "km", "overlap >=60%", "10-60%", "<10%", "km tourist-overlap >=60% (not pax)"], rows1)
    s += "## Confusion: pipeline passenger types vs NARN passenger (km)\n\n" + md_table(
        ["pipeline set", "NARN pax >=60%", "10-60%", "<10%", "share agreeing"],
        [["all passenger types (" + ", ".join(PAX_TYPES) + ")", f"{a:,.0f}", f"{b_:,.0f}", f"{c_:,.0f}", f"{a/max(a+b_+c_,1e-9):.0%}"],
         ["intercity+regional+high_speed", f"{a2:,.0f}", f"{b2:,.0f}", f"{c2:,.0f}", f"{a2/max(a2+b2+c2,1e-9):.0%}"]])
    s += "Pipeline freight/other against NARN passenger: " + ", ".join(
        f"{t} {km(L[(types==t)&(ps>=.6)].sum()):,.0f} km of {km(L[types==t].sum()):,.0f}" for t in ["freight", "other"] if (types == t).any()) + "\n\n"
    s += "## NARN passenger coverage by code (km, share with pipeline passenger track within 30 m)\n\n" + md_table(
        ["PASSNGR", "NARN km", "covered km", "covered share"],
        [[c, f"{v[0]/1000:,.0f}", f"{v[1]/1000:,.0f}", f"{v[1]/max(v[0],1e-9):.0%}"] for c, v in sorted(by_code.items())])
    s += f"NARN passenger km with no pipeline passenger track within 30 m (segments >=60% uncovered): {sum(g['uncovered_km'] for g in gC):,.0f} km in {len(miss)} segments. Total uncovered incl. partial: {narn_total-narn_cov:,.0f} km.\n\n"
    def top(rows, cols_, n=60):
        return md_table(cols_, [[r[c] for c in cols_] for r in rows[:n]])
    sA_all = sum(r["km"] for r in gA); sA_main = sum(r["km"] for r in gA if r["type"] in MAIN_PAX)
    s += f"## Class 1: pipeline passenger, NARN none (<10%): {sA_all:,.0f} km total, {sA_main:,.0f} km intercity/regional/high_speed\n\nTop 60 groups (type, name, operator):\n\n" + top(gA, cols[:5])
    s += f"## Class 1b: same, intercity/regional/high_speed only\n\n" + top([r for r in gA if r["type"] in MAIN_PAX], cols[:5], 40)
    s += f"## Class 2: pipeline freight/other, NARN passenger (>=60%): {sum(r['km'] for r in gB):,.0f} km\n\n" + top(gB, cols[:5])
    s += f"## Class 3: NARN passenger, no pipeline passenger track\n\n" + top(gC, ["passngr", "subdiv", "owner", "uncovered_km", "segs", "pipeline_freight_nearby_km", "pipeline_other_nearby_km"])
    s += f"\nRuntime for this slug: {time.time()-t_slug:.0f} s\n"
    open(f"{out}/summary.md", "w").write(s)
    log(f"{slug}: done in {time.time()-t_slug:.0f}s")

def main():
    slugs = sys.argv[1:] or ["us", "ca"]
    log("loading NARN")
    d = json.load(open(NARN))
    narn_all = []
    for f in d["features"]:
        p = f["properties"]
        for c in explode_lines(f["geometry"]):
            narn_all.append((p.get("PASSNGR"), p.get("NET"), p, c))
    for s in slugs:
        run(s, narn_all)

main()
