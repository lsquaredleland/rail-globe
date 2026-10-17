//! `rail-extract build <slug>`: the station model, stations under construction, tile input and side files for one
//! region, from the cut's files in data/<slug>/ (was stations.py, con_stations.py and build_tiles.py). Writes the same files.

use crate::pyjson::{self, COMPACT, SPACED};
use crate::{construction, stations, stats, tiles};
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
        cfg: if cfg.exists() { crate::config::load_file(cfg) } else { Map::new() },
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

/// The config file for a slug: pipeline/config/<slug>.json, else the template pipeline/config/_default.json (second value true).
pub fn config_path(config_dir: &Path, slug: &str) -> (PathBuf, bool) {
    let own = config_dir.join(format!("{slug}.json"));
    if own.exists() {
        (own, false)
    } else if let Some(group) = config_group(config_dir, slug) {
        (config_dir.join(format!("{group}.json")), true)   // a template like the default: names are filled from the outline
    } else {
        (config_dir.join("_default.json"), true)
    }
}

/// `_groups.json`: {"<config name>": ["<slug>", ...]}. A region with no file of its own whose slug is listed there uses that config
/// (itself an "extends": "_default" template) instead of `_default.json`: one file for a group of countries (docs/12).
fn config_group(config_dir: &Path, slug: &str) -> Option<String> {
    let path = config_dir.join("_groups.json");
    let text = std::fs::read_to_string(&path).ok()?; // no file: no groups
    // a broken _groups.json must stop the run (the whole group would quietly use _default.json)
    let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let groups = v.as_object().unwrap_or_else(|| panic!("{} must be a JSON object", path.display()));
    let mut found: Vec<&String> = Vec::new();
    for (k, list) in groups.iter().filter(|(k, _)| k.as_str() != "_comment") {
        let list = list.as_array().unwrap_or_else(|| panic!("{}: \"{k}\" must be a list of slugs", path.display()));
        if list.iter().any(|s| s.as_str() == Some(slug)) {
            found.push(k);
        }
    }
    if found.len() > 1 {
        panic!("{}: slug \"{slug}\" is in more than one group ({found:?})", path.display());
    }
    found.first().map(|k| {
        assert!(config_dir.join(format!("{k}.json")).exists(), "{}: group \"{k}\" has no {k}.json", path.display());
        (*k).clone()
    })
}

/// True when the config file itself starts from a template (has an `extends` key).
fn extends_template(path: &Path) -> bool {
    fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).map_or(false, |v| v.get("extends").is_some())
}

/// Fill the slug-specific keys of the template from the region's outline properties: stat_name <- name_en (else name),
/// stat_local <- name, stat_auto.country <- country. The generic configs written by hand for the world build had exactly these.
pub fn fill_default(cfg: &mut Map<String, Value>, props: &Value) {
    let name = props["name"].as_str().unwrap_or("");
    let en = props["name_en"].as_str().unwrap_or(name);
    // explicit config values win; only missing keys are filled (an own-file config that extends _default sets none of them)
    cfg.entry("stat_name").or_insert_with(|| Value::from(en));
    cfg.entry("stat_local").or_insert_with(|| Value::from(name));
    if let Some(Value::Object(auto)) = cfg.get_mut("stat_auto") {
        auto.entry("country").or_insert_with(|| props["country"].clone());
    }
}

/// The stale exemptions (matched fewer features than their expect_min) that a region's cut wrote to classify_report.json.
pub fn stale_exemptions(report: &Value, slug: &str) -> Vec<String> {
    let Some(ex) = report["exemptions"].as_object() else { return Vec::new() };
    ex.iter()
        .filter(|(_, e)| e["expect_min_met"] == false)
        .map(|(id, e)| format!("{slug}: stale exemption \"{id}\" matched {} features, expected at least {}", e["matched"], e["expect_min"]))
        .collect()
}

