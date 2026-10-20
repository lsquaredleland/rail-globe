//! The cut's features sorted into tracks, platforms, buildings, railway land and station features (projected to metres), with an
//! index for each, and the rules that drop stations still under construction.

use super::names::CONSTRUCTION_SUFFIX;
use super::{members, Member};
use crate::geomx::{self, Index, G};
use crate::pyjson::{f, py_round};
use crate::site::{s, Feat, Input};
use geo::Point;
use regex::Regex;
use rustc_hash::FxHashSet;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

/// local equirectangular projection to metres, centred on the mean latitude of the point features
pub(super) struct Proj {
    kx: f64,
    ky: f64,
}

impl Proj {
    pub fn new(inp: &Input) -> Proj {
        let lats: Vec<f64> = inp.feats.iter().filter(|f| f.gtype == "Point").filter_map(|f| f.g["coordinates"][1].as_f64()).collect();
        let lat0 = if lats.is_empty() { 40.0 } else { lats.iter().sum::<f64>() / lats.len() as f64 };
        Proj { kx: 111320.0 * lat0.to_radians().cos(), ky: 110540.0 }
    }

    pub fn fwd(&self, x: f64, y: f64) -> (f64, f64) {
        (x * self.kx, y * self.ky)
    }

    /// a metric point back to a [lon, lat] pair, rounded as the files are written
    pub fn unproj(&self, p: &Point<f64>) -> Value {
        json!([f(py_round(p.x() / self.kx, 5)), f(py_round(p.y() / self.ky, 5))])
    }
}

/// a projected feature (metres in a local equirectangular projection)
pub(super) struct It<'a> {
    pub f: &'a Feat,
    pub g: G,
    pub modes: BTreeSet<String>, // platforms
}

impl It<'_> {
    pub fn p(&self, k: &str) -> Option<&str> {
        s(&self.f.p, k)
    }
    pub fn id(&self) -> &str {
        s(&self.f.p, "osm_id").unwrap_or("")
    }
}

pub(super) struct Feats<'a> {
    pub tracks: Vec<It<'a>>,
    pub platforms: Vec<It<'a>>,
    pub buildings: Vec<It<'a>>,
    pub lands: Vec<It<'a>>,
    pub station_feats: Vec<It<'a>>,
    pub track_tree: Index,
    pub land_tree: Index,
    pub bld_tree: Index,
}

pub(super) fn mode_key(k: &str) -> Option<&'static str> {
    Some(match k {
        "subway" => "subway",
        "light_rail" => "light_rail",
        "train" => "train",
        "monorail" => "monorail",
        "funicular" => "funicular",
        _ => return None,
    })
}

/// the mode of a classified track's `type`
pub(super) fn track_mode(t: &str) -> &'static str {
    match t {
        "metro" => "subway",
        "light_rail" => "light_rail",
        "tram" => "tram",
        "monorail" => "monorail",
        "funicular" => "funicular",
        "other" => "other",
        _ => "train",
    }
}

