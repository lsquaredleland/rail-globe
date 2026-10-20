//! Stations still being built (docs/00-overview.md, 2026-10-11).
//!
//! A named node is an under-construction station if it has railway=construction with construction=station|halt, or
//! construction:railway=station|halt|stop, or construction:public_transport=station|stop_position. Nodes of the same name (or one
//! name ending with the other) within 400 m are one station; a station is skipped when an open one of the same name stands within
//! 600 m. Tier from what is being built around it: hsr if high-speed construction track runs within 200 m or the name says so;
//! major if hsr with 4+ mainline construction pieces within 200 m, or the name matches main_station_re. Works in degrees.

use crate::geomx::{self, round, Index, UnionFind};
use crate::site::{s, Input};
use crate::stations::english_name;
use geo::{Geometry, Point};
use regex::Regex;
use rustc_hash::FxHashMap;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

const CLUSTER_DEG: f64 = 0.0037; // about 400 m
const OPEN_DEG: f64 = 0.0055; // about 600 m
const TRACK_DEG: f64 = 0.0015; // about 150 m
const TIER_DEG: f64 = 0.0018; // about 200 m
const MAJOR_TRACKS: i64 = 4;

fn norm(name: &str) -> String {
    let n: String = name.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    for suffix in ["站", "station"] {
        if let Some(x) = n.strip_suffix(suffix) {
            return x.to_string();
        }
    }
    n
}

fn is_con(p: &Map<String, Value>) -> bool {
    (s(p, "railway") == Some("construction") && matches!(s(p, "construction"), Some("station" | "halt")))
        || matches!(s(p, "construction:railway"), Some("station" | "halt" | "stop"))
        || matches!(s(p, "construction:public_transport"), Some("station" | "stop_position"))
}

fn mode_types(m: &str) -> &'static [&'static str] {
    match m {
        "train" => &["high_speed", "intercity", "regional"],
        "subway" => &["metro"],
        "light_rail" | "tram" => &["light_rail"],
        "monorail" => &["monorail"],
        "funicular" => &["funicular"],
        "other" => &["other"],
        _ => &[],
    }
}

fn default_type(m: &str) -> &'static str {
    match m {
        "train" => "regional",
        "subway" => "metro",
        "light_rail" | "tram" => "light_rail",
        "monorail" => "monorail",
        "funicular" => "funicular",
        _ => "other",
    }
}

fn mode_of_track(t: &str) -> &'static str {
    match t {
        "subway" => "subway",
        "light_rail" => "light_rail",
        "tram" => "tram",
        "monorail" => "monorail",
        "funicular" => "funicular",
        _ => "train",
    }
}

