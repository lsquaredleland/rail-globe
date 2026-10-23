//! Tile input and side files, ported from pipeline/build_tiles.py: the GeoJSON layers the Rust tile writer (tilewriter.rs) turns into MLT PMTiles
//! (tile input only; main.rs runs the writer afterwards) and the JSON side files the app loads on demand.
//! Layers (docs/02-architecture.md): track (with the merged zoomed-out overview), lines (one strand per line per way), areas,
//! buildings, platforms, stations, complexes, airports, airport_pts. Tiles include each class one zoom before the style shows it.
//!
//! `run` is a list of steps, each in its own file: `track` (the pass over the features), `airports`, `station_rows`, `dashes` + `cities`
//! (construction dashes and city dots), then `side` (the side files). `props` holds the properties of every layer, `constants` the numbers
//! shared with the app (shared/constants.json), `util` the small helpers.

mod airports;
mod cities;
pub(crate) mod constants;
mod dashes;
mod props;
mod records;
mod side;
mod station_rows;
mod track;
mod util;

use crate::site::Input;
use rustc_hash::FxHashMap;
use serde_json::{Map, Value};
use std::fs;
use std::io::Write;
use std::path::Path;
use util::track_minzoom;

pub(crate) use util::{haversine_len, write_shards_n};

// Zoomed out (tile zooms 0..OVERVIEW_MAXZOOM) the track layer holds a merged overview instead of every piece (docs/00-overview.md)
const OVERVIEW_MAXZOOM: i64 = 8;
// Construction and proposed track is drawn as dashes cut in the pipeline (no ids, no tags), layer `con_dash`; the per-way features stay in `track` for
// hover, click and highlight. Yard track is left out. A type's dashes start at its own track_minzoom, but not before CON_DASH_MINZOOM.
const CON_DASH_MINZOOM: i64 = 3;
const SKETCH_MINZOOM: i64 = 4; // the metro sketch (type only) is drawn from here to the zoom before the metro overview starts
const BIG_LINE_BYTES: usize = 64 * 1024; // a line record over this (long highlight geometry) stays its own file in lines-big/ instead of bloating a shard

/// the tile layers of one region: (tile layer, file in tiles_tmp), in stacking order (the overview is the first "track" file)
pub const TILE_LAYERS: [(&str, &str); 12] = [
    ("track", "track_overview.geojsonl"),
    ("track", "track.geojsonl"),
    ("con_dash", "con_dash.geojsonl"),
    ("lines", "lines.geojsonl"),
    ("areas", "areas.geojsonl"),
    ("buildings", "buildings.geojsonl"),
    ("platforms", "platforms.geojsonl"),
    ("stations", "stations.geojsonl"),
    ("complexes", "complexes.geojsonl"),
    ("airports", "airports.geojsonl"),
    ("airport_pts", "airport_pts.geojsonl"),
    ("cities", "cities.geojsonl"),
];

pub struct Prepared {
    pub summary: String,
}

/// The region's lines and relations by id, and which line each route belongs to.
pub(crate) struct Lookup<'a> {
    pub lines: Vec<&'a Value>,
    pub rels: FxHashMap<&'a str, &'a Value>,
    pub route_line: FxHashMap<&'a str, &'a str>,
    line_by: FxHashMap<&'a str, &'a Value>,
}

impl<'a> Lookup<'a> {
    fn new(inp: &'a Input) -> Lookup<'a> {
        let lines: Vec<&Value> = inp.lines.iter().collect();
        let line_by: FxHashMap<&str, &Value> = lines.iter().map(|l| (l["id"].as_str().unwrap_or(""), *l)).collect();
        let mut route_line: FxHashMap<&str, &str> = FxHashMap::default();
        for l in &lines {
            for r in l["routes"].as_array().into_iter().flatten() {
                route_line.insert(r.as_str().unwrap_or(""), l["id"].as_str().unwrap_or(""));
            }
        }
        let rels: FxHashMap<&str, &Value> = inp.rels.iter().map(|r| (r["id"].as_str().unwrap_or(""), r)).collect();
        Lookup { lines, rels, route_line, line_by }
    }

    /// a string field of a line
    pub fn ls(&self, line: &str, key: &str) -> Option<&'a str> {
        self.line_by.get(line).and_then(|v| v[key].as_str())
    }
}

/// Operator logos (pipeline/logos): the Wikidata id of a station's operator, when we have its logo.
pub(crate) struct Logos {
    ids: std::collections::HashSet<String>,
    china: bool,
}

impl Logos {
    fn load(inp: &Input, out: &Path) -> Logos {
        // logos/logos.json sits in app/static, beside the data folder's parent (app/public/data, or app/public/data/world for the world build)
        let logos_json = out.ancestors().map(|a| a.join("static/logos/logos.json")).find(|p| p.exists());
        let ids: std::collections::HashSet<String> = logos_json
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<Map<String, Value>>(&t).ok())
            .map(|m| m.into_iter().map(|(k, _)| k).collect())
            .unwrap_or_default();
        if ids.is_empty() {
            eprintln!("warning: no logos/logos.json found above {}; stations get no logo", out.display());
        }
        let china = inp.cfg.get("china").is_some_and(crate::config::truthy);
        Logos { ids, china }
    }

    /// the station's own operator:wikidata, else the commonest one of the lines serving it, else China Railway for a train station in China
    fn for_station(&self, p: &Value, lk: &Lookup) -> Option<String> {
        let first = |v: &str| v.split(';').next().unwrap_or("").trim().to_string();
        if let Some(q) = p["operator_wd"].as_str().map(first).filter(|q| self.ids.contains(q)) {
            return Some(q);
        }
        let mut n: Vec<(String, usize)> = Vec::new();
        for lid in p["line_ids"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if let Some(q) = lk.ls(lid, "operator_wd").map(first).filter(|q| self.ids.contains(q)) {
                match n.iter_mut().find(|e| e.0 == q) {
                    Some(e) => e.1 += 1,
                    None => n.push((q, 1)),
                }
            }
        }
        if let Some(best) = n.into_iter().max_by_key(|e| e.1) {
            return Some(best.0);
        }
        (self.china && p["modes"].as_array().is_some_and(|m| m.iter().any(|x| x.as_str() == Some("train"))) && self.ids.contains("Q1073489"))
            .then(|| "Q1073489".to_string())
    }
}

