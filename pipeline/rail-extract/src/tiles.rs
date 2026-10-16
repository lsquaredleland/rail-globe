//! Tile input and side files, ported from pipeline/build_tiles.py: the GeoJSON layers the Rust tile writer (tilewriter.rs) turns into MLT PMTiles
//! (tile input only; main.rs runs the writer afterwards) and the JSON side files the app loads on demand.
//! Layers (docs/02-architecture.md): track (with the merged zoomed-out overview), lines (one strand per line per way), areas,
//! buildings, platforms, stations, complexes, airports, airport_pts. Tiles include each class one zoom before the style shows it.

use crate::classify::speed_kmh;
use crate::geomx::{self, Index};
use crate::pyjson::{self, f, py_round, COMPACT};
use crate::site::{s, Input};
use crate::stations::english_name;
use geo::{Geometry, LineString, Point, Simplify};
use regex::Regex;
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;

const TYPE_ORDER: [&str; 8] = ["high_speed", "intercity", "regional", "metro", "light_rail", "monorail", "funicular", "other"];
// Zoomed out (tile zooms 0..OVERVIEW_MAXZOOM) the track layer holds a merged overview instead of every piece (docs/00-overview.md)
const OVERVIEW_MAXZOOM: i64 = 8;
// Construction and proposed track is drawn as dashes cut in the pipeline (no ids, no tags), layer `con_dash`; the per-way features stay in `track` for
// hover, click and highlight. Yard track is left out. A type's dashes start at its own track_minzoom, but not before CON_DASH_MINZOOM.
const CON_DASH_MINZOOM: i64 = 3;
// The track is merged end to end (per type, proposed flag and speed band), parallel tracks made one line, and cut into real dashes: MapLibre's line-dasharray restarts at tile edges and stretches between whole zooms, so
// the dashes are geometry. One copy per band of CON_BAND_STEP zoom levels from CON_BAND_FIRST; the dash is CON_DASH_PX / CON_GAP_PX on screen at the
// middle of its band (within a band the map scales it by up to 2^CON_BAND_STEP either way). The app shows one band at a time (property `b`).
const CON_BAND_FIRST: f64 = 4.0;
const CON_BAND_STEP: f64 = 0.5;
const CON_BAND_LAST: f64 = 14.5; // the last band is shown from here to the deepest zoom
const CON_DASH_PX: f64 = 12.0;
const CON_GAP_PX: f64 = 8.0;
const SKETCH_MINZOOM: i64 = 4; // the metro sketch (type only) is drawn from here to the zoom before the metro overview starts
const CITY_LINK_KM: f64 = 3.0; // track samples this close (directly or through others) are one city dot
const CITY_ABSORB_KM: f64 = 15.0; // a small group this close to a much bigger one (see CITY_ABSORB_RATIO) is part of that city
const CITY_ABSORB_RATIO: f64 = 5.0;
const CITY_MIN_KM: f64 = 6.0; // a city dot needs this much metro / tram / monorail track
const SPEED_BAND_MINS: [i64; 9] = [400, 350, 300, 250, 200, 160, 120, 80, 0]; // the speed colour bands of app/src/mapStyle.ts
const HL_SIMPLIFY: f64 = 0.001; // degrees, about 100 m: under a pixel at z7
const NAME_SHARDS: u32 = 256;
const CSS_NAMES: [&str; 32] = ["red", "blue", "green", "yellow", "orange", "purple", "pink", "brown", "black", "grey", "gray", "darkblue", "skyblue",
    "lightblue", "darkgreen", "lightgreen", "cyan", "magenta", "violet", "gold", "navy", "teal", "maroon", "lime", "olive", "silver",
    "turquoise", "crimson", "indigo", "darkred", "darkorange", "white"];

fn rank(t: &str) -> usize {
    TYPE_ORDER.iter().position(|x| *x == t).unwrap_or(9)
}

fn track_minzoom(t: &str) -> i64 {
    match t {
        "high_speed" => 0,
        "intercity" => 3,
        "regional" => 5,
        "metro" | "freight" => 7,
        "yard" => 12,
        _ => 8,
    }
}

fn colour(c: Option<&str>) -> Option<String> {
    let c = c?.trim().to_lowercase();
    if c.is_empty() {
        return None;
    }
    let hex = |s: &str| s.chars().all(|ch| ch.is_ascii_hexdigit());
    if c.starts_with('#') && (c.len() == 4 || c.len() == 7) && hex(&c[1..]) {
        return Some(c);
    }
    if c.len() == 6 && hex(&c) {
        return Some(format!("#{c}"));
    }
    CSS_NAMES.contains(&c.as_str()).then_some(c)
}

/// short on-map name of a line: the name before any ": from ↔ to" suffix, else its ref (30 characters at most)
fn label_of(name: Option<&str>, r: Option<&str>) -> String {
    thread_local!(static RE: Regex = Regex::new(r"\s*[:：]\s*").unwrap());
    let n = RE.with(|re| re.split(name.unwrap_or("")).next().unwrap_or("").trim().to_string());
    let v = if !n.is_empty() { n } else { r.unwrap_or("").to_string() };
    v.chars().take(30).collect()
}

/// the line's number for its on-map badge: a ref with a digit (4 characters at most: 2, S5, RE1) or a one or two letter code (M, NS).
/// Longer letter-only codes (Hong Kong's TCL, AEL) are abbreviations, not numbers: those lines show their name instead.
fn badge_of(r: Option<&str>) -> Option<String> {
    let r = r?.trim();
    let n = r.chars().count();
    let ok = !r.contains(';') && n > 0 && (n <= 2 || (n <= 4 && r.chars().any(|c| c.is_ascii_digit())));
    ok.then(|| r.to_string())
}

/// true when a #rgb / #rrggbb colour is light enough that a badge needs dark text on it
fn is_light(c: &str) -> bool {
    if ["white", "yellow", "lime", "cyan", "aqua", "gold", "silver", "lightgreen", "skyblue", "lightblue", "pink", "orange", "beige", "ivory", "khaki", "lightgray", "lightgrey"].contains(&c) {
        return true;
    }
    let h = c.trim_start_matches('#');
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let v = |i: usize, w: usize| u32::from_str_radix(&h[i..i + w], 16).ok().map(|x| if w == 1 { x * 17 } else { x } as f64);
    let (r, g, b) = if h.len() == 3 { (v(0, 1), v(1, 1), v(2, 1)) } else if h.len() == 6 { (v(0, 2), v(2, 2), v(4, 2)) } else { return false };
    match (r, g, b) {
        (Some(r), Some(g), Some(b)) => 0.299 * r + 0.587 * g + 0.114 * b > 160.0,
        _ => false,
    }
}

fn short(rid: &str) -> i64 {
    rid.split_once('/').and_then(|(_, n)| n.parse().ok()).unwrap_or(0)
}

fn coords(g: &Value) -> Vec<(f64, f64)> {
    g["coordinates"].as_array().into_iter().flatten().filter_map(|c| Some((c[0].as_f64()?, c[1].as_f64()?))).collect()
}

pub(crate) fn haversine_len(c: &[(f64, f64)]) -> f64 {
    c.windows(2)
        .map(|w| {
            let ((x1, y1), (x2, y2)) = (w[0], w[1]);
            let dx = (x2 - x1).to_radians() * ((y1 + y2) / 2.0).to_radians().cos();
            let dy = (y2 - y1).to_radians();
            dx.hypot(dy) * 6371.0088
        })
        .sum()
}

/// polygon area in hectares from lon/lat (local equirectangular scale at the centroid), as build_tiles.py area_ha
fn area_ha(g: &Value) -> f64 {
    let Some(geo) = geomx::from_geojson(g, geomx::id) else { return 0.0 };
    let lat = geomx::centroid(&geo).map_or(0.0, |c| c.y());
    py_round(geomx::area(&geo) * 111320.0 * lat.to_radians().cos() * 110540.0 / 10000.0, 2)
}

fn fnv1a(s: &str) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    for b in s.as_bytes() {
        h = (h ^ *b as u32).wrapping_mul(0x01000193);
    }
    h
}

fn safe(i: &str) -> String {
    i.replace('/', "-")
}

