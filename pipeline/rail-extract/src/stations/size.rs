//! Size inputs, score, tier and major-station flag of each station (docs/04-station-model.md). Physical size only, never line counts.

use super::features::{track_mode, It};
use super::names::norm_name;
use super::{Model, Station};
use crate::geomx::{self, Index, G};
use crate::pyjson::py_round;
use geo::{Geometry, MultiPolygon, Point};
use regex::Regex;
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use serde_json::Value;
use std::collections::BTreeSet;

const LAND_DISC: f64 = 500.0; // railway land counts only near the station (landuse=railway polygons span whole corridors)
const HS_NEAR: f64 = 80.0; // "next to high-speed track"
const HS_MAJOR_PLATFORMS: i64 = 8;
const TERMINUS_ZONE: f64 = 120.0; // around the station's mainline platforms
// size score weights and tiers (tuned on the answer lists)
const W_PLATFORM: f64 = 3.0;
const W_TRACK: f64 = 0.5;
const W_TRACK_CAP: f64 = 40.0;
const W_LAND: f64 = 3.0;
const W_BUILDING: f64 = 4.0;
const W_TERMINUS: f64 = 10.0;

pub(super) struct Size {
    pub mainline_platforms: i64,
    pub platform_count: i64,
    pub land_ha: f64,
    pub building_ha: f64,
    pub mainline_track_ways: i64,
    pub dead_ends: i64,
    pub terminus: bool,
    pub score: f64,
    pub tier: i64,
    pub hsr: bool,
    pub major: Option<&'static str>,
}

/// mainline track, its dead ends and high-speed track, with an index for each
struct TrackIndex {
    main_tracks: Vec<usize>, // indices into tracks
    mt_tree: Index,
    dead_pts: Vec<Point<f64>>,
    dead_tree: Index,
    hs: Vec<usize>,
    hs_tree: Index,
}

impl TrackIndex {
    /// track dead ends (terminus flag): ends used by exactly one mainline way
    fn new(tracks: &[It]) -> TrackIndex {
        let main_tracks: Vec<usize> = (0..tracks.len())
            .filter(|&i| tracks[i].p("railway") == Some("rail") && matches!(tracks[i].p("type"), Some("high_speed" | "intercity" | "regional")))
            .collect();
        let line_of = |i: usize| -> Option<&geo::LineString<f64>> { if let Geometry::LineString(l) = &tracks[i].g { Some(l) } else { None } };
        let key = |c: &geo::Coord<f64>| (c.x.round_ties_even() as i64, c.y.round_ties_even() as i64);
        let mut ends: FxHashMap<(i64, i64), u32> = FxHashMap::default();
        for &i in &main_tracks {
            if let Some(l) = line_of(i).filter(|l| l.0.len() >= 2) {
                *ends.entry(key(&l.0[0])).or_default() += 1;
                *ends.entry(key(l.0.last().unwrap())).or_default() += 1;
            }
        }
        let mut dead_pts: Vec<Point<f64>> = Vec::new();
        for &i in &main_tracks {
            if let Some(l) = line_of(i).filter(|l| !l.0.is_empty()) {
                for c in [l.0[0], *l.0.last().unwrap()] {
                    if ends.get(&key(&c)) == Some(&1) {
                        dead_pts.push(Point::from(c));
                    }
                }
            }
        }
        let dead_tree = Index::new(dead_pts.iter().map(|p| geomx::bbox(&Geometry::Point(*p))));
        let mt_tree = Index::new(main_tracks.iter().map(|&i| geomx::bbox(&tracks[i].g)));
        // high-speed track by its OWN tags (a classic line that TGVs merely use does not count)
        let hs: Vec<usize> = (0..tracks.len()).filter(|&i| tracks[i].p("own_type") == Some("high_speed") && tracks[i].p("railway") != Some("construction")).collect();
        let hs_tree = Index::new(hs.iter().map(|&i| geomx::bbox(&tracks[i].g)));
        TrackIndex { main_tracks, mt_tree, dead_pts, dead_tree, hs, hs_tree }
    }
}

/// the major station rules of the config (docs/00-overview.md, 2026-10-11 "Major station rules")
struct MajorRules {
    name_re: Option<Regex>,
    canonical: BTreeSet<String>,
}

