//! Region statistics for the app's info card: how much rail a region has, and each area inside it, written to
//! <out>/<slug>/stats.json (numbers) and shapes.json (simplified outlines, for finding the area under the view).
//!
//! Areas: the region itself (everything in its cut), the areas its config lists under `stat_areas`, and, with `stat_auto`
//! ({"country": "CN", "levels": ["4", "5"], "min_metro": 20, "exclude": [slugs]}), every outline of those admin levels in data/regions/index.json
//! (level 4 = provinces, 5 = prefecture-level cities, and index entries with `city: true`, any level; cities with fewer than `min_metro` metro stations are left out).
//!
//! Length is route km, with double track counted once. OSM maps most double track as one way per track, so summing ways
//! doubles it. Instead the map is cut into square cells of CELL_M metres; per rail type, each cell any track touches counts once,
//! as the average length of a line crossing a square at that angle (side / (|cos| + |sin|)). Tracks closer than a cell
//! are one track; separate corridors more than a cell apart are separate. Accurate to roughly 10%.

use crate::classify::speed_kmh;
use crate::geomx::{self, Index};
use crate::site::{s, Input};
use geo::{MultiPolygon, Point, Simplify};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// rail types counted, in the order the card lists them
const TYPES: [&str; 9] = ["high_speed", "intercity", "regional", "metro", "light_rail", "monorail", "funicular", "other", "freight"];
/// lower bounds of the speed colour bands of app/src/mapStyle.ts (same list as tiles.rs)
const SPEED_BAND_MINS: [i64; 9] = [400, 350, 300, 250, 200, 160, 120, 80, 0];
const CELL_M: f64 = 60.0;
const M_PER_DEG: f64 = 111_320.0;
const SHAPE_EPS: f64 = 0.02; // degrees, about 2 km: enough to tell which area the view centre is in

/// point-in-polygon with the edges bucketed by latitude band, so a test looks at a few edges, not a whole province outline
struct Pip {
    y0: f64,
    inv: f64,
    bands: Vec<Vec<[f64; 4]>>,
}

impl Pip {
    fn new(mp: &MultiPolygon<f64>, bbox: [f64; 4]) -> Pip {
        let n: usize = mp.0.iter().map(|p| p.exterior().0.len() + p.interiors().iter().map(|r| r.0.len()).sum::<usize>()).sum();
        let nb = (n / 6).clamp(1, 4096);
        let (y0, h) = (bbox[1], (bbox[3] - bbox[1]).max(1e-9));
        let inv = nb as f64 / h;
        let mut bands: Vec<Vec<[f64; 4]>> = vec![Vec::new(); nb];
        for p in &mp.0 {
            for ring in std::iter::once(p.exterior()).chain(p.interiors().iter()) {
                for w in ring.0.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    if a.y == b.y {
                        continue;
                    }
                    let (lo, hi) = (a.y.min(b.y), a.y.max(b.y));
                    let (i0, i1) = ((((lo - y0) * inv) as usize).min(nb - 1), (((hi - y0) * inv) as usize).min(nb - 1));
                    for band in &mut bands[i0..=i1] {
                        band.push([a.x, a.y, b.x, b.y]);
                    }
                }
            }
        }
        Pip { y0, inv, bands }
    }
    fn contains(&self, x: f64, y: f64) -> bool {
        let i = ((y - self.y0) * self.inv).max(0.0) as usize;
        let Some(band) = self.bands.get(i) else { return false };
        let mut inside = false;
        for e in band {
            if (e[1] > y) != (e[3] > y) && x < e[0] + (y - e[1]) * (e[2] - e[0]) / (e[3] - e[1]) {
                inside = !inside;
            }
        }
        inside
    }
}

