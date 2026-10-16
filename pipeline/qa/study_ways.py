#!/usr/bin/env python3
"""Per-way table for the freight-vs-passenger study. For every non-yard, open rail way of a country in data/world:
length_km, our type and its source, whether a passenger route relation uses it, the share of the way within 30 m of NARN passenger
track (US/CA), a train station within 200 m of the way, passenger-routed track within 25 m (beside), operator/name/usage/ref tags.
Output: data/qa/freight-study/<slug>.jsonl (git-ignored). Needs numpy and shapely 2; NARN from fetch_narn.py (US, CA).
See docs/13-freight-north-america.md  (one object per way).  Usage: study_ways.py us ca"""
import json, math, sys, os, time
import numpy as np, shapely
from shapely import STRtree

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..'))
OUT = ROOT + '/data/qa/freight-study'
R = 6378137.0
PAX_CODES = set('ABCDEIRVO'); BAD_NET = set('XART')
T0 = time.time()
def log(*a): print(f'[{time.time()-T0:6.1f}s]', *a, flush=True)

def merc(c):
    c = np.asarray(c, float)[:, :2]
    return np.column_stack([R * np.radians(c[:, 0]), R * np.log(np.tan(np.pi / 4 + np.radians(np.clip(c[:, 1], -85, 85)) / 2))])
def coslat(y): return 1.0 / np.cosh(y / R)
def lines_of(g):
    return [g['coordinates']] if g['type'] == 'LineString' else (g['coordinates'] if g['type'] == 'MultiLineString' else [])

def narn(country):
    d = json.load(open(f'{ROOT}/data/ext/ntad/narn_passenger.geojson'))
    gs = []
    for f in d['features']:
        p = f['properties']
        if p['COUNTRY'] != country or p['PASSNGR'] not in PAX_CODES or p['NET'] in BAD_NET: continue
        for l in lines_of(f['geometry']):
            if len(l) >= 2: gs.append(shapely.linestrings(merc(l)))
    return np.array(gs, dtype=object)

def main(slug):
    country = {'us': 'US', 'ca': 'CA'}.get(slug)
    ways = []
    for line in open(f'{ROOT}/data/world/{slug}/features.typed.geojsonl'):
        if '"type":"' not in line: continue
        f = json.loads(line); pr = f['properties']; g = f['geometry']
        if g['type'] not in ('LineString', 'MultiLineString') or 'type' not in pr: continue
        if pr.get('railway') in ('construction', 'proposed') or pr['type'] in ('yard',): continue
        for l in lines_of(g):
            if len(l) >= 2:
                ways.append((pr, l))
                break
        else:
            continue
        # a multi-line way keeps its first part only (ways are LineStrings in practice)
    log(slug, len(ways), 'ways')
    geoms = np.array([shapely.linestrings(merc(l)) for _, l in ways], dtype=object)
    b = shapely.bounds(geoms); cs = coslat((b[:, 1] + b[:, 3]) / 2)
    L = shapely.length(geoms) * cs                       # metres
    n = np.maximum(1, np.ceil(L / 20)).astype(int)
    idx = np.repeat(np.arange(len(geoms)), n)
    start = np.cumsum(np.r_[0, n[:-1]]); k = np.arange(len(idx)) - np.repeat(start, n)
    frac = (k + 0.5) / np.repeat(n, n)
    pts = shapely.line_interpolate_point(geoms[idx], frac, normalized=True)
    log('samples', len(pts))
    def share(tree_geoms, dist_m):
        t = STRtree(tree_geoms)
        d = dist_m / cs[idx]
        hit = np.zeros(len(pts), bool)
        for s in range(0, len(pts), 400000):
            sl = slice(s, s + 400000)
            # one buffer radius per chunk is wrong near poles; per-sample radii via query with dwithin per distance bucket
            a, bb = t.query(pts[sl], predicate='dwithin', distance=d[sl])
            hit[s + a] = True
        return np.bincount(idx, weights=hit, minlength=len(geoms)) / n
    narn_frac = share(narn(country), 30) if country else np.zeros(len(geoms))
    log('narn done')
    # passenger-routed track (own route relation) for "beside"
    routed = np.array([bool(pr.get('_routes')) and pr.get('type_source') == 'route' for pr, _ in ways])
    ptypes = np.array([pr['type'] in ('intercity', 'regional', 'high_speed', 'metro', 'light_rail', 'monorail', 'funicular') for pr, _ in ways])
    rg_idx = np.where(routed & ptypes)[0]
    rg = geoms[rg_idx]
    beside = np.zeros(len(geoms))
    if len(rg):
        t = STRtree(rg)
        d = 25 / cs[idx]
        hit = np.zeros(len(pts), bool)
        # a way is "beside" routed track only through another way
        a, bb = t.query(pts, predicate='dwithin', distance=d)
        keep = rg_idx[bb] != idx[a]
        hit[a[keep]] = True
        beside = np.bincount(idx, weights=hit, minlength=len(geoms)) / n
    log('beside done')
    # train stations (pipeline's complexes with a train mode) within 200 m of the way
    st = []
    for line in open(f'{ROOT}/data/world/{slug}/stations.geojsonl'):
        f = json.loads(line)
        if 'train' in f['properties']['modes']:
            st.append(shapely.points(*merc([f['geometry']['coordinates']])[0]))
    near_station = np.zeros(len(geoms), bool)
    if st:
        t = STRtree(geoms)
        sp = np.array(st, dtype=object)
        sc = coslat(shapely.get_y(sp))
        for i in range(len(sp)):
            hits = t.query(sp[i], predicate='dwithin', distance=200 / sc[i])
            near_station[hits] = True
    log('stations done')
    os.makedirs(OUT, exist_ok=True)
    with open(f'{OUT}/{slug}.jsonl', 'w') as f:
        for i, (pr, _) in enumerate(ways):
            f.write(json.dumps({
                'id': pr.get('osm_id'), 'km': round(float(L[i]) / 1000, 4), 'type': pr['type'], 'src': pr.get('type_source'), 'own': pr.get('own_type'),
                'routed': bool(pr.get('_routes')), 'narn': round(float(narn_frac[i]), 3), 'beside': round(float(beside[i]), 3), 'stn': bool(near_station[i]),
                'name': pr.get('name'), 'op': pr.get('operator'), 'ops': pr.get('_operators'), 'ref': pr.get('ref'), 'usage': pr.get('usage'), 'ms': pr.get('maxspeed'),
                'tm': pr.get('railway:traffic_mode'), 'svc': pr.get('service'), 'net': pr.get('network'),
            }) + '\n')
    log('wrote', len(ways))

for s in sys.argv[1:]:
    main(s)