impl MajorRules {
    fn new(cfg: &serde_json::Map<String, Value>) -> MajorRules {
        MajorRules {
            name_re: cfg.get("main_station_re").and_then(Value::as_str).filter(|v| !v.is_empty()).map(|p| Regex::new(p).expect("main_station_re")),
            canonical: cfg.get("main_terminals").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(norm_name).filter(|n| !n.is_empty()).collect(),
        }
    }
}

/// distinct platform numbers ("1a"/"1b" are one), plus every platform way without a ref
fn count_platforms(platforms: &[It], ix: &[usize]) -> i64 {
    let (mut toks, mut unreffed) = (BTreeSet::new(), Vec::new());
    for &k in ix {
        let t = ref_tokens(platforms[k].p("ref"));
        if t.is_empty() {
            unreffed.push(k);
        } else {
            toks.extend(t.iter().map(|t| plat_key(t)));
        }
    }
    toks.len() as i64 + unreffed.len() as i64
}

pub(super) fn ref_tokens(r: Option<&str>) -> BTreeSet<String> {
    r.unwrap_or("").split([';', ',', '/']).map(str::trim).filter(|t| !t.is_empty()).map(String::from).collect()
}

/// sort key of platform refs: numbers first, by value, then the rest
pub(super) fn ref_key(r: &str) -> (bool, u128, String) {
    let num = !r.is_empty() && r.chars().all(|c| c.is_ascii_digit());
    (!num, if num { r.parse().unwrap_or(u128::MAX) } else { 0 }, r.to_string())
}

/// "1a" and "1b" are sections of platform 1: the platform's number is its digits when only one or two letters follow them
pub(super) fn plat_key(t: &str) -> String {
    let d: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &t[d.len()..];
    if !d.is_empty() && (1..=2).contains(&rest.chars().count()) && rest.chars().all(|c| c.is_ascii_alphabetic()) { d } else { t.to_string() }
}

/// railway land and building (hectares) holding the station's point or its platforms
fn footprint_ha(m: &Model, x: &Station) -> (f64, f64) {
    let f = &m.feats;
    let mut probes = vec![x.pt];
    probes.extend(x.platforms.iter().take(60).filter_map(|&k| geomx::rep_point(&f.platforms[k].g)));
    let (mut land_ids, mut bld_ids) = (BTreeSet::new(), BTreeSet::new());
    for pr in &probes {
        for li in f.land_tree.at(pr) {
            if geomx::contains_pt(&f.lands[li].g, pr) {
                land_ids.insert(li);
            }
        }
        for bi in f.bld_tree.at(pr) {
            if geomx::contains_pt(&f.buildings[bi].g, pr) {
                bld_ids.insert(bi);
            }
        }
    }
    let land_ha = if land_ids.is_empty() {
        0.0
    } else {
        let u = geomx::union_all(land_ids.iter().map(|&i| geomx::polys(&f.lands[i].g)).collect());
        let local: MultiPolygon<f64> = geomx::intersection(&u, &geomx::circle(x.pt, LAND_DISC));
        if local.0.is_empty() { 0.0 } else { py_round(geomx::area(&Geometry::MultiPolygon(local)) / 10000.0, 2) }
    };
    let building_ha = py_round(bld_ids.iter().map(|&i| geomx::area(&f.buildings[i].g) / 10000.0).sum::<f64>(), 3);
    (land_ha, building_ha)
}

/// mainline tracks serving the station's mainline platforms `mp` (or near the point when no platforms are mapped)
fn serving_tracks<'a>(m: &'a Model, ti: &TrackIndex, x: &Station, mp: &[usize]) -> BTreeSet<&'a str> {
    let f = &m.feats;
    let (platforms, tracks) = (&f.platforms, &f.tracks);
    let mut near: BTreeSet<&str> = BTreeSet::new();
    let pg = Geometry::Point(x.pt);
    if !mp.is_empty() && x.modes.contains("train") {
        for &k in mp {
            for t in ti.mt_tree.near(&platforms[k].g, 20.0) {
                if geomx::dist(&tracks[ti.main_tracks[t]].g, &platforms[k].g) <= 20.0 {
                    near.insert(tracks[ti.main_tracks[t]].id());
                }
            }
        }
    }
    // no platform mapped, or none beside a mainline track (Nanjing South: one platform away from the tracks): look near the point
    // (only when no platform has a heavy-rail or heritage track beside it (a metro track does not count): a heritage station next to a main line stays without passenger track)
    let platform_by_track = mp.iter().any(|&k| f.track_tree.near(&platforms[k].g, 20.0).into_iter().any(|t| matches!(track_mode(tracks[t].p("type").unwrap_or("")), "train" | "other") && geomx::dist(&tracks[t].g, &platforms[k].g) <= 20.0));
    if near.is_empty() && x.modes.contains("train") && !platform_by_track {
        for t in ti.mt_tree.near(&pg, 80.0) {
            if geomx::dist(&tracks[ti.main_tracks[t]].g, &pg) <= 80.0 {
                near.insert(tracks[ti.main_tracks[t]].id());
            }
        }
    }
    near
}