struct Area {
    slug: String,
    name: String,
    local: Option<String>,
    kind: &'static str,
    poly: Option<MultiPolygon<f64>>, // None: the whole region
    pip: Option<Pip>,
    bbox: [f64; 4], // w, s, e, n
    min_metro: i64,
    km: BTreeMap<(String, bool), f64>,
    /// open passenger route km by speed band (lower bound of the app's speed bands, -1: no usable maxspeed)
    speed: BTreeMap<i64, f64>,
    /// the same for passenger track under construction (not in the open totals)
    speed_con: BTreeMap<i64, f64>,
    /// the same for proposed passenger track
    speed_prop: BTreeMap<i64, f64>,
    /// proposed route km by type
    km_prop: BTreeMap<String, f64>,
    lines: BTreeMap<String, BTreeSet<String>>,
    st: BTreeMap<&'static str, i64>,
}

fn outline(slug: &str, dirs: &[PathBuf]) -> Option<(MultiPolygon<f64>, [f64; 4])> {
    let text = dirs.iter().find_map(|d| fs::read_to_string(d.join(format!("{slug}.geojson"))).ok())?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let g = if v["type"] == "Feature" { v["geometry"].clone() } else if v["type"] == "FeatureCollection" { v["features"][0]["geometry"].clone() } else { v };
    let mp = geomx::polys(&geomx::from_geojson(&g, geomx::id)?);
    let r = geomx::bbox(&geo::Geometry::MultiPolygon(mp.clone()))?;
    Some((mp, [r.min().x, r.min().y, r.max().x, r.max().y]))
}

fn tracks_of(fe: &crate::site::Feat) -> Vec<(f64, f64)> {
    fe.g["coordinates"].as_array().into_iter().flatten().filter_map(|c| Some((c[0].as_f64()?, c[1].as_f64()?))).collect()
}

/// Route km per area (index into `areas`) for the ways of one rail type: each cell touched is credited once, to the areas
/// holding the first point that touched it.
fn group_km(ways: &[&Vec<(f64, f64)>], n_areas: usize, tree: &Index, pips: &[Option<&Pip>]) -> Vec<f64> {
    let mut km = vec![0.0; n_areas];
    let mut seen: FxHashSet<u64> = FxHashSet::default();
    let step = CELL_M / 3.0;
    for c in ways {
        for w in c.windows(2) {
            let (a, b) = (w[0], w[1]);
            let mid_lat = ((a.1 + b.1) / 2.0).to_radians();
            let (dx, dy) = ((b.0 - a.0) * M_PER_DEG * mid_lat.cos(), (b.1 - a.1) * M_PER_DEG);
            let len = dx.hypot(dy);
            if len < 1e-6 {
                continue;
            }
            // a line at this angle crosses a cell for side / (|cos| + |sin|) on average
            let credit = CELL_M / ((dx.abs() + dy.abs()) / len) / 1000.0;
            let n = (len / step).ceil() as usize;
            for i in 0..=n {
                let t = (i as f64 * step / len).min(1.0);
                let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                let cos = ((y * 2.0).round() / 2.0).to_radians().cos();
                let (cx, cy) = ((x * M_PER_DEG * cos / CELL_M).floor() as i64, (y * M_PER_DEG / CELL_M).floor() as i64);
                let key = (((cx + (1 << 30)) as u64) << 32) | ((cy + (1 << 30)) as u64 & 0xffff_ffff);
                if !seen.insert(key) {
                    continue;
                }
                km[0] += credit;
                for i in tree.at(&Point::new(x, y)) {
                    if pips[i].map_or(false, |p| p.contains(x, y)) {
                        km[i] += credit;
                    }
                }
            }
        }
    }
    km
}

fn r1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn shape(mp: &MultiPolygon<f64>) -> Value {
    let simple = mp.simplify(SHAPE_EPS);
    let r3 = |v: f64| (v * 1000.0).round() / 1000.0;
    Value::Array(
        simple.0.iter().filter(|p| p.exterior().0.len() >= 4).map(|p| {
            Value::Array(std::iter::once(p.exterior()).chain(p.interiors().iter()).map(|r| {
                Value::Array(r.0.iter().map(|c| json!([r3(c.x), r3(c.y)])).collect())
            }).collect())
        }).collect(),
    )
}

