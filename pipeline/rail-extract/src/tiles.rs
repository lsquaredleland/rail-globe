//! Tile input and side files, ported from pipeline/build_tiles.py: the GeoJSON layers tippecanoe turns into the region's PMTiles
//! (run here, in the background while the side files are written) and the JSON side files the app loads on demand.
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
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Child, Command};

const TYPE_ORDER: [&str; 8] = ["high_speed", "intercity", "regional", "metro", "light_rail", "monorail", "funicular", "other"];
// Zoomed out (tile zooms 0..OVERVIEW_MAXZOOM) the track layer holds a merged overview instead of every piece (docs/00-overview.md)
const OVERVIEW_MAXZOOM: i64 = 7;
const SPEED_BAND_MINS: [i64; 6] = [250, 200, 160, 120, 80, 0]; // the speed colour bands of app/src/mapStyle.ts
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

fn short(rid: &str) -> i64 {
    rid.split_once('/').and_then(|(_, n)| n.parse().ok()).unwrap_or(0)
}

fn coords(g: &Value) -> Vec<(f64, f64)> {
    g["coordinates"].as_array().into_iter().flatten().filter_map(|c| Some((c[0].as_f64()?, c[1].as_f64()?))).collect()
}

fn haversine_len(c: &[(f64, f64)]) -> f64 {
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

/// one tippecanoe input line: {"type":"Feature","properties":...,"geometry":...,"tippecanoe":{...}}
fn feat(out: &mut String, props: &Map<String, Value>, geom: &str, minzoom: Option<i64>, maxzoom: Option<i64>) {
    out.push_str("{\"type\":\"Feature\",\"properties\":");
    pyjson::write_map(out, props, COMPACT);
    out.push_str(",\"geometry\":");
    out.push_str(geom);
    if minzoom.is_some() || maxzoom.is_some() {
        out.push_str(",\"tippecanoe\":{");
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

pub struct Prepared {
    pub tippecanoe: Option<Child>,
    pub pmtiles: std::path::PathBuf,
    pub summary: String,
}

/// Writes tippecanoe's input and starts it (unless `tiles` is false), then writes the side files. `stations` and `complexes`
/// are the station model's features, construction stations appended.
pub fn run(slug: &str, inp: &Input, stations: &[Value], complexes: &[Value], route_order: &Map<String, Value>, data: &Path, out: &Path,
    regions_dirs: &[std::path::PathBuf], tiles: bool) -> Prepared {
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

    // ------------------------------------------------------------ track + strands
    let (mut track, mut strands, mut areas, mut buildings, mut plats) = (String::new(), String::new(), String::new(), String::new(), String::new());
    let mut overview: Vec<(String, Map<String, Value>, &str, i64)> = Vec::new(); // (sort key, props, geometry, minzoom)
    let mut line_ways: FxHashMap<i64, Vec<String>> = FxHashMap::default();
    let mut name_ways: Vec<(String, Vec<String>, String)> = Vec::new(); // (name, ids, type), first-seen order
    let mut name_ix: FxHashMap<String, usize> = FxHashMap::default();
    let mut way_geom: FxHashMap<String, Vec<(f64, f64)>> = FxHashMap::default();
    let mut way_len: FxHashMap<i64, f64> = FxHashMap::default();
    let mut named: Vec<(String, Option<String>, String, f64, i64, [f64; 4])> = Vec::new(); // (n, ne, t, km, ways, bbox)
    let mut named_ix: FxHashMap<(String, String), usize> = FxHashMap::default();
    let mut railways: Vec<(String, Option<String>, String, f64)> = Vec::new();
    let mut nk_of: FxHashMap<String, usize> = FxHashMap::default();
    let mut pax: Vec<(geomx::G, String)> = Vec::new(); // passenger track for station types
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
            let name = s(p, "name").filter(|v| !v.is_empty());
            if name.is_some() {
                if let Some(en) = english_name(p).filter(|en| Some(en.as_str()) != name) {
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
            way_geom.insert(osm_id.to_string(), c);
            for l in &lids {
                if !con {
                    line_ways.entry(short(l)).or_default().push(osm_id.to_string());
                }
            }
            if let Some(n) = props.get("n").and_then(Value::as_str) {
                let ix = *name_ix.entry(n.to_string()).or_insert_with(|| {
                    name_ways.push((n.to_string(), Vec::new(), t.to_string()));
                    name_ways.len() - 1
                });
                name_ways[ix].1.push(osm_id.to_string());
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
                // a click opens the first line (ln); track with no mapped line is named through its railway number
                if !props.contains_key("ln") {
                    if let Some(n) = props.get("n").and_then(Value::as_str) {
                        ov.insert("nk".into(), json!(nk_of[n]));
                    }
                }
                overview.push((pyjson::sorted_key(&Value::Object(ov.clone())), ov, fe.graw.as_str(), mz));
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
                        sp.insert("c".into(), json!(c));
                    }
                    feat(&mut strands, &sp, &fe.graw, Some(11), None);
                    n_strands += 1;
                }
            }
            if rank(t) < 9 && !con {
                if let Some(g) = geomx::from_geojson(&fe.g, geomx::id) {
                    pax.push((g, t.to_string()));
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
    for stv in stations {
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
        if let Some(a) = &ap {
            m.insert("ap".into(), json!(a));
        }
        if p["construction"].as_bool() == Some(true) {
            m.insert("con".into(), json!(1));
        }
        airport_cache.push(ap);
        let minz = match tier { 1 => 5, 2 => 7, _ => 9 };
        feat(&mut st_rows, &m, &pyjson::to_string(&stv["geometry"], COMPACT), Some(minz), None);
    }
    for c in complexes {
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

    // ------------------------------------------------------------ tippecanoe: every layer in one run, the overview coalesced
    overview.sort_by(|a, b| a.0.cmp(&b.0));
    let mut ov_text = String::new();
    for (_, props, geom, mz) in &overview {
        feat(&mut ov_text, props, geom, Some(*mz), Some(OVERVIEW_MAXZOOM));
    }
    let layers: [(&str, &String); 10] = [("track", &ov_text), ("track", &track), ("lines", &strands), ("areas", &areas), ("buildings", &buildings),
        ("platforms", &plats), ("stations", &st_rows), ("complexes", &cx_rows), ("airports", &airports), ("airport_pts", &airport_pts)];
    let mut args: Vec<String> = Vec::new();
    for (k, (name, text)) in layers.iter().enumerate() {
        let file = tmp.join(if k == 0 { "track_overview.geojsonl".to_string() } else { format!("{name}.geojsonl") });
        fs::File::create(&file).and_then(|mut w| w.write_all(text.as_bytes())).unwrap_or_else(|e| panic!("write {}: {e}", file.display()));
        args.push("-L".into());
        args.push(format!("{name}:{}", file.display()));
    }
    let pm = out.join(format!("{slug}.pmtiles"));
    // --coalesce merges consecutive features with equal attributes (only overview pieces and identical neighbouring strands
    // have them); --preserve-input-order keeps every layer in the order written here
    let child = if tiles {
        Some(Command::new("tippecanoe")
            .args(["-o", &pm.to_string_lossy(), "-f", "-Z0", "-z14", "--no-tile-size-limit", "--no-feature-limit", "--drop-rate=0", "--simplification=2",
                "--coalesce", "--preserve-input-order", "--read-parallel", "-q"])
            .args(&args)
            .spawn()
            .expect("start tippecanoe (is it installed?)"))
    } else {
        None
    };

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
    for x in stations {
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
            "osm": sources.first().copied(), "wikidata": p["wikidata"], "pt": stv["geometry"]["coordinates"],
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
    for (n, ids, t) in &name_ways {
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
        tippecanoe: child,
        pmtiles: pm,
        summary: format!(
            "{slug}: track={n_track}, lines={n_strands}, areas={n_areas}, buildings={n_bld}, platforms={n_plats}, stations={}, complexes={}, airports={n_air}, airport_pts={n_air_pts}; track overview z0-{OVERVIEW_MAXZOOM}: {} pieces; side files {} stations, {} lines, {n_named} named railway lines",
            stations.len(), complexes.len(), overview.len(), stations.len(), lines.len()
        ),
    }
}