/// Makes the output folders; the side-file folders are emptied first (shard counts change between builds, so stale files must go).
fn prepare_dirs(tmp: &Path, side: &Path) {
    for d in ["lines", "lines-big", "stations", "tags", "name-hl"] {
        let _ = fs::remove_dir_all(side.join(d));
    }
    for d in [tmp, &side.join("lines"), &side.join("lines-big"), &side.join("stations"), &side.join("tags"), &side.join("name-hl")] {
        fs::create_dir_all(d).expect("create output folder");
    }
}

/// The construction and proposed track as dashes: ways merged end to end per (type, proposed, speed band), parallel tracks made one line.
fn con_dash_text(con_groups: std::collections::BTreeMap<(String, bool, i64), Vec<Vec<(f64, f64)>>>) -> String {
    let mut out = String::new();
    for ((t, prop, band), ways) in con_groups {
        let merged = dashes::linemerge(ways);
        for line in dashes::dedupe_parallel(merged.iter().map(|l| l.iter().map(|&p| dashes::to_merc(p)).collect()).collect()) {
            dashes::con_dashes(&mut out, &t, prop, band, &line, track_minzoom(&t).max(CON_DASH_MINZOOM));
        }
    }
    out
}

/// Writes the tile input files, one per layer, in TILE_LAYERS order.
fn write_tile_input(tmp: &Path, texts: [&String; 12]) {
    for ((_, file), text) in TILE_LAYERS.iter().zip(texts) {
        let file = tmp.join(file);
        fs::File::create(&file).and_then(|mut w| w.write_all(text.as_bytes())).unwrap_or_else(|e| panic!("write {}: {e}", file.display()));
    }
}

/// Writes the tile input (tiles_tmp), then the side files; the caller runs the tile writer afterwards. `stations` and `complexes`
/// are the station model's features, construction stations appended.
pub fn run(
    slug: &str,
    inp: &Input,
    stations: &mut [Value],
    complexes: &mut [Value],
    route_order: &Map<String, Value>,
    data: &Path,
    out: &Path,
    regions_dirs: &[std::path::PathBuf],
) -> Prepared {
    let tmp = data.join("tiles_tmp");
    let side_dir = out.join(slug);
    prepare_dirs(&tmp, &side_dir);
    let lk = Lookup::new(inp);
    let logos = Logos::load(inp, out);

    // track, strands, areas, buildings, platforms, the overview, and the track beside stations
    let mut sc = track::Scan::run(inp, &lk);
    // airports (outline + terminals) and the stations that serve them
    let airports = airports::Airports::build(inp);
    // stations and complexes
    station_rows::demote_construction(slug, &sc, stations, complexes);
    let rows = station_rows::rows(&sc, stations, complexes, &airports, &logos, &lk);
    // construction dashes, and city dots: where metro and tram systems are, visible from far out
    let con_dash = con_dash_text(std::mem::take(&mut sc.con_groups));
    let cities_text = cities::city_dots(&sc.city_track);

    // tile input: every layer, the overview first
    let mut overview = std::mem::take(&mut sc.overview);
    overview.sort_by(|a, b| a.0.cmp(&b.0));
    let mut ov_text = String::new();
    for (_, props, geom, mz, xz) in &overview {
        util::feat(&mut ov_text, props, geom, Some(*mz), Some(*xz));
    }
    write_tile_input(
        &tmp,
        [
            &ov_text,
            &sc.track,
            &con_dash,
            &sc.strands,
            &sc.areas,
            &sc.buildings,
            &sc.plats,
            &rows.stations,
            &rows.complexes,
            &airports.airports,
            &airports.airport_pts,
            &cities_text,
        ],
    );

    let n_named = side::Side {
        slug,
        inp,
        lk: &lk,
        sc: &sc,
        stations,
        complexes,
        route_order,
        airport_of: &rows.airport_of,
        logos: &logos,
        dir: &side_dir,
        out,
        regions_dirs,
    }
    .write_all();
    Prepared {
        summary: format!(
            "{slug}: track={}, lines={}, areas={}, buildings={}, platforms={}, stations={}, complexes={}, airports={}, airport_pts={}; track overview z0-{OVERVIEW_MAXZOOM}: {} pieces; side files {} stations, {} lines, {n_named} named railway lines",
            sc.n_track, sc.n_strands, sc.n_areas, sc.n_bld, sc.n_plats, stations.len(), complexes.len(), airports.n_air, airports.n_air_pts, overview.len(), stations.len(), lk.lines.len()
        ),
    }
}