/// one tile input line: {"type":"Feature","properties":...,"geometry":...,"zoom":{"minzoom":..,"maxzoom":..}} ("zoom" is read by tilewriter.rs)
fn feat(out: &mut String, props: &Map<String, Value>, geom: &str, minzoom: Option<i64>, maxzoom: Option<i64>) {
    out.push_str("{\"type\":\"Feature\",\"properties\":");
    pyjson::write_map(out, props, COMPACT);
    out.push_str(",\"geometry\":");
    out.push_str(geom);
    if minzoom.is_some() || maxzoom.is_some() {
        out.push_str(",\"zoom\":{");
        if let Some(z) = minzoom {
            out.push_str(&format!("\"minzoom\":{z}"));
        }
        if let Some(z) = maxzoom {
            out.push_str(&format!("{}\"maxzoom\":{z}", if minzoom.is_some() { "," } else { "" }));
        }
        out.push('}');
    }
    out.push_str("}\n");
}

fn write_json(path: &Path, v: &Value) {
    fs::write(path, pyjson::to_string(v, COMPACT)).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

/// shapely's linemerge: pieces joined end to end through points where exactly two of them meet
/// web mercator on the unit square (x east, y south) and back
fn to_merc((lon, lat): (f64, f64)) -> (f64, f64) {
    ((lon + 180.0) / 360.0, 0.5 - (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0).tan().ln() / (2.0 * std::f64::consts::PI))
}
fn from_merc((x, y): (f64, f64)) -> (f64, f64) {
    (x * 360.0 - 180.0, (2.0 * ((0.5 - y) * 2.0 * std::f64::consts::PI).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees())
}

/// cut a line (mercator) into dashes of `dash` with `gap` between, the pattern running along the whole line from its start
fn cut_dashes(line: &[(f64, f64)], dash: f64, gap: f64) -> Vec<Vec<(f64, f64)>> {
    let mut out = Vec::new();
    let (mut cur, mut on, mut left) = (vec![line[0]], true, dash);
    for w in line.windows(2) {
        let (mut a, b) = (w[0], w[1]);
        let mut seg = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        while seg > left {
            let t = left / seg;
            let p = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            if on {
                cur.push(p);
                out.push(std::mem::take(&mut cur));
            } else {
                cur = vec![p];
            }
            seg -= left;
            a = p;
            on = !on;
            left = if on { dash } else { gap };
        }
        left -= seg;
        if on {
            cur.push(b);
        }
    }
    if on && cur.len() > 1 {
        out.push(cur);
    }
    out
}

/// Parallel tracks (double track is mapped as two ways) get the same dashes only if they are one line: each gets its own pattern from its own start, so
/// two dashed lines a few metres apart interleave into a solid one. Lines are taken longest first; a stretch within about CON_PARALLEL_CELL of one already
/// taken is dropped, so each corridor keeps one centreline. Input and output in mercator.
const CON_PARALLEL_CELL: f64 = 1.8e-6; // about 60-80 m
fn dedupe_parallel(mut lines: Vec<Vec<(f64, f64)>>) -> Vec<Vec<(f64, f64)>> {
    let len = |l: &Vec<(f64, f64)>| l.windows(2).map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()).sum::<f64>();
    lines.sort_by(|a, b| len(b).partial_cmp(&len(a)).unwrap_or(std::cmp::Ordering::Equal));
    let cell = |p: (f64, f64)| ((p.0 / CON_PARALLEL_CELL).floor() as i64, (p.1 / CON_PARALLEL_CELL).floor() as i64);
    let mut taken: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for l in lines {
        // samples along the line, half a cell apart
        let mut s: Vec<(f64, f64)> = vec![l[0]];
        for w in l.windows(2) {
            let d = ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
            let n = (d / (CON_PARALLEL_CELL / 2.0)).ceil().max(1.0) as usize;
            for i in 1..=n {
                let t = i as f64 / n as f64;
                s.push((w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t));
            }
        }
        let covered: Vec<bool> = s.iter().map(|&p| { let (cx, cy) = cell(p); (-1..=1).any(|dx| (-1..=1).any(|dy| taken.contains(&(cx + dx, cy + dy)))) }).collect();
        let mut run: Vec<(f64, f64)> = Vec::new();
        for (i, &p) in s.iter().enumerate() {
            if !covered[i] {
                run.push(p);
            } else if !run.is_empty() {
                if run.len() > 1 { out.push(std::mem::take(&mut run)); } else { run.clear(); }
            }
        }
        if run.len() > 1 {
            out.push(run);
        }
        for &p in &s { taken.insert(cell(p)); }
    }
    out
}

/// the dashes of one merged construction line (mercator) for every band (property `b`), each band in the tile zoom where the app shows it
fn con_dashes(out: &mut String, props: &Map<String, Value>, merc: &[(f64, f64)], first_tile_zoom: i64) {
    if merc.len() < 2 {
        return;
    }
    let nb = ((CON_BAND_LAST - CON_BAND_FIRST) / CON_BAND_STEP).round() as i64;
    for b in 0..=nb {
        let z = CON_BAND_FIRST + b as f64 * CON_BAND_STEP;
        let tile_zoom = (z.floor() as i64).min(14);
        if tile_zoom < first_tile_zoom {
            continue;
        }
        let zc = if b == nb { z + CON_BAND_STEP } else { z + CON_BAND_STEP / 2.0 };
        let px = 1.0 / (512.0 * 2f64.powf(zc));
        let mut p = props.clone();
        p.insert("b".into(), json!(b));
        for d in cut_dashes(&merc, CON_DASH_PX * px, CON_GAP_PX * px) {
            let coords: Vec<Value> = d.iter().map(|&q| { let (x, y) = from_merc(q); json!([x, y]) }).collect();
            feat(out, &p, &pyjson::to_string(&json!({"type": "LineString", "coordinates": coords}), COMPACT), Some(tile_zoom), Some(tile_zoom));
        }
    }
}

fn linemerge(lines: Vec<Vec<(f64, f64)>>) -> Vec<Vec<(f64, f64)>> {
    let key = |p: (f64, f64)| (p.0.to_bits(), p.1.to_bits());
    let mut at: FxHashMap<(u64, u64), Vec<usize>> = FxHashMap::default();
    for (i, l) in lines.iter().enumerate() {
        at.entry(key(l[0])).or_default().push(i);
        at.entry(key(*l.last().unwrap())).or_default().push(i);
    }
    let mut used = vec![false; lines.len()];
    let mut out = Vec::new();
    // walk from every line end that is not a simple pass-through (degree != 2), then close up the remaining rings
    let walk = |start: usize, from_end: bool, used: &mut Vec<bool>| -> Vec<(f64, f64)> {
        let mut path: Vec<(f64, f64)> = Vec::new();
        let (mut i, mut rev) = (start, from_end);
        loop {
            used[i] = true;
            let mut l = lines[i].clone();
            if rev {
                l.reverse();
            }
            if path.is_empty() {
                path.extend(l);
            } else {
                path.extend(l.into_iter().skip(1));
            }
            let end = *path.last().unwrap();
            let next = at.get(&key(end)).filter(|v| v.len() == 2).and_then(|v| v.iter().copied().find(|&j| !used[j]));
            match next {
                Some(j) => {
                    rev = key(lines[j][0]) != key(end);
                    i = j;
                }
                None => break,
            }
        }
        path
    };
    for i in 0..lines.len() {
        if used[i] {
            continue;
        }
        let deg_start = at[&key(lines[i][0])].len();
        let deg_end = at[&key(*lines[i].last().unwrap())].len();
        if deg_start != 2 {
            out.push(walk(i, false, &mut used));
        } else if deg_end != 2 {
            out.push(walk(i, true, &mut used));
        }
    }
    for i in 0..lines.len() {
        if !used[i] {
            out.push(walk(i, false, &mut used));
        }
    }
    out
}

/// highlight data of a line or named railway: its track pieces' ids and a simplified geometry (build_tiles.py hl_record)
fn hl_record(ids: &[String], way_geom: &FxHashMap<String, Vec<(f64, f64)>>, extra: Vec<(&str, Value)>) -> Value {
    let lines: Vec<Vec<(f64, f64)>> = ids.iter().filter_map(|i| way_geom.get(i)).filter(|g| g.len() >= 2).cloned().collect();
    let geom = if lines.is_empty() {
        Value::Null
    } else {
        let parts: Vec<Value> = linemerge(lines)
            .into_iter()
            .map(|p| {
                let ls: LineString<f64> = p.into_iter().map(|(x, y)| geo::Coord { x, y }).collect();
                let ls = ls.simplify(HL_SIMPLIFY);
                Value::Array(ls.0.iter().map(|c| json!([f(py_round(c.x, 5)), f(py_round(c.y, 5))])).collect())
            })
            .collect();
        json!({"type": "MultiLineString", "coordinates": parts})
    };
    let mut m = Map::new();
    m.insert("ways".into(), json!(ids));
    m.insert("geom".into(), geom);
    for (k, v) in extra {
        m.insert(k.into(), v);
    }
    Value::Object(m)
}

fn geojson_value(g: &Geometry<f64>) -> Value {
    let ring = |l: &LineString<f64>| Value::Array(l.0.iter().map(|c| json!([f(c.x), f(c.y)])).collect());
    match g {
        Geometry::Polygon(p) => json!({"type": "Polygon", "coordinates": std::iter::once(p.exterior()).chain(p.interiors()).map(ring).collect::<Vec<_>>()}),
        Geometry::MultiPolygon(m) => json!({"type": "MultiPolygon", "coordinates": m.0.iter().map(|p| std::iter::once(p.exterior()).chain(p.interiors()).map(ring).collect::<Vec<_>>()).collect::<Vec<_>>()}),
        Geometry::Point(p) => json!({"type": "Point", "coordinates": [f(p.x()), f(p.y())]}),
        _ => Value::Null,
    }
}

/// the tile layers of one region: (tile layer, file in tiles_tmp), in stacking order (the overview is the first "track" file)
pub const TILE_LAYERS: [(&str, &str); 12] = [("track", "track_overview.geojsonl"), ("track", "track.geojsonl"), ("con_dash", "con_dash.geojsonl"),
    ("lines", "lines.geojsonl"), ("areas", "areas.geojsonl"), ("buildings", "buildings.geojsonl"), ("platforms", "platforms.geojsonl"),
    ("stations", "stations.geojsonl"), ("complexes", "complexes.geojsonl"), ("airports", "airports.geojsonl"),
    ("airport_pts", "airport_pts.geojsonl"), ("cities", "cities.geojsonl")];

pub struct Prepared {
    pub summary: String,
}

/// One dot per city with a metro, tram or monorail system, from the track itself: open passenger ways of type metro, `tram` ways and
/// monorail (not light_rail, which OSM uses for German S-Bahn, and not service track), sampled about every km. Samples within CITY_LINK_KM of
/// each other, directly or through others, are one city (single linkage), so a city is one dot however far it spreads; groups with
/// less than CITY_MIN_KM of track are dropped. Properties: m / l / r = metro / tram / monorail km, n = all km.
fn city_dots(samples: &[(f64, f64, f64, u8)]) -> String {
    let km = |a: (f64, f64), b: (f64, f64)| {
        let dy = (a.1 - b.1) * 111.2;
        let dx = (a.0 - b.0) * 111.2 * ((a.1 + b.1) / 2.0).to_radians().cos();
        (dx * dx + dy * dy).sqrt()
    };
    // grid of 0.2 degrees: neighbours are in the surrounding 3 x 3 cells
    let cell = |v: f64| (v / 0.2).floor() as i32;
    let mut grid: FxHashMap<(i32, i32), Vec<usize>> = FxHashMap::default();
    for (i, p) in samples.iter().enumerate() {
        grid.entry((cell(p.0), cell(p.1))).or_default().push(i);
    }
    let mut parent: Vec<usize> = (0..samples.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..samples.len() {
        let (cx, cy) = (cell(samples[i].0), cell(samples[i].1));
        for gx in cx - 1..=cx + 1 {
            for gy in cy - 1..=cy + 1 {
                for &j in grid.get(&(gx, gy)).into_iter().flatten() {
                    if j > i && km((samples[i].0, samples[i].1), (samples[j].0, samples[j].1)) <= CITY_LINK_KM {
                        let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                        if a != b {
                            parent[a.max(b)] = a.min(b);
                        }
                    }
                }
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for i in 0..samples.len() {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    // a small group near a much bigger one is part of that city (a satellite district or suburb whose track is not linked to the main
    // network): absorb it, biggest groups first, so a big city is one dot. Equal neighbours (two cities) stay apart.
    let km_of = |m: &[usize]| m.iter().map(|&j| samples[j].2).sum::<f64>();
    let mut order: Vec<usize> = groups.keys().copied().collect();
    order.sort_by(|a, b| km_of(&groups[b]).partial_cmp(&km_of(&groups[a])).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(b)));
    let mut owner: Vec<usize> = (0..samples.len()).map(|i| find(&mut parent, i)).collect();
    let mut total: FxHashMap<usize, f64> = order.iter().map(|&g| (g, km_of(&groups[&g]))).collect();
    for &g in &order {
        let mine = total[&g];
        let mut target = None;
        'find: for &i in &groups[&g] {
            let (cx, cy) = (cell(samples[i].0), cell(samples[i].1));
            for gx in cx - 2..=cx + 2 {
                for gy in cy - 2..=cy + 2 {
                    for &j in grid.get(&(gx, gy)).into_iter().flatten() {
                        let o = owner[j];
                        if o != g && total[&o] >= CITY_ABSORB_RATIO * mine && km((samples[i].0, samples[i].1), (samples[j].0, samples[j].1)) <= CITY_ABSORB_KM {
                            target = Some(o);
                            break 'find;
                        }
                    }
                }
            }
        }
        if let Some(o) = target {
            for &i in &groups[&g] {
                owner[i] = o;
            }
            *total.get_mut(&o).unwrap() += mine;
            total.insert(g, 0.0);
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for i in 0..samples.len() {
        groups.entry(owner[i]).or_default().push(i);
    }
    let mut out = String::new();
    for members in groups.values() {
        let by = |k: u8| members.iter().filter(|&&j| samples[j].3 == k).map(|&j| samples[j].2).sum::<f64>();
        let total = by(0) + by(1) + by(2);
        if total < CITY_MIN_KM {
            continue;
        }
        let n = members.len() as f64;
        let mid = (members.iter().map(|&j| samples[j].0).sum::<f64>() / n, members.iter().map(|&j| samples[j].1).sum::<f64>() / n);
        // the dot sits on the sample nearest the middle, so it is never off the network (a city can be long and thin)
        let at = members.iter().copied().min_by(|&a, &b| km((samples[a].0, samples[a].1), mid).partial_cmp(&km((samples[b].0, samples[b].1), mid)).unwrap_or(std::cmp::Ordering::Equal)).unwrap();
        let mut pr = Map::new();
        pr.insert("m".into(), json!(by(0).round() as i64));
        pr.insert("l".into(), json!(by(1).round() as i64));
        pr.insert("r".into(), json!(by(2).round() as i64));
        pr.insert("n".into(), json!(total.round() as i64));
        feat(&mut out, &pr, &format!("{{\"type\":\"Point\",\"coordinates\":[{:.5},{:.5}]}}", samples[at].0, samples[at].1), Some(2), Some(7));
    }
    out
}

/// Writes the tile input (tiles_tmp), then the side files; the caller runs the tile writer afterwards. `stations` and `complexes`
/// are the station model's features, construction stations appended.
pub fn run(slug: &str, inp: &Input, stations: &mut [Value], complexes: &mut [Value], route_order: &Map<String, Value>, data: &Path, out: &Path,
    regions_dirs: &[std::path::PathBuf]) -> Prepared {
    let tmp = data.join("tiles_tmp");
    let side = out.join(slug);
    for d in [&tmp, &side.join("lines"), &side.join("stations"), &side.join("tags"), &side.join("name-hl")] {
        fs::create_dir_all(d).expect("create output folder");
    }
    let lines: Vec<&Value> = inp.lines.iter().collect();
    let line_by: FxHashMap<&str, &Value> = lines.iter().map(|l| (l["id"].as_str().unwrap_or(""), *l)).collect();
    let mut route_line: FxHashMap<&str, &str> = FxHashMap::default();
    for l in &lines {
        for r in l["routes"].as_array().into_iter().flatten() {
            route_line.insert(r.as_str().unwrap_or(""), l["id"].as_str().unwrap_or(""));
        }
    }
    let rels: FxHashMap<&str, &Value> = inp.rels.iter().map(|r| (r["id"].as_str().unwrap_or(""), r)).collect();
    let ls = |l: &str, k: &str| line_by.get(l).and_then(|v| v[k].as_str());

    // ------------------------------------------------------------ operator logos (pipeline/logos): the Wikidata id of a station's operator, when we have its logo
    // logos/logos.json sits in app/public, above the data folder (app/public/data, or app/public/data/world for the world build)
    let logos_json = out.ancestors().map(|a| a.join("logos/logos.json")).find(|p| p.exists());
    let logo_ids: std::collections::HashSet<String> = logos_json.as_ref().and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Map<String, Value>>(&t).ok()).map(|m| m.into_iter().map(|(k, _)| k).collect()).unwrap_or_default();
    if logo_ids.is_empty() {
        eprintln!("warning: no logos/logos.json found above {}; stations get no logo", out.display());
    }
    let china = inp.cfg.get("china").map_or(false, |v| match v { Value::Bool(b) => *b, Value::String(s) => !s.is_empty() && s != "false", Value::Number(n) => n.as_f64() != Some(0.0), _ => false });
    // the station's own operator:wikidata, else the commonest one of the lines serving it, else China Railway for a train station in China
    let logo_for = |p: &Value| -> Option<String> {
        let first = |v: &str| v.split(';').next().unwrap_or("").trim().to_string();
        if let Some(q) = p["operator_wd"].as_str().map(first).filter(|q| logo_ids.contains(q)) {
            return Some(q);
        }
        let mut n: Vec<(String, usize)> = Vec::new();
        for lid in p["line_ids"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if let Some(q) = ls(lid, "operator_wd").map(first).filter(|q| logo_ids.contains(q)) {
                match n.iter_mut().find(|e| e.0 == q) {
                    Some(e) => e.1 += 1,
                    None => n.push((q, 1)),
                }
            }
        }
        if let Some(best) = n.into_iter().max_by_key(|e| e.1) {
            return Some(best.0);
        }
        (china && p["modes"].as_array().map_or(false, |m| m.iter().any(|x| x.as_str() == Some("train"))) && logo_ids.contains("Q1073489"))
            .then(|| "Q1073489".to_string())
    };

    // ------------------------------------------------------------ track + strands
    let (mut track, mut strands, mut areas, mut buildings, mut plats) = (String::new(), String::new(), String::new(), String::new(), String::new());
    let mut overview: Vec<(String, Map<String, Value>, &str, i64, i64)> = Vec::new(); // (sort key, props, geometry, minzoom, maxzoom)
    let mut con_groups: BTreeMap<(String, bool, i64), Vec<Vec<(f64, f64)>>> = BTreeMap::new(); // (type, proposed, speed band or -1) -> ways
    let mut line_ways: FxHashMap<i64, Vec<String>> = FxHashMap::default();
    let mut name_ways: Vec<(String, Vec<String>, String)> = Vec::new(); // (name, ids, type), first-seen order
    let mut name_ix: FxHashMap<String, usize> = FxHashMap::default();
    let mut name_types: Vec<FxHashMap<String, usize>> = Vec::new();   // per name: ways by type, for the colour of its highlight
    let mut way_geom: FxHashMap<String, Vec<(f64, f64)>> = FxHashMap::default();
    let mut way_len: FxHashMap<i64, f64> = FxHashMap::default();
    let mut named: Vec<(String, Option<String>, String, f64, i64, [f64; 4])> = Vec::new(); // (n, ne, t, km, ways, bbox)
    let mut named_ix: FxHashMap<(String, String), usize> = FxHashMap::default();
    let mut railways: Vec<(String, Option<String>, String, f64)> = Vec::new();
    let mut nk_of: FxHashMap<String, usize> = FxHashMap::default();
    let mut pax: Vec<(geomx::G, String)> = Vec::new(); // passenger track for station types
    let mut frt: Vec<(geomx::G, String)> = Vec::new(); // freight and yard track, to tell a freight station from one with no track at all
    let mut con_pax: Vec<(geomx::G, String)> = Vec::new(); // under-construction track (not proposed), to tell a station that is only being built
    let mut city_track: Vec<(f64, f64, f64, u8)> = Vec::new(); // (lon, lat, km, 0 metro / 1 tram / 2 monorail) samples for the city dots
    let (mut n_track, mut n_strands, mut n_areas, mut n_bld, mut n_plats) = (0, 0, 0, 0, 0);
    let tracks_div = |p: &Map<String, Value>| {
        let tr = s(p, "tracks").unwrap_or("");
        match tr.parse::<i64>() {
            Ok(n) if n > 0 && tr.chars().all(|c| c.is_ascii_digit()) => n.min(4) as f64,
            _ => 1.0,
        }
    };
    for fe in &inp.feats {
        let p = &fe.p;
        let t = s(p, "type").filter(|v| !v.is_empty());
        let osm_id = s(p, "osm_id").unwrap_or("");
        if fe.gtype == "LineString" && t.is_some() {
            let t = t.unwrap();
            let c = coords(&fe.g);
            if osm_id.starts_with("way/") {
                way_len.insert(short(osm_id), haversine_len(&c));
            }
            let prop = s(p, "railway") == Some("proposed");
            let con = s(p, "railway") == Some("construction") || prop;
            if !con && s(p, "service").map_or(true, str::is_empty) {
                if let Some(kind) = match (s(p, "railway"), t) { (_, "metro") => Some(0u8), (Some("tram"), _) => Some(1), (_, "monorail") => Some(2), _ => None } {
                    let mut acc = 0.0;
                    for (i, w) in c.windows(2).enumerate() {
                        acc += haversine_len(w);
                        if acc >= 1.0 || i + 2 == c.len() {
                            city_track.push((w[1].0, w[1].1, acc, kind));
                            acc = 0.0;
                        }
                    }
                }
            }
            let mut lids: Vec<&str> = p.get("_routes").and_then(Value::as_array).into_iter().flatten()
                .filter_map(|r| route_line.get(r.as_str().unwrap_or("")).copied()).collect::<BTreeSet<_>>().into_iter().collect();
            lids.sort_by_key(|l| (rank(ls(l, "type").unwrap_or("")), short(l)));
            let mut props = Map::new();
            props.insert("id".into(), json!(osm_id));
            props.insert("type".into(), json!(t));
            for (ki, ko) in [("usage", "usage"), ("electrified", "el"), ("voltage", "v"), ("maxspeed", "ms"), ("gauge", "g"), ("tracks", "tr"), ("name", "n"), ("frequency", "fq")] {
                if let Some(v) = p.get(ki).filter(|v| v.as_str().map_or(false, |x| !x.is_empty())) {
                    props.insert(ko.into(), v.clone());
                }
            }
            if let Some(k) = speed_kmh(s(p, "maxspeed")).filter(|k| *k != 0.0) {
                props.insert("kmh".into(), json!(k.round_ties_even() as i64));
            }
            // track with no name of its own is named after its railway (the route=railway relation it belongs to: LGV Sud-Est), not after its services
            let own_name = s(p, "name").filter(|v| !v.is_empty());
            let name = own_name.or(s(p, "_rwy").filter(|v| !v.is_empty()));
            if own_name.is_none() {
                if let Some(rn) = name {
                    props.insert("n".into(), json!(rn));
                }
            }
            if name.is_some() {
                let en = if own_name.is_some() { english_name(p) } else { s(p, "_rwy_en").map(String::from) };
                if let Some(en) = en.filter(|en| Some(en.as_str()) != name) {
                    props.insert("ne".into(), json!(en));
                }
            }
            if let Some(v) = p.get("service").filter(|v| v.as_str().map_or(false, |x| !x.is_empty())) {
                props.insert("sv".into(), v.clone());
            }
            if s(p, "tunnel").map_or(false, |v| !v.is_empty() && v != "no") {
                props.insert("tun".into(), json!(1));
            }
            if s(p, "bridge").map_or(false, |v| !v.is_empty() && v != "no") {
                props.insert("br".into(), json!(1));
            }
            if con {
                props.insert("con".into(), json!(1));
            }
            if prop {
                props.insert("prop".into(), json!(1));
            }
            if !lids.is_empty() && !con {
                props.insert("ln".into(), json!(lids.iter().map(|l| short(l).to_string()).collect::<Vec<_>>().join(",")));
                props.insert("nl".into(), json!(lids.len()));
                // one colour for the track below z12: first line (most prominent type) that has a colour
                if let Some(c) = lids.iter().find_map(|l| colour(ls(l, "colour"))) {
                    props.insert("oc".into(), json!(c));
                }
            }
            let mz = track_minzoom(t);
            if let Some(n) = name.filter(|_| lids.is_empty() && !con && t != "yard" && t != "freight") {
                let k = (n.to_string(), t.to_string());
                let ix = *named_ix.entry(k).or_insert_with(|| {
                    named.push((n.to_string(), props.get("ne").and_then(Value::as_str).map(String::from), t.to_string(), 0.0, 0, [180.0, 90.0, -180.0, -90.0]));
                    named.len() - 1
                });
                let a = &mut named[ix];
                a.3 += haversine_len(&c) / tracks_div(p);
                a.4 += 1;
                for &(x, y) in &c {
                    a.5 = [a.5[0].min(x), a.5[1].min(y), a.5[2].max(x), a.5[3].max(y)];
                }
            }
            if let Some(n) = props.get("n").and_then(Value::as_str) {
                let nk = *nk_of.entry(n.to_string()).or_insert_with(|| {
                    railways.push((n.to_string(), props.get("ne").and_then(Value::as_str).map(String::from), t.to_string(), 0.0));
                    railways.len() - 1
                });
                let r = &mut railways[nk];
                r.3 += haversine_len(&c) / tracks_div(p);
                if rank(t) < rank(&r.2) {
                    r.2 = t.to_string(); // the most prominent type on the railway
                }
            }
            feat(&mut track, &props, &fe.graw, Some(mz.max(OVERVIEW_MAXZOOM + 1)), None);
            n_track += 1;
            if con && t != "yard" && c.len() >= 2 {
                let band = props.get("kmh").and_then(Value::as_i64).map_or(-1, |k| SPEED_BAND_MINS.iter().find(|&&b| k >= b).copied().unwrap_or(0));
                con_groups.entry((t.to_string(), prop, band)).or_default().push(c.clone());
            }
            way_geom.insert(osm_id.to_string(), c);
            for l in &lids {
                if !con {
                    line_ways.entry(short(l)).or_default().push(osm_id.to_string());
                }
            }
            if let Some(n) = props.get("n").and_then(Value::as_str) {
                let ix = *name_ix.entry(n.to_string()).or_insert_with(|| {
                    name_ways.push((n.to_string(), Vec::new(), t.to_string()));
                    name_types.push(FxHashMap::default());
                    name_ways.len() - 1
                });
                name_ways[ix].1.push(osm_id.to_string());
                *name_types[ix].entry(t.to_string()).or_default() += 1;
            }
            if mz <= OVERVIEW_MAXZOOM {
                let mut ov = Map::new();
                for k in ["type", "con", "prop", "oc", "ln"] {
                    if let Some(v) = props.get(k) {
                        ov.insert(k.into(), v.clone());
                    }
                }
                if let Some(k) = props.get("kmh").and_then(Value::as_i64) {
                    ov.insert("kmh".into(), json!(SPEED_BAND_MINS.iter().find(|&&b| k >= b).copied().unwrap_or(0)));
                }
                // a click opens the first line (ln); every named track also carries its railway number (nk): the hover names the railway, not the services
                if let Some(n) = props.get("n").and_then(Value::as_str) {
                    ov.insert("nk".into(), json!(nk_of[n]));
                }
                // a sketch of the metro network for z4-6, with no attributes but the type, so it merges into a few long lines
                if t == "metro" && !con {
                    let mut sk = Map::new();
                    sk.insert("type".into(), json!("metro"));
                    overview.push((pyjson::sorted_key(&Value::Object(sk.clone())), sk, fe.graw.as_str(), SKETCH_MINZOOM, mz - 1));
                }
                overview.push((pyjson::sorted_key(&Value::Object(ov.clone())), ov, fe.graw.as_str(), mz, OVERVIEW_MAXZOOM));
            }
            if !lids.is_empty() && !con && t != "yard" {
                for (i, l) in lids.iter().enumerate() {
                    let mut sp = Map::new();
                    sp.insert("lid".into(), json!(short(l)));
                    sp.insert("t".into(), json!(ls(l, "type")));
                    sp.insert("i".into(), json!(i));
                    sp.insert("n".into(), json!(lids.len()));
                    sp.insert("lb".into(), json!(label_of(ls(l, "name"), ls(l, "ref"))));
                    if let Some(en) = ls(l, "name_en").filter(|v| !v.is_empty()) {
                        sp.insert("lbe".into(), json!(label_of(Some(en), ls(l, "ref"))));
                    }
                    if let Some(c) = colour(ls(l, "colour")) {
                        if let Some(rf) = badge_of(ls(l, "ref")) {
                            sp.insert("rf".into(), json!(rf));
                            if is_light(&c) {
                                sp.insert("rl".into(), json!(1));
                            }
                        }
                        sp.insert("c".into(), json!(c));
                    }
                    feat(&mut strands, &sp, &fe.graw, Some(11), None);
                    n_strands += 1;
                }
            }
            if !con && matches!(t, "freight" | "yard") {
                if let Some(g) = geomx::from_geojson(&fe.g, geomx::id) {
                    frt.push((g, t.to_string()));
                }
            }
            if rank(t) < 9 && (!con || s(p, "railway") == Some("construction")) {
                if let Some(g) = geomx::from_geojson(&fe.g, geomx::id) {
                    if con { con_pax.push((g, t.to_string())) } else { pax.push((g, t.to_string())) }
                }
            }
        } else if fe.gtype == "Polygon" || fe.gtype == "MultiPolygon" {
            let mut ap = Map::new();
            ap.insert("id".into(), json!(osm_id));
            ap.insert("ha".into(), f(area_ha(&fe.g)));
            if let Some(n) = p.get("name").filter(|v| v.as_str().map_or(false, |x| !x.is_empty())) {
                ap.insert("n".into(), n.clone());
            }
            if let Some(o) = p.get("operator").filter(|v| v.as_str().map_or(false, |x| !x.is_empty())) {
                ap.insert("op".into(), o.clone());
            }
            let with = |mut m: Map<String, Value>, kv: &[(&str, Value)]| {
                for (k, v) in kv {
                    m.insert((*k).into(), v.clone());
                }
                m
            };
            if s(p, "landuse") == Some("railway") {
                feat(&mut areas, &with(ap, &[("k", json!("land"))]), &fe.graw, Some(10), None);
                n_areas += 1;
            } else if s(p, "railway") == Some("yard") {
                feat(&mut areas, &with(ap, &[("k", json!("yard"))]), &fe.graw, Some(10), None);
                n_areas += 1;
            } else if s(p, "building") == Some("train_station") {
                feat(&mut buildings, &with(ap, &[("k", json!("building"))]), &fe.graw, Some(12), None);
                n_bld += 1;
            } else if s(p, "railway") == Some("platform") || s(p, "public_transport") == Some("platform") {
                feat(&mut plats, &with(ap, &[("k", json!("platform")), ("ref", json!(s(p, "ref").unwrap_or("")))]), &fe.graw, Some(13), None);
                n_plats += 1;
            }
        } else if fe.gtype == "LineString" && (s(p, "railway") == Some("platform") || s(p, "public_transport") == Some("platform")) {
            let m: Map<String, Value> = [("id".to_string(), json!(osm_id)), ("ref".to_string(), json!(s(p, "ref").unwrap_or(""))), ("k".to_string(), json!("platform"))].into_iter().collect();
            feat(&mut plats, &m, &fe.graw, Some(13), None);
            n_plats += 1;
        }
    }

    // ------------------------------------------------------------ airports (outline + terminals) and stations that serve them
    let (mut airports, mut airport_pts) = (String::new(), String::new());
    let (mut aero_geoms, mut aero_props): (Vec<geomx::G>, Vec<&Map<String, Value>>) = (vec![], vec![]);
    let (mut n_air, mut n_air_pts) = (0, 0);
    for a in &inp.airports {
        let p = &a.p;
        let lb = s(p, "iata").filter(|v| !v.is_empty()).or(s(p, "name").filter(|v| !v.is_empty())).unwrap_or("");
        let ha = area_ha(&a.g);
        let mut props = Map::new();
        props.insert("id".into(), p.get("id").cloned().unwrap_or(Value::Null));
        props.insert("k".into(), p.get("k").cloned().unwrap_or(Value::Null));
        props.insert("lb".into(), json!(lb));
        props.insert("ha".into(), f(ha));
        for (ki, ko) in [("name", "n"), ("name_en", "ne"), ("iata", "iata"), ("icao", "icao"), ("operator", "op")] {
            if let Some(v) = p.get(ki).filter(|v| v.as_str().map_or(false, |x| !x.is_empty())) {
                props.insert(ko.into(), v.clone());
            }
        }
        if s(p, "k") == Some("aerodrome") {
            let mil = s(p, "military").map_or(false, |v| !v.is_empty()) || s(p, "atype").unwrap_or("").contains("military");
            let iata = s(p, "iata").map_or(false, |v| !v.is_empty());
            let g = geomx::from_geojson(&a.g, geomx::id);
            if iata && !mil && ha >= 300.0 {
                props.insert("com".into(), json!(1));
                // one icon point per commercial airport, so tile boundaries cannot repeat it
                if let Some(rp) = g.as_ref().and_then(geomx::rep_point) {
                    let m: Map<String, Value> = [("id".to_string(), p.get("id").cloned().unwrap_or(Value::Null)), ("lb".to_string(), json!(lb))].into_iter().collect();
                    feat(&mut airport_pts, &m, &pyjson::to_string(&geojson_value(&Geometry::Point(rp)), COMPACT), Some(8), None);
                    n_air_pts += 1;
                }
            }
            feat(&mut airports, &props, &a.graw, Some(if iata { 8 } else { 10 }), None);
            n_air += 1;
            // stations get the airport label only at commercial airports: an IATA code, not military, and 300 ha or more
            if iata && !mil && ha >= 300.0 {
                if let Some(g) = g {
                    aero_geoms.push(geomx::make_valid(g));
                    aero_props.push(p);
                }
            }
        } else {
            feat(&mut airports, &props, &a.graw, Some(12), None);
            n_air += 1;
        }
    }
    let aero_tree = Index::new(aero_geoms.iter().map(geomx::bbox));
    let airport_words = Regex::new(r"(?i)airport|a[ée]roport|flughafen|aeropuerto|机场|航站楼").unwrap();
    const BUFFER_DEG: f64 = 0.003; // about 300 m: stations sit at the terminal edge, often just outside the mapped aerodrome
    let station_pt = |st: &Value| -> Point<f64> {
        let c = &st["geometry"]["coordinates"];
        Point::new(c[0].as_f64().unwrap_or(0.0), c[1].as_f64().unwrap_or(0.0))
    };
    let airport_of = |st: &Value| -> Option<String> {
        let pt = station_pt(st);
        let pg = Geometry::Point(pt);
        for i in aero_tree.near(&pg, BUFFER_DEG) {
            if geomx::dist(&aero_geoms[i], &pg) <= BUFFER_DEG {
                let p = aero_props[i];
                return Some(s(p, "iata").filter(|v| !v.is_empty()).or(s(p, "name").filter(|v| !v.is_empty())).unwrap_or("Airport").to_string());
            }
        }
        let pr = &st["properties"];
        let nm = [pr["name"].as_str(), pr["name_en"].as_str()].into_iter().flatten().filter(|v| !v.is_empty()).collect::<Vec<_>>().join(" ");
        airport_words.is_match(&nm).then(|| "Airport".to_string())
    };

    // ------------------------------------------------------------ stations / complexes
    // rail types serving each station (its lines, and passenger track beside it), so hiding a type can hide its stations
    let pax_tree = Index::new(pax.iter().map(|(g, _)| geomx::bbox(g)));
    let frt_tree = Index::new(frt.iter().map(|(g, _)| geomx::bbox(g)));
    let con_tree = Index::new(con_pax.iter().map(|(g, _)| geomx::bbox(g)));
    const NEAR: f64 = 0.0007; // about 70 m
    let mode_types = |m: &str| -> &'static [&'static str] {
        match m {
            "train" => &["high_speed", "intercity", "regional"],
            "subway" => &["metro"],
            "light_rail" | "tram" => &["light_rail"],
            "monorail" => &["monorail"],
            "funicular" => &["funicular"],
            "other" => &["other"],
            _ => &[],
        }
    };
    let mut cx_types: FxHashMap<String, BTreeSet<String>> = FxHashMap::default();
    let (mut st_rows, mut cx_rows) = (String::new(), String::new());
    let mut airport_cache: Vec<Option<String>> = Vec::with_capacity(stations.len());
    // A station mapped as open but with no service, no open passenger track beside it and construction track beside it is still being
    // built (OSM often tags the station before the line opens): it becomes a construction station here, so the map, the card and the
    // side files agree. A complex whose stations are all being built is one too.
    let mut cx_open: FxHashMap<String, bool> = FxHashMap::default(); // complex -> has a station that is open
    let mut demoted = 0;
    for stv in stations.iter_mut() {
        let p = &stv["properties"];
        let mut open = true;
        if p["construction"].as_bool() != Some(true) && p["line_counts"].as_object().map_or(true, |o| o.is_empty()) {
            let allowed: FxHashSet<&str> = p["modes"].as_array().into_iter().flatten().filter_map(Value::as_str).flat_map(|m| mode_types(m).iter().copied()).collect();
            let pg = Geometry::Point(station_pt(stv));
            let near = |tree: &Index, rows: &[(geomx::G, String)]| -> BTreeSet<String> {
                tree.near(&pg, NEAR).into_iter().filter(|&i| allowed.contains(rows[i].1.as_str()) && geomx::dist(&rows[i].0, &pg) <= NEAR).map(|i| rows[i].1.clone()).collect()
            };
            let con_ty = if near(&pax_tree, &pax).is_empty() { near(&con_tree, &con_pax) } else { BTreeSet::new() };
            if !con_ty.is_empty() {
                let pm = stv["properties"].as_object_mut().unwrap();
                pm.insert("construction".into(), json!(true));
                pm.insert("rail_types".into(), json!(con_ty));
                demoted += 1;
            }
        }
        open &= stv["properties"]["construction"].as_bool() != Some(true);
        *cx_open.entry(stv["properties"]["complex"].as_str().unwrap_or("").to_string()).or_insert(false) |= open;
    }
    for c in complexes.iter_mut() {
        if cx_open.get(c["properties"]["id"].as_str().unwrap_or("")) == Some(&false) {
            c["properties"]["construction"] = json!(true);
        }
    }
    eprintln!("{slug}: {demoted} stations with only construction track beside them shown as under construction");
    for stv in stations.iter() {
        let p = &stv["properties"];
        let ty: BTreeSet<String> = if p["construction"].as_bool() == Some(true) {
            p["rail_types"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect()
        } else {
            let mut ty: BTreeSet<String> = p["line_counts"].as_object().into_iter().flatten().map(|(k, _)| k.clone()).collect();
            let allowed: FxHashSet<&str> = p["modes"].as_array().into_iter().flatten().filter_map(Value::as_str).flat_map(|m| mode_types(m).iter().copied()).collect();
            let pg = Geometry::Point(station_pt(stv));
            for i in pax_tree.near(&pg, NEAR) {
                if allowed.contains(pax[i].1.as_str()) && geomx::dist(&pax[i].0, &pg) <= NEAR {
                    ty.insert(pax[i].1.clone());
                }
            }
            ty
        };
        cx_types.entry(p["complex"].as_str().unwrap_or("").to_string()).or_default().extend(ty.iter().cloned());
        let mut lc: Vec<(&String, &Value)> = p["line_counts"].as_object().into_iter().flatten().collect();
        lc.sort_by_key(|(k, _)| rank(k));
        let lct = lc.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(",");
        let hs_lines = p["line_counts"]["high_speed"].as_i64().unwrap_or(0) > 0;
        let tier = p["tier"].as_i64().unwrap_or(3);
        let ap = airport_of(stv);
        let mut m = Map::new();
        m.insert("id".into(), p["id"].clone());
        m.insert("cx".into(), p["complex"].clone());
        m.insert("n".into(), json!(p["name"].as_str().unwrap_or("")));
        m.insert("ne".into(), json!(p["name_en"].as_str().unwrap_or("")));
        m.insert("tier".into(), json!(tier));
        m.insert("term".into(), json!(p["terminus"].as_bool().unwrap_or(false) as i64));
        m.insert("hsr".into(), json!((p["hsr"].as_bool().unwrap_or(false) || hs_lines) as i64));
        m.insert("mp".into(), p["mainline_platforms"].clone());
        m.insert("pc".into(), p["platform_count"].clone());
        m.insert("lc".into(), p["line_count"].clone());
        m.insert("lct".into(), json!(lct));
        m.insert("ty".into(), json!(if ty.is_empty() { String::new() } else { format!(",{},", ty.iter().cloned().collect::<Vec<_>>().join(",")) }));
        m.insert("modes".into(), json!(p["modes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(",")));
        if let Some(q) = logo_for(p) {
            m.insert("lg".into(), json!(q));
        }
        if let Some(a) = &ap {
            m.insert("ap".into(), json!(a));
        }
        if p["construction"].as_bool() == Some(true) {
            m.insert("con".into(), json!(1));
        } else if ty.is_empty() && lct.is_empty() {
            // no service and no passenger track beside it: with freight or yard track beside it, a freight station (the tooltip says so)
            let pg = Geometry::Point(station_pt(stv));
            if frt_tree.near(&pg, NEAR).into_iter().any(|i| geomx::dist(&frt[i].0, &pg) <= NEAR) {
                m.insert("fr".into(), json!(1));
            }
        }
        airport_cache.push(ap);
        let minz = match tier { 1 => 5, 2 => 7, _ => 9 };
        feat(&mut st_rows, &m, &pyjson::to_string(&stv["geometry"], COMPACT), Some(minz), None);
    }
    for c in complexes.iter() {
        let p = &c["properties"];
        let tier = p["tier"].as_i64().unwrap_or(3);
        let ty = cx_types.get(p["id"].as_str().unwrap_or("")).filter(|t| !t.is_empty());
        let mut m = Map::new();
        m.insert("id".into(), p["id"].clone());
        m.insert("n".into(), json!(p["name"].as_str().unwrap_or("")));
        m.insert("ne".into(), json!(p["name_en"].as_str().unwrap_or("")));
        m.insert("tier".into(), json!(tier));
        m.insert("term".into(), json!(p["terminus"].as_bool().unwrap_or(false) as i64));
        m.insert("ns".into(), p["station_count"].clone());
        if p["construction"].as_bool() == Some(true) {
            m.insert("con".into(), json!(1));
        }
        m.insert("ty".into(), json!(ty.map_or(String::new(), |t| format!(",{},", t.iter().cloned().collect::<Vec<_>>().join(",")))));
        let minz = match tier { 1 => 5, 2 => 8, _ => 10 };
        feat(&mut cx_rows, &m, &pyjson::to_string(&c["geometry"], COMPACT), Some(minz), None);
    }

    // ------------------------------------------------------------ construction dashes (con_dash)
    let mut con_dash = String::new();
    for ((t, prop, band), ways) in con_groups {
        let mut props = Map::new();
        props.insert("type".into(), json!(t));
        if prop {
            props.insert("prop".into(), json!(1));
        }
        if band >= 0 {
            props.insert("kmh".into(), json!(band));
        }
        let merged = linemerge(ways);
        for line in dedupe_parallel(merged.iter().map(|l| l.iter().map(|&p| to_merc(p)).collect()).collect()) {
            con_dashes(&mut con_dash, &props, &line, track_minzoom(&t).max(CON_DASH_MINZOOM));
        }
    }

    // ------------------------------------------------------------ city dots: where metro and tram systems are, visible from far out
    let cities_text = city_dots(&city_track);

    // ------------------------------------------------------------ tile input: every layer, the overview first
    overview.sort_by(|a, b| a.0.cmp(&b.0));
    let mut ov_text = String::new();
    for (_, props, geom, mz, xz) in &overview {
        feat(&mut ov_text, props, geom, Some(*mz), Some(*xz));
    }
    let texts: [&String; 12] = [&ov_text, &track, &con_dash, &strands, &areas, &buildings, &plats, &st_rows, &cx_rows, &airports, &airport_pts, &cities_text];
    for ((_, file), text) in TILE_LAYERS.iter().zip(texts) {
        let file = tmp.join(file);
        fs::File::create(&file).and_then(|mut w| w.write_all(text.as_bytes())).unwrap_or_else(|e| panic!("write {}: {e}", file.display()));
    }

    // ------------------------------------------------------------ side files
    let mut idx = Map::new();
    for l in &lines {
        let mut km = 0.0f64;
        for rid in l["routes"].as_array().into_iter().flatten() {
            if let Some(r) = rels.get(rid.as_str().unwrap_or("")) {
                let wl: f64 = r["members"].as_array().into_iter().flatten()
                    .filter(|m| m["type"] == "way" && matches!(m["role"].as_str(), Some("" | "route")))
                    .map(|m| way_len.get(&m["id"].as_i64().unwrap_or(0)).copied().unwrap_or(0.0)).sum();
                km = km.max(wl);
            }
        }
        let id = l["id"].as_str().unwrap_or("");
        idx.insert(short(id).to_string(), json!({"n": l["name"], "r": l["ref"], "c": colour(l["colour"].as_str()), "t": l["type"], "o": l["operator"],
            "f": l["from"], "to": l["to"], "km": f(py_round(km, 1)), "ne": l.get("name_en").cloned().unwrap_or(Value::Null)}));
    }
    write_json(&side.join("lines-index.json"), &Value::Object(idx));
    let mut nl: Vec<&(String, Option<String>, String, f64, i64, [f64; 4])> = named.iter().filter(|v| v.3 >= 1.5).collect();
    // by length rounded to 0.1 km, longest first (build_tiles.py sorted after rounding: equal lengths keep first-seen order)
    nl.sort_by(|a, b| py_round(b.3, 1).partial_cmp(&py_round(a.3, 1)).unwrap_or(std::cmp::Ordering::Equal));
    let n_named = nl.len();
    write_json(&side.join("named-lines.json"), &Value::Array(nl.iter().map(|v| json!({"n": v.0, "ne": v.1, "t": v.2, "km": f(py_round(v.3, 1)), "ways": v.4,
        "bbox": v.5.iter().map(|x| f(py_round(*x, 4))).collect::<Vec<_>>()})).collect()));
    // railways.json: [name, English name, type, km] by railway number (nk)
    write_json(&side.join("railways.json"), &Value::Array(railways.iter().map(|r| json!([r.0, r.1, r.2, f(py_round(r.3, 1))])).collect()));

    // raw (filtered) OSM tags plus Rail Globe derived fields, sharded by id for the app's Inspect mode
    let mut props_by_id: FxHashMap<&str, &Map<String, Value>> = FxHashMap::default();
    let mut shards: Vec<Map<String, Value>> = vec![Map::new(); 256];
    for fe in &inp.feats {
        let oid = s(&fe.p, "osm_id").unwrap_or("");
        props_by_id.insert(oid, &fe.p);
        let without: Map<String, Value> = fe.p.iter().filter(|(k, _)| k.as_str() != "osm_id").map(|(k, v)| (k.clone(), v.clone())).collect();
        shards[(short(oid).rem_euclid(256)) as usize].insert(oid.to_string(), Value::Object(without));
    }
    for (i, sh) in shards.into_iter().enumerate() {
        write_json(&side.join("tags").join(format!("{i}.json")), &Value::Object(sh));
    }

    let st_by_id: FxHashMap<&str, &Value> = stations.iter().map(|x| (x["properties"]["id"].as_str().unwrap_or(""), &x["properties"])).collect();
    let cx_name_of: FxHashMap<&str, &Value> = complexes.iter().map(|c| (c["properties"]["id"].as_str().unwrap_or(""), &c["properties"]["name"])).collect();
    let mut cx_members: FxHashMap<&str, Vec<&Value>> = FxHashMap::default();
    for x in stations.iter() {
        cx_members.entry(x["properties"]["complex"].as_str().unwrap_or("")).or_default().push(&x["properties"]);
    }
    let model_keys = ["score", "tier", "major", "terminus", "dead_ends", "mainline_platforms", "platform_count", "mainline_track_ways", "land_ha", "building_ha", "grouping", "hsr"];
    for (k, stv) in stations.iter().enumerate() {
        let p = &stv["properties"];
        let mut groups: Vec<(String, Vec<i64>)> = Vec::new();
        for lid in p["line_ids"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if let Some(t) = ls(lid, "type") {
                match groups.iter_mut().find(|g| g.0 == t) {
                    Some(g) => g.1.push(short(lid)),
                    None => groups.push((t.to_string(), vec![short(lid)])),
                }
            }
        }
        groups.sort_by_key(|g| rank(&g.0));
        let others: Vec<Value> = cx_members.get(p["complex"].as_str().unwrap_or("")).into_iter().flatten().filter(|o| o["id"] != p["id"])
            .map(|o| json!({"id": o["id"], "name": o["name"], "name_en": o["name_en"], "modes": o["modes"]})).collect();
        let sources: Vec<&str> = p["source_ids"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let mut tags: Vec<Value> = sources.iter().filter_map(|i| props_by_id.get(i).map(|pr| {
            let t: Map<String, Value> = pr.iter().filter(|(k, _)| k.as_str() != "osm_id").map(|(k, v)| (k.clone(), v.clone())).collect();
            json!({"id": i, "tags": t})
        })).collect();
        tags.extend(sources.iter().filter_map(|i| rels.get(i).map(|r| json!({"id": i, "tags": r["tags"]}))));
        let model: Map<String, Value> = model_keys.iter().map(|k| (k.to_string(), p.get(*k).cloned().unwrap_or(Value::Null))).collect();
        let rec = json!({
            "id": p["id"], "name": p["name"], "name_en": p["name_en"], "tier": p["tier"], "terminus": p["terminus"],
            "hsr": p["hsr"].as_bool().unwrap_or(false) || p["line_counts"]["high_speed"].as_i64().unwrap_or(0) > 0, "modes": p["modes"],
            "platforms": p["mainline_platforms"], "platform_count": p["platform_count"],
            "platform_refs": p["platform_refs"], "platform_refs_other": p["platform_refs_other"],
            "rail_lines": p.get("rail_lines").cloned().unwrap_or(json!([])),
            "line_count": p["line_count"], "lines": Value::Object(groups.into_iter().map(|(t, mut v)| { v.sort(); (t, json!(v)) }).collect()),
            "complex": if others.is_empty() { Value::Null } else { json!({"id": p["complex"], "name": cx_name_of.get(p["complex"].as_str().unwrap_or("")).copied().cloned().unwrap_or(Value::Null), "others": others}) },
            "construction": p["construction"].as_bool().unwrap_or(false), "airport": airport_cache[k],
            "osm": sources.first().copied(), "wikidata": p["wikidata"], "pt": stv["geometry"]["coordinates"], "logo": logo_for(p),
            "tags": tags, "model": Value::Object(model),
        });
        write_json(&side.join("stations").join(format!("{}.json", safe(p["id"].as_str().unwrap_or("")))), &rec);
    }

    for l in &lines {
        let lid = l["id"].as_str().unwrap_or("");
        let mut routes: Vec<Value> = Vec::new();
        for rid in l["routes"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            let Some(r) = rels.get(rid) else { continue };
            let wl: f64 = r["members"].as_array().into_iter().flatten()
                .filter(|m| m["type"] == "way" && matches!(m["role"].as_str(), Some("" | "route")))
                .map(|m| way_len.get(&m["id"].as_i64().unwrap_or(0)).copied().unwrap_or(0.0)).sum();
            let order = route_order.get(rid).cloned().unwrap_or(json!([]));
            routes.push(json!({"id": rid, "name": r["tags"]["name"], "from": r["tags"]["from"], "to": r["tags"]["to"], "km": f(py_round(wl, 1)), "stations": order}));
        }
        routes.sort_by(|a, b| {
            let k = |r: &Value| (r["stations"].as_array().map_or(0, |v| v.len()), r["km"].as_f64().unwrap_or(0.0));
            let (ka, kb) = (k(a), k(b));
            kb.0.cmp(&ka.0).then(kb.1.partial_cmp(&ka.1).unwrap_or(std::cmp::Ordering::Equal))
        });
        let used: BTreeSet<&str> = routes.iter().take(8).flat_map(|r| r["stations"].as_array().into_iter().flatten().filter_map(Value::as_str)).collect();
        let names: Map<String, Value> = used.iter().filter_map(|sid| st_by_id.get(sid).map(|p| (sid.to_string(), json!({"n": p["name"], "ne": p["name_en"]})))).collect();
        let km = routes.iter().filter_map(|r| r["km"].as_f64()).fold(None, |a: Option<f64>, x| Some(a.map_or(x, |a| a.max(x))));
        let c = colour(l["colour"].as_str());
        let mut extra = vec![("type", l["type"].clone())];
        if let Some(c) = &c {
            extra.push(("oc", json!(c)));
        }
        let rec = json!({
            "id": short(lid), "name": l["name"], "ref": l["ref"], "type": l["type"], "colour": c,
            "operator": l["operator"], "network": l["network"], "from": l["from"], "to": l["to"],
            "km": km.map_or(json!(0), f), "osm": lid,
            "routes": routes.iter().take(8).collect::<Vec<_>>(), "station_names": names, "tags": l.get("tags").filter(|t| !t.is_null()).cloned().unwrap_or(json!({})),
            "name_en": l.get("name_en").cloned().unwrap_or(Value::Null),
            "route_ids": l["routes"].as_array().map(|v| v.iter().take(30).cloned().collect::<Vec<_>>()).unwrap_or_default(),
            "hl": hl_record(line_ways.get(&short(lid)).map(Vec::as_slice).unwrap_or(&[]), &way_geom, extra),
        });
        write_json(&side.join("lines").join(format!("{}.json", short(lid))), &rec);
    }

    // highlight data for named railways, sharded by a hash of the name (any track name: a hovered piece with no mapped line)
    let mut hl_shards: Vec<Map<String, Value>> = vec![Map::new(); NAME_SHARDS as usize];
    for (ix, (n, ids, first)) in name_ways.iter().enumerate() {
        // the type most of its ways have, not the first way's: a railway often starts with a yard or siding piece (Korea's Gyeonggang Line did)
        let mut by: Vec<(&String, &usize)> = name_types[ix].iter().filter(|(k, _)| k.as_str() != "yard").collect();
        by.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        let t = by.first().map(|e| e.0).unwrap_or(first);
        hl_shards[(fnv1a(n) % NAME_SHARDS) as usize].insert(n.clone(), hl_record(ids, &way_geom, vec![("type", json!(t))]));
    }
    for (i, sh) in hl_shards.into_iter().enumerate() {
        write_json(&side.join("name-hl").join(format!("{i}.json")), &Value::Object(sh));
    }

    // boundary (simplified) for the faint region outline: hand-made outline first, else one written by `rail-extract outlines`
    if let Some(path) = regions_dirs.iter().map(|d| d.join(format!("{slug}.geojson"))).find(|p| p.exists()) {
        let b: Value = serde_json::from_str(&fs::read_to_string(&path).expect("read outline")).expect("parse outline");
        if let Some(g) = geomx::from_geojson(&b["geometry"], geomx::id) {
            let g = match g {
                Geometry::Polygon(p) => Geometry::Polygon(p.simplify(0.0008)),
                Geometry::MultiPolygon(m) => Geometry::MultiPolygon(m.simplify(0.0008)),
                g => g,
            };
            let v = json!({"type": "Feature", "properties": {"slug": slug}, "geometry": geojson_value(&g)});
            fs::write(out.join(format!("{slug}.boundary.geojson")), pyjson::to_string(&v, pyjson::Style { ascii: true, compact: true })).expect("write boundary");
        }
    }
    Prepared {
        summary: format!(
            "{slug}: track={n_track}, lines={n_strands}, areas={n_areas}, buildings={n_bld}, platforms={n_plats}, stations={}, complexes={}, airports={n_air}, airport_pts={n_air_pts}; track overview z0-{OVERVIEW_MAXZOOM}: {} pieces; side files {} stations, {} lines, {n_named} named railway lines",
            stations.len(), complexes.len(), overview.len(), stations.len(), lines.len()
        ),
    }
}
