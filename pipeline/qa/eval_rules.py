#!/usr/bin/env python3
"""Scores "is this mainline way passenger?" rules against the NARN passenger inventory, km-weighted, from the tables study_ways.py wrote.
Usage: eval_rules.py us ca     (docs/13-freight-north-america.md has the results and the reading)"""
import json, os, re, sys, collections

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..'))
PX = ('high_speed', 'intercity', 'regional')
# passenger agencies, matched against the operator tags only (word boundaries: "trackage" must not match ACE)
AGENCY = re.compile(r'\b(Amtrak|Metra|Metro-North|Long Island Rail Road|SEPTA|NJ ?Transit|New Jersey Transit|MARC|Maryland Area Regional|Caltrain|Peninsula Corridor|Virginia Railway Express|Metrolink|Southern California Regional Rail|Sound Transit|North County Transit|Brightline|MBTA|Massachusetts Bay Transportation|Tri-Rail|South Florida Regional|Northern Indiana Commuter|Trinity Railway|GO Transit|Metrolinx|VIA Rail|Capital Metro|Utah Transit|Rio Metro|Altamont Corridor|CTrail|Connecticut Department of Transportation|Shore Line East|Denton County|exo|West Coast Express|Alaska Railroad|NICTD|Northstar|Sounder|Coaster|FrontRunner)\b', re.I)

def agency(w):
    return bool(AGENCY.search(' '.join(filter(None, [w['op'], ' '.join(w['ops'] or [])]))))

def main(c):
    W = [json.loads(l) for l in open(f'{ROOT}/data/qa/freight-study/{c}.jsonl')]
    U = [w for w in W if w['type'] in {'high_speed', 'intercity', 'regional', 'freight', 'other'}]
    truth = sum(w['km'] * w['narn'] for w in U)
    print(f'{c}: mainline km {sum(w["km"] for w in U):.0f}, NARN passenger km inside it {truth:.0f}')
    def ev(name, f):
        tp = fp = 0.0
        for w in U:
            if f(w): tp += w['km'] * w['narn']; fp += w['km'] * (1 - w['narn'])
        print(f'  {name:44s} passenger km {tp + fp:8.0f}  precision {100 * tp / max(tp + fp, 1):5.1f}%  recall {100 * tp / max(truth, 1):5.1f}%  not-passenger km called passenger {fp:7.0f}')
    px = lambda w: w['type'] in PX
    route = lambda w: w['src'] == 'route'
    ev('current typing', px)
    ev('on a passenger route', lambda w: px(w) and route(w))
    ev('route, or beside routed track (>=50%, 25 m)', lambda w: px(w) and (route(w) or w['beside'] >= .5))
    ev('... or a passenger agency is the operator', lambda w: px(w) and (route(w) or w['beside'] >= .5 or agency(w)))
    ev('... or a train station within 200 m', lambda w: px(w) and (route(w) or w['beside'] >= .5 or agency(w) or w['stn']))

for c in sys.argv[1:] or ['us', 'ca']:
    main(c)