pub fn run(slug: &str, inp: &Input, stations: &[Value], dirs: &[PathBuf], out: &Path) -> String {
    let new = |slug: &str, name: String, local: Option<String>, kind: &'static str, poly: Option<MultiPolygon<f64>>, bbox: [f64; 4], min_metro: i64| Area {
        slug: slug.to_string(), name, local, kind, pip: poly.as_ref().map(|p| Pip::new(p, bbox)), poly, bbox, min_metro,
        km: BTreeMap::new(), speed: BTreeMap::new(), speed_con: BTreeMap::new(), speed_prop: BTreeMap::new(), km_prop: BTreeMap::new(), lines: BTreeMap::new(), st: BTreeMap::new(),
    };
    let mut areas: Vec<Area> = Vec::new();
    let own = outline(slug, dirs);
    areas.push(new(slug, inp.cfg.get("stat_name").and_then(Value::as_str).unwrap_or(slug).to_string(),
        inp.cfg.get("stat_local").and_then(Value::as_str).map(String::from), "region", None, own.map_or([-180.0, -90.0, 180.0, 90.0], |o| o.1), 0));
    for a in inp.cfg.get("stat_areas").and_then(Value::as_array).into_iter().flatten() {
        let aslug = a["slug"].as_str().unwrap_or("");
        let Some((poly, bbox)) = outline(aslug, dirs) else { panic!("stats: no outline for {aslug} (pipeline/regions or data/regions)") };
        areas.push(new(aslug, a["name"].as_str().unwrap_or(aslug).to_string(), a["local"].as_str().map(String::from), "area", Some(poly), bbox, 0));
    }
    if let Some(auto) = inp.cfg.get("stat_auto") {
        let country = auto["country"].as_str().unwrap_or("");
        let levels: Vec<&str> = auto["levels"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let min_metro = auto["min_metro"].as_i64().unwrap_or(0);
        let exclude: BTreeSet<&str> = auto["exclude"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let index: Map<String, Value> = dirs.iter().find_map(|d| fs::read_to_string(d.join("index.json")).ok()).and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| panic!("stats: stat_auto needs data/regions/index.json (rail-extract outlines --level 4 --country {country})"));
        let have: BTreeSet<String> = areas.iter().map(|a| a.slug.clone()).collect();
        let mut picked: Vec<(&String, &Value)> = index.iter().filter(|(k, v)| v["country"] == country && (levels.contains(&v["admin_level"].as_str().unwrap_or("")) || v["city"] == true) && !have.contains(*k) && !exclude.contains(k.as_str())).collect();
        picked.sort_by_key(|(k, _)| k.to_string());
        for (k, v) in picked {
            let Some((poly, bbox)) = outline(k, dirs) else { continue };
            let lvl5 = v["admin_level"] == "5" || v["city"] == true;   // `city: true`: an outline fetched by pipeline/fetch_city_outlines.py (any admin level)
            let en = v["name_en"].as_str().or(v["name"].as_str()).unwrap_or(k).trim_end_matches(" City").trim_end_matches(" Prefecture").to_string();
            areas.push(new(k, en.clone(), v["name"].as_str().filter(|n| *n != en).map(String::from), if lvl5 { "city" } else { "province" }, Some(poly), bbox, if lvl5 { min_metro } else { 0 }));
        }
    }

    let tree = Index::new(areas.iter().enumerate().map(|(i, a)| (i > 0).then(|| geo::Rect::new((a.bbox[0], a.bbox[1]), (a.bbox[2], a.bbox[3])))));
    let pip_own: Vec<Option<Pip>> = areas.iter_mut().map(|a| a.pip.take()).collect(); // apart from `areas`, which is filled in below
    let pips: Vec<Option<&Pip>> = pip_own.iter().map(|p| p.as_ref()).collect();
    let n_areas = areas.len();

    // line id for each route, and each line's type
    let mut route_line: FxHashMap<&str, &str> = FxHashMap::default();
    let mut line_type: FxHashMap<&str, &str> = FxHashMap::default();
    for l in &inp.lines {
        let id = l["id"].as_str().unwrap_or("");
        line_type.insert(id, l["type"].as_str().unwrap_or(""));
        for r in l["routes"].as_array().into_iter().flatten() {
            route_line.insert(r.as_str().unwrap_or(""), id);
        }
    }

    // ways by (type, under construction); lines per area from each way's middle vertex
    let mut groups: BTreeMap<(String, bool), Vec<Vec<(f64, f64)>>> = BTreeMap::new();
    let mut prop_groups: BTreeMap<String, Vec<Vec<(f64, f64)>>> = BTreeMap::new();
    let mut speed_groups: BTreeMap<(i64, u8), Vec<Vec<(f64, f64)>>> = BTreeMap::new();
    let mut line_hits: Vec<(Vec<usize>, Vec<&str>)> = Vec::new();
    for fe in &inp.feats {
        let p = &fe.p;
        let Some(t) = s(p, "type").filter(|t| TYPES.contains(t)) else { continue };
        if fe.gtype != "LineString" {
            continue;
        }
        let con = s(p, "railway") == Some("construction");
        let prop = s(p, "railway") == Some("proposed");
        let c = tracks_of(fe);
        if c.len() < 2 {
            continue;
        }
        if !con && !prop {
            let lids: Vec<&str> = p.get("_routes").and_then(Value::as_array).into_iter().flatten().filter_map(|r| route_line.get(r.as_str().unwrap_or("")).copied()).collect();
            if !lids.is_empty() {
                let m = c[c.len() / 2];
                let mut in_areas = vec![0usize];
                in_areas.extend(tree.at(&Point::new(m.0, m.1)).into_iter().filter(|&i| pips[i].map_or(false, |pp| pp.contains(m.0, m.1))));
                line_hits.push((in_areas, lids));
            }
        }
        if t != "freight" || prop {
            let band = speed_kmh(s(p, "maxspeed")).filter(|k| *k != 0.0).map_or(-1, |k| {
                let k = k.round_ties_even() as i64;
                SPEED_BAND_MINS.iter().find(|&&b| k >= b).copied().unwrap_or(0)
            });
            speed_groups.entry((band, if prop { 2 } else { con as u8 })).or_default().push(c.clone());
        }
        if prop { prop_groups.entry(t.to_string()).or_default().push(c); } else { groups.entry((t.to_string(), con)).or_default().push(c); }
    }
    for (ix, lids) in &line_hits {
        for &i in ix {
            for l in lids {
                if let Some(lt) = line_type.get(l).filter(|lt| !lt.is_empty()) {
                    areas[i].lines.entry(lt.to_string()).or_default().insert(l.to_string());
                }
            }
        }
    }
    let results: Vec<((String, bool), Vec<f64>)> = groups.par_iter().map(|(k, ways)| {
        let refs: Vec<&Vec<(f64, f64)>> = ways.iter().collect();
        (k.clone(), group_km(&refs, n_areas, &tree, &pips))
    }).collect();
    for (k, km) in results {
        for (i, v) in km.into_iter().enumerate() {
            if v > 0.0 {
                areas[i].km.insert(k.clone(), v);
            }
        }
    }

    let prop_results: Vec<(String, Vec<f64>)> = prop_groups.par_iter().map(|(k, ways)| {
        let refs: Vec<&Vec<(f64, f64)>> = ways.iter().collect();
        (k.clone(), group_km(&refs, n_areas, &tree, &pips))
    }).collect();
    for (k, km) in prop_results {
        for (i, v) in km.into_iter().enumerate() {
            if v > 0.0 {
                areas[i].km_prop.insert(k.clone(), v);
            }
        }
    }

    let speed_results: Vec<((i64, u8), Vec<f64>)> = speed_groups.par_iter().map(|(k, ways)| {
        let refs: Vec<&Vec<(f64, f64)>> = ways.iter().collect();
        (*k, group_km(&refs, n_areas, &tree, &pips))
    }).collect();
    for ((k, kind), km) in speed_results {
        for (i, v) in km.into_iter().enumerate() {
            if v > 0.0 {
                match kind { 2 => areas[i].speed_prop.insert(k, v), 1 => areas[i].speed_con.insert(k, v), _ => areas[i].speed.insert(k, v) };
            }
        }
    }

    for st in stations {
        let p = &st["properties"];
        let xy = &st["geometry"]["coordinates"];
        let (x, y) = (xy[0].as_f64().unwrap_or(0.0), xy[1].as_f64().unwrap_or(0.0));
        let modes: BTreeSet<&str> = p["modes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let mut here = vec![0usize];
        here.extend(tree.at(&Point::new(x, y)).into_iter().filter(|&i| pips[i].map_or(false, |pp| pp.contains(x, y))));
        for i in here {
            let a = &mut areas[i];
            if p["construction"].as_bool() == Some(true) {
                *a.st.entry("construction").or_default() += 1;
                continue;
            }
            *a.st.entry("open").or_default() += 1;
            *a.st.entry(match p["tier"].as_i64() { Some(1) => "tier1", Some(2) => "tier2", _ => "tier3" }).or_default() += 1;
            if modes.contains("subway") { *a.st.entry("metro").or_default() += 1; }
            if modes.contains("light_rail") || modes.contains("tram") { *a.st.entry("light_rail").or_default() += 1; }
            if p["hsr"].as_bool() == Some(true) || p["line_counts"]["high_speed"].as_i64().unwrap_or(0) > 0 { *a.st.entry("hsr").or_default() += 1; }
        }
    }

    let mut rows = Vec::new();
    let mut shapes = Map::new();
    let mut log = Vec::new();
    let mut dropped = 0;
    for a in &areas {
        if a.st.get("metro").copied().unwrap_or(0) < a.min_metro || (a.kind != "region" && a.km.is_empty()) {
            dropped += 1;
            continue;
        }
        let mut km = Map::new();
        for t in TYPES {
            let (open, con) = (a.km.get(&(t.to_string(), false)).copied().unwrap_or(0.0), a.km.get(&(t.to_string(), true)).copied().unwrap_or(0.0));
            let prop = a.km_prop.get(&t.to_string()).copied().unwrap_or(0.0);
            if open > 0.0 || con > 0.0 || prop > 0.0 {
                km.insert(t.into(), json!({ "open": r1(open), "con": r1(con), "prop": r1(prop) }));
            }
        }
        let lines: Map<String, Value> = TYPES.iter().filter_map(|t| a.lines.get(*t).map(|s| (t.to_string(), json!(s.len())))).collect();
        let band = |m: &BTreeMap<i64, f64>| -> Map<String, Value> { m.iter().map(|(b, v)| ((if *b < 0 { "none".to_string() } else { b.to_string() }), json!(r1(*v)))).collect() };
        let (speed, speed_con, speed_prop) = (band(&a.speed), band(&a.speed_con), band(&a.speed_prop));
        let st: Map<String, Value> = a.st.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
        let open: f64 = a.km.iter().filter(|((_, c), _)| !c).map(|(_, v)| v).sum();
        if rows.len() < 6 {
            log.push(format!("{} {:.0} km", a.slug, open));
        }
        if let Some(p) = &a.poly {
            shapes.insert(a.slug.clone(), shape(p));
        }
        rows.push(json!({ "slug": a.slug, "name": a.name, "local": a.local, "kind": a.kind, "bbox": a.bbox, "km": km, "lines": lines, "speed": speed, "speed_con": speed_con, "speed_prop": speed_prop, "stations": st }));
    }
    let dir = out.join(slug);
    fs::create_dir_all(&dir).expect("create output folder");
    fs::write(dir.join("stats.json"), serde_json::to_string(&json!({ "areas": rows })).unwrap()).expect("write stats.json");
    fs::write(dir.join("shapes.json"), serde_json::to_string(&Value::Object(shapes)).unwrap()).expect("write shapes.json");
    format!("{} areas ({} left out): {}", rows.len(), dropped, log.join(", "))
}
