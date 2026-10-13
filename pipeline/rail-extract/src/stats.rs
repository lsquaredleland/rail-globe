//! Region statistics for the app's info card: how much rail a region (and the areas inside it that its config lists under
//! `stat_areas`) has, written to <out>/<slug>/stats.json. The region itself counts everything in its cut; an area inside it is
//! its outline (pipeline/regions/<slug>.geojson, else data/regions/<slug>.geojson) applied to the same features.
//! Track is counted as track km: every mapped track counts, so a double-track line is about twice its route length (OSM maps
//! most double track as one way per track; a way tagged tracks=N counts N times).

use crate::geomx;
use crate::site::{s, Input};
use geo::{Contains, MultiPolygon, Point};
use rustc_hash::FxHashMap;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// rail types counted, in the order the card lists them
const TYPES: [&str; 9] = ["high_speed", "intercity", "regional", "metro", "light_rail", "monorail", "funicular", "other", "freight"];

struct Area {
    slug: String,
    name: String,
    local: Option<String>,
    poly: Option<MultiPolygon<f64>>, // None: the whole region
    bbox: [f64; 4],                  // w, s, e, n
    km: BTreeMap<(String, bool), f64>,
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

fn seg_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = (b.0 - a.0).to_radians() * ((a.1 + b.1) / 2.0).to_radians().cos();
    let dy = (b.1 - a.1).to_radians();
    dx.hypot(dy) * 6371.0088
}

fn tracks_mult(tr: &str) -> f64 {
    match tr.parse::<i64>() {
        Ok(n) if n > 0 && tr.chars().all(|c| c.is_ascii_digit()) => n.min(4) as f64,
        _ => 1.0,
    }
}

fn r1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

pub fn run(slug: &str, inp: &Input, stations: &[Value], dirs: &[PathBuf], out: &Path) -> String {
    let new = |slug: &str, name: String, local: Option<String>, poly: Option<MultiPolygon<f64>>, bbox: [f64; 4]| Area {
        slug: slug.to_string(), name, local, poly, bbox, km: BTreeMap::new(), lines: BTreeMap::new(), st: BTreeMap::new(),
    };
    let mut areas: Vec<Area> = Vec::new();
    let own = outline(slug, dirs);
    areas.push(new(slug, inp.cfg.get("stat_name").and_then(Value::as_str).unwrap_or(slug).to_string(),
        inp.cfg.get("stat_local").and_then(Value::as_str).map(String::from), None, own.map_or([-180.0, -90.0, 180.0, 90.0], |o| o.1)));
    for a in inp.cfg.get("stat_areas").and_then(Value::as_array).into_iter().flatten() {
        let aslug = a["slug"].as_str().unwrap_or("");
        let Some((poly, bbox)) = outline(aslug, dirs) else { panic!("stats: no outline for {aslug} (pipeline/regions or data/regions)") };
        areas.push(new(aslug, a["name"].as_str().unwrap_or(aslug).to_string(), a["local"].as_str().map(String::from), Some(poly), bbox));
    }

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

    for fe in &inp.feats {
        let p = &fe.p;
        let Some(t) = s(p, "type").filter(|t| TYPES.contains(t)) else { continue };
        if fe.gtype != "LineString" || s(p, "railway") == Some("proposed") {
            continue;
        }
        let con = s(p, "railway") == Some("construction");
        let c: Vec<(f64, f64)> = fe.g["coordinates"].as_array().into_iter().flatten().filter_map(|c| Some((c[0].as_f64()?, c[1].as_f64()?))).collect();
        if c.len() < 2 {
            continue;
        }
        let mult = tracks_mult(s(p, "tracks").unwrap_or(""));
        let (mut w0, mut s0, mut e0, mut n0) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for &(x, y) in &c {
            w0 = w0.min(x); e0 = e0.max(x); s0 = s0.min(y); n0 = n0.max(y);
        }
        let lids: Vec<&str> = if con { Vec::new() } else {
            p.get("_routes").and_then(Value::as_array).into_iter().flatten().filter_map(|r| route_line.get(r.as_str().unwrap_or("")).copied()).collect()
        };
        for a in areas.iter_mut() {
            let km = match &a.poly {
                None => c.windows(2).map(|w| seg_km(w[0], w[1])).sum::<f64>(),
                Some(poly) => {
                    if w0 > a.bbox[2] || e0 < a.bbox[0] || s0 > a.bbox[3] || n0 < a.bbox[1] {
                        continue;
                    }
                    // a segment belongs to the area when its midpoint does
                    c.windows(2).filter(|w| poly.contains(&Point::new((w[0].0 + w[1].0) / 2.0, (w[0].1 + w[1].1) / 2.0))).map(|w| seg_km(w[0], w[1])).sum::<f64>()
                }
            };
            if km <= 0.0 {
                continue;
            }
            *a.km.entry((t.to_string(), con)).or_default() += km * mult;
            for l in &lids {
                if let Some(lt) = line_type.get(l).filter(|lt| !lt.is_empty()) {
                    a.lines.entry(lt.to_string()).or_default().insert(l.to_string());
                }
            }
        }
    }

    for st in stations {
        let p = &st["properties"];
        let xy = &st["geometry"]["coordinates"];
        let pt = Point::new(xy[0].as_f64().unwrap_or(0.0), xy[1].as_f64().unwrap_or(0.0));
        let modes: BTreeSet<&str> = p["modes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        for a in areas.iter_mut() {
            if a.poly.as_ref().map_or(false, |poly| !poly.contains(&pt)) {
                continue;
            }
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
    let mut log = Vec::new();
    for a in &areas {
        let mut km = Map::new();
        for t in TYPES {
            let (open, con) = (a.km.get(&(t.to_string(), false)).copied().unwrap_or(0.0), a.km.get(&(t.to_string(), true)).copied().unwrap_or(0.0));
            if open > 0.0 || con > 0.0 {
                km.insert(t.into(), json!({ "open": r1(open), "con": r1(con) }));
            }
        }
        let lines: Map<String, Value> = TYPES.iter().filter_map(|t| a.lines.get(*t).map(|s| (t.to_string(), json!(s.len())))).collect();
        let st: Map<String, Value> = a.st.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
        let open: f64 = a.km.iter().filter(|((_, c), _)| !c).map(|(_, v)| v).sum();
        log.push(format!("{} {:.0} km", a.slug, open));
        rows.push(json!({ "slug": a.slug, "name": a.name, "local": a.local, "bbox": a.bbox, "km": km, "lines": lines, "stations": st }));
    }
    let dir = out.join(slug);
    fs::create_dir_all(&dir).expect("create output folder");
    fs::write(dir.join("stats.json"), serde_json::to_string(&json!({ "areas": rows })).unwrap()).expect("write stats.json");
    log.join(", ")
}
