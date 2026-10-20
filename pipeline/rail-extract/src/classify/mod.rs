//! Rail type classification (docs/01-features.md, section 2), run at the end of every cut:
//! the cut already holds the features, relations and tags in memory, so no JSON is written and read back in between.
//!
//! Writes data/<slug>/features.typed.geojsonl (features.geojsonl with `construction_type`, `type`, `type_source`, `own_type`
//! added to track), lines.json (one record per line: route_master, else route) and classify_report.json.
//! Per-region rules stay in pipeline/config/<slug>.json.

use crate::geomx::UnionFind;
use crate::out::{js, py_str, J};
use crate::region::Region;
use crate::rules::{List, Tags};
use regex::Regex;
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::Value;
use std::path::Path;

mod config;
mod decide;
mod evidence;
mod exemptions;
mod own_tag;
mod report;
mod routes;
mod smoothing;
mod tags;

pub use self::config::Cfg;
use self::config::*;
use self::decide::*;
use self::evidence::*;
use self::exemptions::*;
use self::own_tag::*;
use self::report::*;
use self::routes::*;
use self::smoothing::*;
use self::tags::*;
pub use self::tags::{speed_kmh, FeatProps, Get, Owned};

/// a kept relation, as the side table has it
pub struct RelRow {
    pub id: String,
    pub route: bool,
    pub master: bool,
    pub tags: Owned,
    pub rel_members: Vec<i64>, // member relation ids (route masters list their routes)
}

/// a feature line of features.geojsonl that is a candidate track (a way written as a LineString)
pub struct TrackIn<'a> {
    pub start: usize, // byte range of the whole feature line in the features buffer (an exemption can drop it)
    pub end: usize,
    pub pos: usize, // byte position in the features buffer where its properties object closes
    pub props: FeatProps<'a>,
    pub routes: Vec<String>,
    pub first: (i32, i32),   // first point (lat, lon), for the proposed-area test
    pub ll: Vec<(i32, i32)>, // every point (lat, lon, 1e-7 degrees; lat = i32::MIN when the node is missing), for the passenger-station test
}

pub struct Out {
    /// exemptions that matched fewer features than their expect_min, as warning text
    pub stale: Vec<String>,
    pub typed: Vec<u8>,
    pub lines: Vec<u8>,
    pub report: Vec<u8>,
    pub n_lines: usize,
}

/// Types every track and line of the region: lines.json, features.typed.geojsonl and the report.
pub fn classify(slug: &str, cfg: &Cfg, rows: &[RelRow], feats: &[u8], tracks: &[TrackIn], pax: &[(i32, i32)]) -> Out {
    let con_mode = construction_modes(tracks, rows);
    let norm: Vec<TrackIn>;
    let tracks: &[TrackIn] = if con_mode.iter().any(Option::is_some) {
        norm = with_construction_tags(tracks, &con_mode);
        &norm
    } else {
        tracks
    };

    // routes -> lines -> types
    let routes: Vec<&RelRow> = rows.iter().filter(|r| r.route && !route_under_construction(&r.tags)).collect();
    let hs_routes = high_speed_routes(tracks, cfg);
    let route_pos: FxHashMap<&str, usize> = routes.iter().enumerate().map(|(i, r)| (r.id.as_str(), i)).collect();
    let mut route_master: FxHashMap<String, &str> = FxHashMap::default();
    for m in rows.iter().filter(|r| r.master) {
        for id in &m.rel_members {
            route_master.insert(format!("relation/{id}"), m.id.as_str());
        }
    }
    let mut ex_stats: Vec<ExStats> = cfg.exemptions.iter().map(|_| ExStats::default()).collect();
    let rtype: Vec<(String, &'static str)> = routes.iter().map(|r| route_type(&r.tags, cfg, hs_routes.get(&r.id).copied().unwrap_or(HsTrack::No))).collect();
    let lines = build_lines(cfg, rows, &routes, &route_master, &rtype, &mut ex_stats);
    let route_lines = RouteLines { route_pos: &route_pos, route_master: &route_master, line_type: &lines.line_type };

    // tracks: type, evidence, neighbours, exemptions
    let mut decided = decide_tracks(tracks, cfg, pax, &route_lines);
    if let Some(mode) = cfg.needs_evidence {
        needs_evidence_pass(tracks, cfg, pax, mode, &mut decided);
    }
    let mut at_end: FxHashMap<(i32, i32), Vec<usize>> = FxHashMap::default();
    for (i, tr) in tracks.iter().enumerate() {
        if decided[i].is_some() && tr.props.g("railway") == Some("rail") {
            for e in [tr.ll.first(), tr.ll.last()].into_iter().flatten() {
                at_end.entry(*e).or_default().push(i);
            }
        }
    }
    smooth_short_pieces(tracks, &at_end, &mut decided);
    smooth_chains(tracks, &at_end, &mut decided);
    let dropped = apply_exemptions(tracks, cfg, &mut decided, &mut ex_stats);

    let (typed, counts, sources) = write_typed(feats, tracks, &mut decided, &dropped, &con_mode);
    let (stale, report) = report_json(slug, cfg, &ex_stats, &lines, &counts, &sources);
    Out { stale, typed, lines: lines.out, report, n_lines: lines.ids.len() }
}

/// the feature text up to the end of its properties with `railway=<mode>` and `construction=<value>` rewritten as `railway=construction`, `construction=<mode>`
fn as_construction(text: &[u8], mode: &str) -> Vec<u8> {
    let s = String::from_utf8_lossy(text).into_owned();
    let swap = |s: String, key: &str, new: &str| -> Option<String> {
        let pat = format!("\"{key}\":\"");
        let a = s.rfind(&pat)? + pat.len();
        let b = a + s[a..].find('"')?;
        Some(format!("{}{}{}", &s[..a], new, &s[b..]))
    };
    let Some(s) = swap(s.clone(), "railway", "construction") else { return s.into_bytes() };
    match swap(s.clone(), "construction", mode) {
        Some(s) => s.into_bytes(),
        None => format!("{s},\"construction\":\"{mode}\"").into_bytes(), // no construction key in the text: keep the mode
    }
}

#[cfg(test)]
mod tests;
