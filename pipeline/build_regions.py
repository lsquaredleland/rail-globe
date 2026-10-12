"""Turn the Nominatim boundary dumps in regions/ into one GeoJSON per pilot region, plus an osmium extract config."""
import glob, json
from shapely.geometry import shape, mapping
from shapely.ops import unary_union

def geom(f):
    return shape(json.load(open(f))[0]['geojson'])

regions = {
    'sf-bay-area': unary_union([geom(f) for f in sorted(glob.glob('regions/ba_*.json'))]),
    'paris': geom('regions/idf.json'),
    'chengdu': geom('regions/chengdu.json'),
}
cfg = {'directory': '../data', 'extracts': []}
for slug, g in regions.items():
    g = g.buffer(0)
    json.dump({'type': 'Feature', 'properties': {'slug': slug}, 'geometry': mapping(g)}, open(f'regions/{slug}.geojson', 'w'))
    print(slug, g.geom_type, [round(x, 2) for x in g.bounds])
    cfg['extracts'].append({'output': f'{slug}.osm.pbf', 'output_format': 'pbf', 'polygon': {'file_name': f'regions/{slug}.geojson', 'file_type': 'geojson'}})
json.dump(cfg, open('extracts.json', 'w'), indent=2)
