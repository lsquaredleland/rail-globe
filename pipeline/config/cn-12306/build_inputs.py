#!/usr/bin/env python3
"""Refresh timetable.json, stations.tsv, meta.json and city_en.json from the china-rail checkout (default ~/Downloads/china-rail) and the
corridor-v3 handoff (default ~/Downloads/china-rail-handoff-v3, read only).

    python3 pipeline/config/cn-12306/build_inputs.py [--china-rail DIR] [--timetable FILE] [--intercity FILE] [--handoff DIR] [--world DIR]

Reads china-rail's deliverable/china-rail-timetables-<date>.json (newest by default; the trunk trains), the handoff's
output/china-rail-intercity-<date>.json (the C and S intercity trains, a later day; none with --intercity none), derived/stations.json,
harvest/osm_stations.json, harvest/station_resolution.json and harvest/pair_distances.json of china-rail (plus the handoff's
data/derived/cs_out/ pair distances and station resolution for the intercity stops), and osm-rail's data/world/cn/stations.geojsonl.
Trains that share a 12306 train_no are kept once (the trunk's first). meta.json records both days with their classes and train counts;
city_en.json is the handoff's English gazetteer of city and station names. Standard library only.

timetable.json: per train the class and the stops as [name, arrive, depart] ("HH:MM", "" when 12306 has none).
stations.tsv: name, lon, lat, source, english (the English name on the 12306 list, where it has one). Sources: list (china-rail's station list), fix (a name whose point put a pair
more than 3x + 3 km from china-rail's chord_km: moved to the known place of that name that fits its neighbours; fix+list when the
name is on the list too), resolution (china-rail's cluster pick between several places), osm (the only place of that name),
candidate (several places, no pick: the build takes the one nearest the stops on both sides), dropped (no place fits).
"""
import argparse, collections, glob, json, math, os, re

ap = argparse.ArgumentParser()
ap.add_argument('--china-rail', default=os.path.expanduser('~/Downloads/china-rail'))
ap.add_argument('--timetable')
ap.add_argument('--intercity', help='the C and S deliverable [default: the newest output/china-rail-intercity-*.json of --handoff]; "none" for no intercity trains')
ap.add_argument('--handoff', default=os.path.expanduser('~/Downloads/china-rail-handoff-v3'))
ap.add_argument('--world', default=os.path.join(os.path.dirname(__file__), '../../../data/world'))
args = ap.parse_args()
CR, HERE = args.china_rail, os.path.dirname(os.path.abspath(__file__))
nrm = lambda s: re.sub(r'\s+', '', s)
tt = args.timetable or sorted(glob.glob(CR + '/deliverable/china-rail-timetables-*.json'))[-1]
d = json.load(open(tt))
for t in d['trains']:
    t['snapshot'] = 'trunk'
ic = args.intercity or (sorted(glob.glob(args.handoff + '/output/china-rail-intercity-*.json')) or ['none'])[-1]
dic = None if ic == 'none' else json.load(open(ic))
if dic:
    have = {t['train_no'] for t in d['trains']}
    dic['trains'] = [dict(t, snapshot='intercity') for t in dic['trains'] if t['train_no'] not in have]
    d['trains'] = d['trains'] + dic['trains']
STN = json.load(open(CR + '/derived/stations.json'))
OS = json.load(open(CR + '/harvest/osm_stations.json'))
RES = json.load(open(CR + '/harvest/station_resolution.json'))
pd = json.load(open(CR + '/harvest/pair_distances.json'))['pairs']
if dic:
    cs = args.handoff + '/data/derived/cs_out/'
    pd = pd + json.load(open(cs + 'pair_distances.json'))['pairs']
    RES = dict(json.load(open(cs + 'station_resolution.json')), **RES)
chord = {(nrm(p['from']), nrm(p['to'])): p['chord_km'] for p in pd}

# places by name: china-rail's OSM list first, osm-rail's own station points only for names it lacks
cand = collections.defaultdict(list)
for n, v in OS.items():
    for la, lo in v:
        cand[nrm(n)].append((lo, la))
own = collections.defaultdict(list)
for l in open(os.path.join(args.world, 'cn/stations.geojsonl')):
    f = json.loads(l)
    p = f['properties']
    for k in ('name', 'name:zh'):
        if p.get(k):
            nm = re.sub(r'\s+', '', p[k])
            c = tuple(f['geometry']['coordinates'])
            own[nm].append(c)
            if nm.endswith('站'):
                own[nm[:-1]].append(c)
for n, v in own.items():
    if n not in cand:
        for c in v:
            if c not in cand[n]:
                cand[n].append(c)
hv = lambda a, b: ((a[0] - b[0]) * 0.87) ** 2 + (a[1] - b[1]) ** 2