/// terminus test: the number of mainline dead ends within TERMINUS_ZONE of the station's mainline platforms or point
/// (the zone is a 120 m buffer round the platforms and point)
fn dead_ends_near(m: &Model, ti: &TrackIndex, x: &Station, mp: &[usize]) -> i64 {
    if !x.modes.contains("train") {
        return 0;
    }
    let pg = Geometry::Point(x.pt);
    let zone: Vec<&G> = mp.iter().map(|&k| &m.feats.platforms[k].g).collect();
    let mut cands: BTreeSet<usize> = ti.dead_tree.near(&pg, TERMINUS_ZONE).into_iter().collect();
    for g in &zone {
        cands.extend(ti.dead_tree.near(g, TERMINUS_ZONE));
    }
    let mut dead = 0i64;
    for di in cands {
        let p = &ti.dead_pts[di];
        if geomx::dist_pt(&pg, p) < TERMINUS_ZONE || zone.iter().any(|g| geomx::dist_pt(g, p) < TERMINUS_ZONE) {
            dead += 1;
        }
    }
    dead
}

/// size inputs, score, tier and major flag of every station
pub(super) fn score_stations(m: &Model) -> Vec<Size> {
    let f = &m.feats;
    let ti = TrackIndex::new(&f.tracks);
    let rules = MajorRules::new(&m.inp.cfg);
    m.st
        .par_iter()
        .map(|x| {
            let mp: Vec<usize> = x.platforms.iter().copied().filter(|&k| f.platforms[k].modes.contains("train")).collect();
            let mainline_platforms = count_platforms(&f.platforms, &mp);
            let platform_count = count_platforms(&f.platforms, &x.platforms);
            let (land_ha, building_ha) = footprint_ha(m, x);
            let near = serving_tracks(m, &ti, x, &mp);
            let dead = dead_ends_near(m, &ti, x, &mp);
            let terminus = dead >= 2;
            let score = py_round(
                W_PLATFORM * mainline_platforms as f64 + W_TRACK * (near.len() as f64).min(W_TRACK_CAP) + W_LAND * land_ha.sqrt()
                    + W_BUILDING * building_ha.sqrt() + if terminus { W_TERMINUS } else { 0.0 },
                1,
            );
            let mut tier = if score >= 60.0 { 1 } else if score >= 25.0 { 2 } else { 3 };
            let pg = Geometry::Point(x.pt);
            let hsr = ti.hs_tree.near(&pg, HS_NEAR).into_iter().any(|t| geomx::dist(&f.tracks[ti.hs[t]].g, &pg) <= HS_NEAR);
            let names: Vec<&str> = x.feats.iter().map(|&k| f.station_feats[k].p("name").unwrap_or("")).collect();
            let mut norms: BTreeSet<String> = names.iter().map(|n| norm_name(n)).collect();
            norms.insert(x.norm.clone());
            let mut major = None;
            if x.modes.contains("train") {
                if rules.name_re.as_ref().map_or(false, |re| names.iter().any(|n| re.is_match(n))) {
                    major = Some("name_pattern");
                } else if norms.iter().any(|n| rules.canonical.contains(n)) {
                    major = Some("canonical_list");
                } else if hsr && (terminus || mainline_platforms >= HS_MAJOR_PLATFORMS) {
                    major = Some("hsr_infrastructure");
                }
            }
            // a train station with no passenger track near it (only freight, yard or no mapped track) is not a major station, whatever its
            // name says (Zhuhai West): it stays on the map at the smallest size
            let no_passenger_track = x.modes.contains("train") && near.is_empty();
            if no_passenger_track {
                major = None;
                tier = 3;
            }
            if major.is_some() {
                tier = 1;
            }
            Size { mainline_platforms, platform_count, land_ha, building_ha, mainline_track_ways: near.len() as i64, dead_ends: dead, terminus, score, tier, hsr, major }
        })
        .collect()
}