/// Cluster the nodes: same name, or one name ending with the other (深圳机场东 / 机场东), within 400 m.
/// Returns the points, the normalised names, and the clusters (indexes into `nodes`).
fn cluster_nodes(inp: &Input, nodes: &[usize]) -> (Vec<Point<f64>>, Vec<String>, Vec<Vec<usize>>) {
    let pt = |i: usize| -> Point<f64> {
        let c = &inp.feats[i].g["coordinates"];
        Point::new(c[0].as_f64().unwrap_or(0.0), c[1].as_f64().unwrap_or(0.0))
    };
    let pts: Vec<Point<f64>> = nodes.iter().map(|&i| pt(i)).collect();
    let nms: Vec<String> = nodes.iter().map(|&i| norm(s(&inp.feats[i].p, "name").unwrap_or(""))).collect();
    let mut uf = UnionFind::new(nodes.len());
    let node_tree = Index::new(pts.iter().map(|p| geomx::bbox(&Geometry::Point(*p))));
    for i in 0..nodes.len() {
        for j in node_tree.near(&Geometry::Point(pts[i]), CLUSTER_DEG) {
            if j <= i {
                continue;
            }
            let (a, b) = (&nms[i], &nms[j]);
            let compat = a == b || (a.chars().count().min(b.chars().count()) >= 3 && (a.ends_with(b.as_str()) || b.ends_with(a.as_str())));
            if compat && geomx::dist_pt(&Geometry::Point(pts[i]), &pts[j]) <= CLUSTER_DEG {
                uf.link(i, j);
            }
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut gix: FxHashMap<usize, usize> = FxHashMap::default();
    for i in 0..nodes.len() {
        let r = uf.find(i);
        let k = *gix.entry(r).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[k].push(i);
    }
    (pts, nms, groups)
}

/// Appends the construction stations to `stations` and `complexes` (the station model's output); returns a one-line summary.
pub fn run(slug: &str, inp: &Input, stations: &mut Vec<Value>, complexes: &mut Vec<Value>) -> String {
    let main_re = inp.cfg.get("main_station_re").and_then(Value::as_str).filter(|v| !v.is_empty()).map(|p| Regex::new(p).expect("main_station_re"));
    let hs_name = Regex::new(r"(?i)高铁|高速铁路|high.speed").unwrap();
    let nodes: Vec<usize> = (0..inp.feats.len())
        .filter(|&i| inp.feats[i].gtype == "Point" && is_con(&inp.feats[i].p) && s(&inp.feats[i].p, "name").is_some_and(|n| !n.is_empty()))
        .collect();
    let con: Vec<usize> = (0..inp.feats.len()).filter(|&i| inp.feats[i].gtype == "LineString" && s(&inp.feats[i].p, "railway") == Some("construction")).collect();
    let con_g: Vec<geomx::G> = con.iter().filter_map(|&i| geomx::from_geojson(&inp.feats[i].g, geomx::id)).collect();
    let track_tree = Index::new(con_g.iter().map(geomx::bbox));
    let tp = |k: usize, key: &str| s(&inp.feats[con[k]].p, key);
    let open_pts: Vec<(Point<f64>, String)> = stations
        .iter()
        .map(|r| {
            let c = &r["geometry"]["coordinates"];
            (Point::new(c[0].as_f64().unwrap_or(0.0), c[1].as_f64().unwrap_or(0.0)), norm(r["properties"]["name"].as_str().unwrap_or("")))
        })
        .collect();
    let open_tree = Index::new(open_pts.iter().map(|(p, _)| geomx::bbox(&Geometry::Point(*p))));

    let (pts, nms, groups) = cluster_nodes(inp, &nodes);

    let (mut skipped, mut added) = (0, 0);
    let mut tiers = [0usize; 4];
    for g in &groups {
        // the longest normalised name of the cluster (the first such)
        let nm = g.iter().map(|&i| &nms[i]).fold(None, |a: Option<&String>, n| if a.is_none_or(|b| n.chars().count() > b.chars().count()) { Some(n) } else { a }).unwrap();
        let (cx, cy) = (g.iter().map(|&i| pts[i].x()).sum::<f64>() / g.len() as f64, g.iter().map(|&i| pts[i].y()).sum::<f64>() / g.len() as f64);
        let c = Point::new(cx, cy);
        let cg = Geometry::Point(c);
        if open_tree.near(&cg, OPEN_DEG).into_iter().any(|i| &open_pts[i].1 == nm && geomx::dist_pt(&cg, &open_pts[i].0) <= OPEN_DEG) {
            skipped += 1;
            continue;
        }
        let mut props = Map::new();
        for &i in g {
            for (k, v) in &inp.feats[nodes[i]].p {
                props.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        let mut modes: BTreeSet<String> = BTreeSet::new();
        let mut hsr = false;
        for &i in g {
            for k in ["subway", "light_rail", "train"] {
                if s(&inp.feats[nodes[i]].p, k) == Some("yes") {
                    modes.insert(k.to_string());
                }
            }
        }
        if modes.is_empty() {
            let mut best: Option<(f64, usize)> = None;
            for k in track_tree.near(&cg, TRACK_DEG) {
                let d = geomx::dist(&con_g[k], &cg);
                if d <= TRACK_DEG && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, k));
                }
            }
            if let Some((_, k)) = best {
                modes.insert(mode_of_track(tp(k, "construction_type").unwrap_or("rail")).to_string());
                hsr = tp(k, "type") == Some("high_speed");
            }
        }
        let label = format!("{} {}", s(&props, "name").unwrap_or(""), english_name(&props).unwrap_or_default());
        if hs_name.is_match(&label) {
            hsr = true;
            modes = BTreeSet::from(["train".to_string()]);
        }
        if modes.is_empty() {
            modes.insert("train".to_string());
        }
        let mainline = modes.contains("train");
        let (mut hs_ways, mut main_ways) = (0i64, 0i64);
        let near: Vec<usize> = track_tree.near(&cg, TIER_DEG).into_iter().filter(|&k| geomx::dist(&con_g[k], &cg) <= TIER_DEG).collect();
        if mainline {
            for &k in &near {
                let t = tp(k, "type");
                hs_ways += (t == Some("high_speed")) as i64;
                main_ways += matches!(t, Some("high_speed" | "intercity" | "regional")) as i64;
            }
        }
        hsr = hsr || (mainline && (hs_ways > 0 || hs_name.is_match(&label)));
        let major = mainline && ((hsr && main_ways >= MAJOR_TRACKS) || main_re.as_ref().is_some_and(|re| re.is_match(s(&props, "name").unwrap_or(""))));
        let tier: i64 = if major { 1 } else if mainline && hsr { 2 } else { 3 };
        // rail types this station belongs to, so hiding a type hides it: nearby construction track of a matching kind, else a default
        let allowed: BTreeSet<&str> = modes.iter().flat_map(|m| mode_types(m).iter().copied()).collect();
        let mut rail_types: BTreeSet<String> = near.iter().filter_map(|&k| tp(k, "type")).filter(|t| allowed.contains(t)).map(String::from).collect();
        if rail_types.is_empty() {
            rail_types = if hsr && mainline { BTreeSet::from(["high_speed".to_string()]) } else { modes.iter().map(|m| default_type(m).to_string()).collect() };
        } else if hsr && mainline {
            rail_types = BTreeSet::from(["high_speed".to_string()]);
        }
        let ids: Vec<&str> = g.iter().map(|&i| s(&inp.feats[nodes[i]].p, "osm_id").unwrap_or("")).collect();
        let sid = ids.iter().min().copied().unwrap_or("").to_string();
        let name = props.get("name").cloned().unwrap_or(Value::Null);
        let name_en = english_name(&props);
        let wikidata = props.get("wikidata").filter(|v| v.as_str().is_some_and(|x| !x.is_empty())).map(|v| json!([v])).unwrap_or(Value::Null);
        let geom = json!({"type": "Point", "coordinates": [Value::from(round(cx, 5)), Value::from(round(cy, 5))]});
        let pr = json!({
            "id": sid, "complex": sid, "name": name, "name_en": name_en, "modes": modes, "tier": tier,
            "score": 0, "terminus": false, "mainline_platforms": 0, "platform_count": 0, "land_ha": 0, "building_ha": 0,
            "mainline_track_ways": 0, "dead_ends": 0, "hsr": hsr, "major": major, "platform_refs": [], "platform_refs_other": [],
            "line_count": 0, "line_counts": {}, "line_ids": [], "wikidata": wikidata,
            "grouping": "construction", "rail_types": rail_types, "source_ids": ids, "construction": true,
        });
        let cp = json!({"id": sid, "name": name, "name_en": name_en, "tier": tier, "terminus": false, "station_count": 1,
            "modes": modes, "platform_count": 0, "line_count": 0, "grouping": "construction", "station_ids": [sid], "construction": true});
        stations.push(json!({"type": "Feature", "properties": pr, "geometry": geom}));
        complexes.push(json!({"type": "Feature", "properties": cp, "geometry": geom}));
        tiers[tier as usize] += 1;
        added += 1;
    }
    format!("{slug}: {} construction station nodes -> {added} stations added, skipped {{'already open': {skipped}}}, tiers {{1: {}, 2: {}, 3: {}}}", nodes.len(), tiers[1], tiers[2], tiers[3])
}