def hav(a, b):
    p1, p2 = math.radians(a[1]), math.radians(b[1])
    x = math.sin((p2 - p1) / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(math.radians(b[0] - a[0]) / 2) ** 2
    return 12742 * math.asin(math.sqrt(x))


def resolve(name, anchor, fix):
    if name in fix:
        return fix[name]
    if name in STN:
        return (STN[name]['lon'], STN[name]['lat'])
    c = cand.get(name)
    if not c:
        return None
    if len(c) == 1:
        return c[0]
    r = RES.get(name)
    if r:
        pts = [(p['point'][1], p['point'][0]) for p in r['clusters_chosen']]
        return min(c, key=lambda x: min(hv(x, p) for p in pts))
    return min(c, key=lambda x: hv(x, anchor)) if anchor else c[0]


def stop_points(t, fix):
    names = [nrm(s['name']) for s in t['stops']]
    un = [(STN[n]['lon'], STN[n]['lat']) if n in STN else cand[n][0] for n in names if n in STN or (n in cand and len(cand[n]) == 1)]
    anchor = (sum(p[0] for p in un) / len(un), sum(p[1] for p in un) / len(un)) if un else None
    return names, [resolve(n, anchor, fix) for n in names]


def scan(fix):
    bad, use = {}, collections.defaultdict(list)
    for t in d['trains']:
        names, pts = stop_points(t, fix)
        for i in range(len(names) - 1):
            a, b = names[i], names[i + 1]
            if a == b or not pts[i] or not pts[i + 1]:
                continue
            k = (a, b) if (a, b) in chord else (b, a) if (b, a) in chord else None
            if not k:
                continue
            ch = hav(pts[i], pts[i + 1])
            use[a].append((b, pts[i + 1], chord[k]))
            use[b].append((a, pts[i], chord[k]))
            if ch > 3 * chord[k] + 3:
                bad[(a, b)] = (round(ch), chord[k])
    return bad, use


# names that put a pair far from china-rail's chord: try every known place of the name, keep the one that fits its neighbours
bad, use = scan({})
fix = {}
for n in sorted({n for k in bad for n in k}):
    places = list(cand.get(n, []))
    for c in own.get(n, []):
        if c not in places:
            places.append(c)
    if not places:
        fix[n] = None
        continue
    err = lambda c: sum(abs(hav(c, p) - km) for _, p, km in use[n]) / max(1, len(use[n]))
    best = min(places, key=err)
    mean = sum(km for _, _, km in use[n]) / max(1, len(use[n]))
    fix[n] = best if err(best) < 0.35 * mean + 5 else None

names = collections.OrderedDict()
for t in d['trains']:
    for s in t['stops']:
        names[nrm(s['name'])] = 1
rows, unplaced = [], 0
for n in names:
    if n in fix:
        rows.append((n, *(fix[n] or (None, None)), 'dropped' if fix[n] is None else 'fix+list' if n in STN else 'fix'))
    elif n in STN:
        rows.append((n, STN[n]['lon'], STN[n]['lat'], 'list'))
    elif not cand.get(n):
        unplaced += 1
    elif len(cand[n]) == 1:
        rows.append((n, *cand[n][0], 'osm'))
    elif n in RES:
        p = resolve(n, None, {})
        rows.append((n, p[0], p[1], 'resolution'))
    else:
        rows += [(n, c[0], c[1], 'candidate') for c in cand[n]]
with open(HERE + '/stations.tsv', 'w') as f:
    f.write('# name, lon, lat, source, english (the English name on the 12306 list, where it has one). Sources: see build_inputs.py\n')
    for n, lo, la, s in rows:
        f.write('%s\t%s\t%s\t%s\t%s\n' % (n, '' if lo is None else '%.6f' % lo, '' if la is None else '%.6f' % la, s, STN[n].get('en') or '' if n in STN else ''))

tm = lambda s: '' if (not s or '-' in s) else s
out = [{'code': t['code'], 'class': t['class'], 'stops': [[nrm(s['name']), tm(s['arrive']), tm(s['depart'])] for s in t['stops']]} for t in d['trains']]
open(HERE + '/timetable.json', 'w').write(json.dumps(out, ensure_ascii=False, separators=(',', ':')))
snaps = []
for name, src, path in (('trunk', d, tt), ('intercity', dic, ic)):
    if src and any(t['snapshot'] == name for t in d['trains']):
        ts = [t for t in d['trains'] if t['snapshot'] == name]
        cls = collections.Counter(t['class'] for t in ts)
        snaps.append({'name': name, 'date': src['date'], 'classes': [c for c, _ in cls.most_common()], 'trains': len(ts), 'source': src.get('source'), 'timetable': os.path.basename(path)})
json.dump({'snapshots': snaps, 'trains': len(out)}, open(HERE + '/meta.json', 'w'), indent=1)
if dic:
    gaz = json.load(open(args.handoff + '/data/city_en.json'))
    json.dump(dict(sorted(gaz.items())), open(HERE + '/city_en.json', 'w'), ensure_ascii=False, indent=0)
print('%s: %d trains (%s), %d station names (%d without a place), %d rows; %d fixed or dropped' % (os.path.basename(tt), len(out), ', '.join('%d %s' % (s['trains'], s['name']) for s in snaps), len(names), unplaced, len(rows), len(fix)))
