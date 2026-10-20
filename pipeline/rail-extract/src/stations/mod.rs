//! Station model (docs/04-station-model.md).
//!
//! Levels: Complex (the place a traveller names) > Station (one per mode group) > stop points. Rank comes from physical size only;
//! lines are counted for the panel and never set rank. Where the first of several equal candidates is needed, this takes the lowest
//! index (or sorted order), so runs repeat.
//!
//! `run` is the steps in order, one module each:
//!   - features: sort the cut's features, project them to metres, index them; drop stations still under construction
//!   - group: stations from stop areas and station features, platforms given to them, duplicates merged (steps 1 to 3b)
//!   - size: platform, track, land and building size, score, tier, major-station flag
//!   - link: stop positions, lines per station, complexes (steps 4 and 5)
//!   - output: names and the rows written to stations.geojsonl and complexes.geojsonl, and the report
//!   - names: name helpers; the region-specific word lists are in pipeline/config/_station-names.json

mod features;
mod group;
mod link;
mod names;
mod output;
mod size;
#[cfg(test)]
mod tests;

pub use names::english_name;

use crate::geomx::{self, Index, UnionFind};
use crate::site::Input;
use features::{Feats, Proj};
use geo::{Geometry, Point};
use group::Grouped;
use rustc_hash::FxHashMap;
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::sync::LazyLock;

const RADIUS_TRAIN: f64 = 300.0; // metres, tuned on the pilots
const RADIUS_OTHER: f64 = 100.0;
const GROUP_RADIUS: f64 = 300.0;

fn radius(m: &str) -> f64 {
    if m == "train" { RADIUS_TRAIN } else { RADIUS_OTHER }
}

fn max_radius(ms: &BTreeSet<String>) -> f64 {
    ms.iter().map(|m| radius(m)).fold(f64::MIN, f64::max)
}

/// a member of a relation, as the cut writes it (a missing type or id stays None)
struct Member<'a> {
    kind: Option<&'a str>,
    id: Option<i64>,
    role: &'a str,
}

impl Member<'_> {
    /// (osm type, id), the lookup key of node_station and member_area; a missing part reads as "" and 0
    fn key(&self) -> (String, i64) {
        (self.kind.unwrap_or("").to_string(), self.id.unwrap_or(0))
    }
}

fn members(r: &Value) -> impl Iterator<Item = Member<'_>> {
    r["members"].as_array().into_iter().flatten().map(|m| Member { kind: m["type"].as_str(), id: m["id"].as_i64(), role: m["role"].as_str().unwrap_or("") })
}

static NO_TAGS: LazyLock<Map<String, Value>> = LazyLock::new(Map::new);

/// the stop areas and stop area groups of the cut's relations
struct Rels<'a> {
    member_area: Vec<((String, i64), String)>, // member -> stop area, in relation order; a later stop area wins (dict semantics)
    area_tags: FxHashMap<String, &'a Map<String, Value>>,
    group_rows: Vec<&'a Value>,
}

impl<'a> Rels<'a> {
    fn new(inp: &'a Input) -> Rels<'a> {
        let mut member_area: Vec<((String, i64), String)> = Vec::new();
        let mut member_area_ix: FxHashMap<(String, i64), usize> = FxHashMap::default();
        let mut area_tags: FxHashMap<String, &Map<String, Value>> = FxHashMap::default();
        let mut group_rows: Vec<&Value> = Vec::new();
        for r in &inp.rels {
            match r["kind"].as_str() {
                Some("stop_area") => {
                    let rid = r["id"].as_str().unwrap_or("").to_string();
                    area_tags.insert(rid.clone(), r["tags"].as_object().unwrap_or(&NO_TAGS));
                    for m in members(r) {
                        let key = m.key();
                        match member_area_ix.get(&key) {
                            Some(&k) => member_area[k].1 = rid.clone(),
                            None => {
                                member_area_ix.insert(key.clone(), member_area.len());
                                member_area.push((key, rid.clone()));
                            }
                        }
                    }
                }
                Some("stop_area_group") => group_rows.push(r),
                _ => {}
            }
        }
        Rels { member_area, area_tags, group_rows }
    }

    /// the tags of a stop area
    fn area_tag_map(&self, area: &str) -> Option<&'a Map<String, Value>> {
        self.area_tags.get(area).copied()
    }

    /// the name of a stop area
    fn area_name(&self, area: &str) -> Option<&'a str> {
        self.area_tag_map(area)?.get("name").and_then(Value::as_str)
    }
}

struct Station {
    feats: Vec<usize>, // indices into station_feats
    modes: BTreeSet<String>,
    areas: BTreeSet<String>,
    platforms: Vec<usize>, // indices into platforms
    grouping: &'static str,
    pt: Point<f64>,
    norm: String,
    merged_from: usize,
    stated: bool, // some feature's tags state the mode
}

/// the id of a station: its stop area, else its first feature
fn station_id(feats: &Feats, x: &Station) -> String {
    x.areas.iter().next().cloned().unwrap_or_else(|| feats.station_feats[x.feats[0]].id().to_string())
}

/// the Wikidata ids of a station's features
fn station_wikidata(feats: &Feats, x: &Station) -> BTreeSet<String> {
    x.feats.iter().filter_map(|&k| feats.station_feats[k].p("wikidata").filter(|v| !v.is_empty()).map(String::from)).collect()
}

/// an index of the stations' points
fn point_index(st: &[Station]) -> Index {
    Index::new(st.iter().map(|x| geomx::bbox(&Geometry::Point(x.pt))))
}

/// groups in order of their first member
fn groups(uf: &mut UnionFind, n: usize) -> Vec<Vec<usize>> {
    let mut by_root: FxHashMap<usize, usize> = FxHashMap::default();
    let mut out: Vec<Vec<usize>> = Vec::new();
    for i in 0..n {
        let r = uf.find(i);
        let k = *by_root.entry(r).or_insert_with(|| {
            out.push(Vec::new());
            out.len() - 1
        });
        out[k].push(i);
    }
    out
}

/// the final stations with everything the later steps read
struct Model<'a> {
    inp: &'a Input,
    feats: Feats<'a>,
    rels: Rels<'a>,
    st: Vec<Station>,
    st_tree: Index,
    area_to_station: FxHashMap<String, usize>,
    plat_station: Vec<(String, usize)>,
}

/// what is written, kept in memory for the next steps
pub struct Out {
    pub stations: Vec<Value>,  // GeoJSON features, stations.geojsonl
    pub complexes: Vec<Value>, // complexes.geojsonl
    pub route_order: Map<String, Value>,
    pub report: Value,
}

pub fn run(slug: &str, inp: &Input) -> Out {
    let proj = Proj::new(inp);
    let mut feats = Feats::sort(inp, &proj);
    let rels = Rels::new(inp);
    let Grouped { st, area_to_station, plat_station } = group::group_stations(&mut feats, &rels);
    let st_tree = point_index(&st);
    let m = Model { inp, feats, rels, st, st_tree, area_to_station, plat_station };
    let sizes = size::score_stations(&m);
    let (node_station, stop_unmapped) = link::stop_positions(&m, &proj);
    let lines = link::lines_per_station(&m, &node_station);
    let cx = link::complexes(&m, &sizes);
    output::write(slug, &m, &proj, &sizes, lines, &cx, stop_unmapped)
}