fn stale_exemption_warnings(dir: &Path) -> Vec<String> {
    let slug = dir.file_name().and_then(|s| s.to_str()).unwrap_or("");
    fs::read_to_string(dir.join("classify_report.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map_or(Vec::new(), |r| stale_exemptions(&r, slug))
}

pub struct Paths {
    pub data: PathBuf,         // data/
    pub config: PathBuf,       // pipeline/config/
    pub out: PathBuf,          // app/public/data/
    pub regions: Vec<PathBuf>, // pipeline/regions/, data/regions/
}

/// Build one region; returns its log lines.
pub fn build(slug: &str, paths: &Paths) -> Vec<String> {
    let t0 = Instant::now();
    let mut log = Vec::new();
    let lap = |log: &mut Vec<String>, t: &mut Instant, what: String| {
        log.push(format!("  {:6.2}s  {what}", t.elapsed().as_secs_f64()));
        *t = Instant::now();
    };
    let mut t = Instant::now();
    let dir = paths.data.join(slug);
    let (cfg_path, is_default) = config_path(&paths.config, slug);
    let mut inp = load(&dir, &cfg_path);
    // a file of its own that says "extends" gets the same fill for the keys it leaves out; the hand-written pilot configs (no extends) stay as written
    if is_default || extends_template(&cfg_path) {
        let outline = paths.regions.iter().map(|d| d.join(format!("{slug}.geojson"))).find(|p| p.exists());
        let props = outline
            .and_then(|p| serde_json::from_str::<Value>(&fs::read_to_string(p).ok()?).ok())
            .map_or(Value::Null, |v| v["properties"].clone());
        fill_default(&mut inp.cfg, &props);
        if is_default {
            log.push(format!("  config: pipeline/config/_default.json (no {slug}.json)"));
        }
    }
    log.extend(stale_exemption_warnings(&dir).into_iter().map(|w| format!("  WARNING: {w}")));
    lap(&mut log, &mut t, format!("read {} features, {} relations, {} stop positions", inp.feats.len(), inp.rels.len(), inp.stops.len()));

    let mut st = stations::run(slug, &inp);
    fs::write(dir.join("route_stations.json"), pyjson::to_string(&Value::Object(st.route_order.clone()), SPACED)).expect("write route_stations.json");
    fs::write(dir.join("stations_report.json"), pyjson::pretty_ascii(&st.report)).expect("write stations_report.json");
    lap(&mut log, &mut t, format!("station model: {} stations, {} complexes", st.stations.len(), st.complexes.len()));

    let con = construction::run(slug, &inp, &mut st.stations, &mut st.complexes);
    write_lines(&dir.join("stations.geojsonl"), &st.stations);
    write_lines(&dir.join("complexes.geojsonl"), &st.complexes);
    lap(&mut log, &mut t, format!("under construction: {con}"));

    let prep = tiles::run(slug, &inp, &mut st.stations, &mut st.complexes, &st.route_order, &dir, &paths.out, &paths.regions);
    lap(&mut log, &mut t, "tile input and side files written".to_string());
    // after the tiles: they turn stations that are only being built into construction stations, which the card counts that way
    write_lines(&dir.join("stations.geojsonl"), &st.stations);
    write_lines(&dir.join("complexes.geojsonl"), &st.complexes);
    let summary = stats::run(slug, &inp, &st.stations, &paths.regions, &paths.out);
    lap(&mut log, &mut t, format!("region statistics: {summary}"));
    log.push(format!("  {}", prep.summary));
    log.push(format!("  {:6.2}s  total", t0.elapsed().as_secs_f64()));
    log
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_filled_from_the_outline() {
        let mut cfg: Map<String, Value> = serde_json::from_str(include_str!("../../config/_default.json")).unwrap();
        let props: Value = serde_json::from_str(r#"{"name":"Österreich","name_en":"Austria","country":"AT"}"#).unwrap();
        fill_default(&mut cfg, &props);
        assert_eq!(cfg["stat_name"], "Austria");
        assert_eq!(cfg["stat_local"], "Österreich");
        assert_eq!(cfg["stat_auto"], serde_json::json!({"levels": ["4"], "country": "AT"}));
        assert_eq!(cfg["high_speed_min_kmh"], 200);
    }

    #[test]
    fn own_file_with_only_extends_gets_default_names() {
        let dir = std::env::temp_dir().join(format!("site_own_cfg_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("_default.json"), include_str!("../../config/_default.json")).unwrap();
        std::fs::write(dir.join("xx.json"), r#"{"extends":"_default"}"#).unwrap();
        std::fs::write(dir.join("yy.json"), r#"{"extends":"_default","stat_name":"Mine","stat_auto":{"country":"ZZ"}}"#).unwrap();
        let props: Value = serde_json::from_str(r#"{"name":"Xland","name_en":"Xlandia","country":"XX"}"#).unwrap();
        let (p, is_default) = config_path(&dir, "xx");
        assert!(!is_default && extends_template(&p));
        let mut cfg = crate::config::load_file(&p);
        fill_default(&mut cfg, &props);
        assert_eq!(cfg["stat_name"], "Xlandia");
        assert_eq!(cfg["stat_local"], "Xland");
        assert_eq!(cfg["stat_auto"], serde_json::json!({"levels": ["4"], "country": "XX"}));
        let mut cfg = crate::config::load_file(&dir.join("yy.json"));
        fill_default(&mut cfg, &props);
        assert_eq!(cfg["stat_name"], "Mine");
        assert_eq!(cfg["stat_local"], "Xland");
        assert_eq!(cfg["stat_auto"]["country"], "ZZ");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn config_path_falls_back_to_the_template() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../config");
        assert_eq!(config_path(&dir, "paris"), (dir.join("paris.json"), false));
        assert_eq!(config_path(&dir, "no-such-slug"), (dir.join("_default.json"), true));
        // listed in _groups.json: the group's template
        assert_eq!(config_path(&dir, "tz"), (dir.join("_evidence-stations.json"), true));
        assert_eq!(config_path(&dir, "ar"), (dir.join("_evidence-routes.json"), true));
    }
}
