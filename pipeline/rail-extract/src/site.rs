//! `rail-extract build <slug>`: the station model, stations under construction, tile input, tippecanoe and side files for one
//! region, from the cut's files in data/<slug>/ (was stations.py, con_stations.py and build_tiles.py). Writes the same files.

use crate::pyjson::{self, COMPACT, SPACED};
use crate::{construction, stations, tiles};
use rayon::prelude::*;
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// one GeoJSON feature line: properties (in file order), geometry, and the geometry's text as written (passed through to the
/// tile input unchanged, as Python's json round trip left it)
pub struct Feat {
    pub p: Map<String, Value>,
    pub g: Value,
    pub graw: String,
    pub gtype: String,
}

pub struct Input {
    pub feats: Vec<Feat>,
    pub rels: Vec<Value>,
    pub stops: Vec<Value>,
    pub lines: Vec<Value>,
    pub airports: Vec<Feat>,
    pub cfg: Map<String, Value>,
}

/// a string property
#[inline]
pub fn s<'a>(p: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    p.get(k).and_then(Value::as_str)
}

fn read_lines(path: &Path) -> Vec<String> {
    match fs::read_to_string(path) {
        Ok(t) => t.lines().filter(|l| !l.is_empty()).map(String::from).collect(),
        Err(_) => Vec::new(),
    }
}

fn parse_feats(path: &Path) -> Vec<Feat> {
    read_lines(path)
        .into_par_iter()
        .map(|line| {
            let mut v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let p = match v["properties"].take() {
                Value::Object(m) => m,
                _ => Map::new(),
            };
            let g = v["geometry"].take();
            let at = line.rfind("\"geometry\":").expect("feature without geometry");
            let graw = line[at + 11..line.len() - 1].to_string();
            let gtype = g["type"].as_str().unwrap_or("").to_string();
            Feat { p, g, graw, gtype }
        })
        .collect()
}

fn parse_values(path: &Path) -> Vec<Value> {
    read_lines(path).into_par_iter().map(|l| serde_json::from_str(&l).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).collect()
}

pub fn load(dir: &Path, cfg: &Path) -> Input {
    Input {
        feats: parse_feats(&dir.join("features.typed.geojsonl")),
        rels: parse_values(&dir.join("relations.jsonl")),
        stops: parse_values(&dir.join("stop_positions.jsonl")),
        lines: fs::read_to_string(dir.join("lines.json")).ok().and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok()).unwrap_or_default(),
        airports: parse_feats(&dir.join("airports.geojsonl")),
        cfg: fs::read_to_string(cfg).ok().and_then(|t| serde_json::from_str::<Map<String, Value>>(&t).ok()).unwrap_or_default(),
    }
}

fn write_lines(path: &Path, rows: &[Value]) {
    let mut out = String::new();
    for r in rows {
        pyjson::write(&mut out, r, COMPACT);
        out.push('\n');
    }
    fs::write(path, out).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

pub struct Paths {
    pub data: PathBuf,         // data/
    pub config: PathBuf,       // pipeline/config/
    pub out: PathBuf,          // app/public/data/
    pub regions: Vec<PathBuf>, // pipeline/regions/, data/regions/
}

/// Build one region; returns its log lines.
pub fn build(slug: &str, paths: &Paths, tiles_on: bool) -> Vec<String> {
    let t0 = Instant::now();
    let mut log = Vec::new();
    let lap = |log: &mut Vec<String>, t: &mut Instant, what: String| {
        log.push(format!("  {:6.2}s  {what}", t.elapsed().as_secs_f64()));
        *t = Instant::now();
    };
    let mut t = Instant::now();
    let dir = paths.data.join(slug);
    let inp = load(&dir, &paths.config.join(format!("{slug}.json")));
    lap(&mut log, &mut t, format!("read {} features, {} relations, {} stop positions", inp.feats.len(), inp.rels.len(), inp.stops.len()));

    let mut st = stations::run(slug, &inp);
    fs::write(dir.join("route_stations.json"), pyjson::to_string(&Value::Object(st.route_order.clone()), SPACED)).expect("write route_stations.json");
    fs::write(dir.join("stations_report.json"), pyjson::pretty_ascii(&st.report)).expect("write stations_report.json");
    lap(&mut log, &mut t, format!("station model: {} stations, {} complexes", st.stations.len(), st.complexes.len()));

    let con = construction::run(slug, &inp, &mut st.stations, &mut st.complexes);
    write_lines(&dir.join("stations.geojsonl"), &st.stations);
    write_lines(&dir.join("complexes.geojsonl"), &st.complexes);
    lap(&mut log, &mut t, format!("under construction: {con}"));

    let prep = tiles::run(slug, &inp, &st.stations, &st.complexes, &st.route_order, &dir, &paths.out, &paths.regions, tiles_on);
    lap(&mut log, &mut t, "tile input and side files written".to_string());
    if let Some(mut child) = prep.tippecanoe {
        let ok = child.wait().map_or(false, |s| s.success());
        assert!(ok, "tippecanoe failed for {slug}");
        let mb = fs::metadata(&prep.pmtiles).map_or(0.0, |m| m.len() as f64 / 1e6);
        lap(&mut log, &mut t, format!("tippecanoe finished (waited): {mb:.2} MB"));
    }
    log.push(format!("  {}", prep.summary));
    log.push(format!("  {:6.2}s  total", t0.elapsed().as_secs_f64()));
    log
}