impl<'a> Feats<'a> {
    /// sort the features into the lists, project them, build the indexes; stations under construction are left out
    pub fn sort(inp: &'a Input, proj: &Proj) -> Feats<'a> {
        let (mut tracks, mut platforms, mut buildings, mut lands, mut station_feats): (Vec<It>, Vec<It>, Vec<It>, Vec<It>, Vec<It>) =
            Default::default();
        // nodes and ways that a route relation lists: a station among them is in service
        let route_members: FxHashSet<String> = inp
            .rels
            .iter()
            .filter(|r| r["kind"] == "route")
            .flat_map(members)
            .filter_map(|Member { kind, id, .. }| Some(format!("{}/{}", kind?, id?)))
            .collect();
        for fe in &inp.feats {
            let Some(g) = geomx::from_geojson(&fe.g, |x, y| proj.fwd(x, y)) else { continue };
            let g = geomx::make_valid(g);
            if geomx::is_empty(&g) {
                continue;
            }
            let it = It { f: fe, g, modes: BTreeSet::new() };
            let (rw, pt) = (it.p("railway"), it.p("public_transport"));
            let ty = it.p("type").filter(|t| !t.is_empty());
            if ty.is_some() && fe.gtype == "LineString" && ty != Some("yard") && rw != Some("proposed") {
                tracks.push(it);
            } else if rw == Some("platform") || pt == Some("platform") {
                platforms.push(it);
            } else {
                // a way can be a station AND a building (Boston South Station): it is both, so test the station tags first and keep the
                // feature in `buildings` too (its footprint feeds the size score and the complex grouping)
                let is_station = matches!(rw, Some("station" | "halt" | "tram_stop")) || pt == Some("station");
                let is_building = it.p("building") == Some("train_station");
                let is_land = it.p("landuse") == Some("railway");
                // landuse=railway is a footprint too (Varanasi Junction is a station and railway land): the feature goes to each list it belongs to
                if is_land && (is_station || !is_building) {
                    lands.push(It { f: it.f, g: it.g.clone(), modes: BTreeSet::new() });
                }
                if is_building {
                    buildings.push(It { f: it.f, g: it.g.clone(), modes: BTreeSet::new() });
                }
                if is_station && rw != Some("construction") && !con_name(&it.f.p) {
                    if !con_tags(&it.f.p, route_members.contains(it.id())) {
                        station_feats.push(it);
                    }
                }
            }
        }
        let track_tree = Index::new(tracks.iter().map(|t| geomx::bbox(&t.g)));
        let land_tree = Index::new(lands.iter().map(|t| geomx::bbox(&t.g)));
        let bld_tree = Index::new(buildings.iter().map(|t| geomx::bbox(&t.g)));
        Feats { tracks, platforms, buildings, lands, station_feats, track_tree, land_tree, bld_tree }
    }

    /// the mode of the nearest track within `r` metres of `g`
    pub fn nearest_track_mode(&self, g: &G, r: f64) -> Option<&'static str> {
        let mut best: Option<(usize, f64)> = None;
        for i in self.track_tree.near(g, r) {
            let d = geomx::dist(&self.tracks[i].g, g);
            if best.map_or(true, |(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        let (i, d) = best?;
        if d > r {
            return None;
        }
        Some(track_mode(self.tracks[i].p("type").unwrap_or("")))
    }

    /// (modes, whether the tags state them): without a mode tag the mode comes from the nearest track, a guess that platforms may correct
    pub fn feature_modes(&self, it: &It) -> (BTreeSet<String>, bool) {
        let mut ms = BTreeSet::new();
        if let Some(m) = it.p("station").and_then(mode_key) {
            ms.insert(m.to_string());
        }
        if it.p("railway") == Some("tram_stop") {
            ms.insert("tram".to_string());
        }
        for k in ["subway", "light_rail", "train"] {
            if it.p(k) == Some("yes") {
                ms.insert(k.to_string());
            }
        }
        let stated = !ms.is_empty();
        if !stated {
            ms.insert(self.nearest_track_mode(&it.g, 150.0).unwrap_or("train").to_string());
        }
        (ms, stated)
    }
}

/// A name that says the station is not open: "(在建)", "（建设中）", "(under construction)", "(en construcción)".
fn con_name(p: &Map<String, Value>) -> bool {
    thread_local! {
        static RE: Regex = Regex::new(CONSTRUCTION_SUFFIX).unwrap();
    }
    s(p, "name").map_or(false, |n| RE.with(|r| r.is_match(n.trim())))
}

/// A station tagged as one being built: `construction=yes|station`, `construction:railway` or `construction:public_transport`, and none
/// of the evidence that it is open: a `railway=station|halt|tram_stop` tag of its own (a station that carries the tag of a future
/// extension, Crumpsall, Arcueil-Cachan, or a leftover `construction=yes`, Bönningstedt, is open), membership of a service route
/// (`in_route`), or for a bare `construction=yes` a stop area. A platform beside it proves nothing: the platforms of planned stations
/// are mapped without a lifecycle prefix. A feature with only `public_transport=
/// station` (or `railway=site`) and `construction:railway=station` is the lifecycle form of a station being built.
pub(super) fn con_tags(p: &Map<String, Value>, in_route: bool) -> bool {
    let strong = s(p, "construction") == Some("station")
        || s(p, "construction:railway").map_or(false, |v| !v.is_empty())
        || s(p, "construction:public_transport").map_or(false, |v| !v.is_empty());
    let weak = s(p, "construction") == Some("yes");
    let open_tag = matches!(s(p, "railway"), Some("station" | "halt" | "tram_stop"));
    // a stop area is no proof of service (planned Bangkok stations have one), but it does clear a bare construction=yes
    let has_area = s(p, "_stop_area").map_or(false, |v| !v.is_empty());
    (strong || weak) && !open_tag && !in_route && (strong || !has_area)
}

/// either reason, for a feature that has no route or platform to check
#[cfg(test)]
pub(super) fn under_construction(p: &Map<String, Value>) -> bool {
    con_name(p) || con_tags(p, false)
}
