//! Step 3 of `run`: the station and complex layers. Which rail types and speed bands serve each station (so hiding a type or a speed band hides
//! its stations), stations that are really still being built, and the airport and logo of each.

use super::airports::Airports;
use super::props::{ComplexProps, StationProps, ToMap};
use super::track::{Scan, SB_NONE};
use super::util::{feat, rank};
use super::{Logos, Lookup};
use crate::geomx::{self, Index};
use geo::{Geometry, Point};
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{json, Value};
use std::collections::BTreeSet;

const NEAR: f64 = 0.0007; // about 70 m

pub(super) struct StationRows {
    pub stations: String,
    pub complexes: String,
    /// the airport of each station, in station order (the side files carry it too)
    pub airport_of: Vec<Option<String>>,
}

/// the rail types a station's modes call for (a tram stop is served by light rail track, not by a metro line beside it)
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

fn station_pt(st: &Value) -> Point<f64> {
    let c = &st["geometry"]["coordinates"];
    Point::new(c[0].as_f64().unwrap_or(0.0), c[1].as_f64().unwrap_or(0.0))
}

/// the speed bands of a station's track as `,0,3,` (see StationProps `sb`)
fn sb_str(v: &BTreeSet<u8>) -> String {
    format!(",{},", v.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(","))
}

/// A station mapped as open but with no service, no open passenger track beside it and construction track beside it is still being
/// built (OSM often tags the station before the line opens): it becomes a construction station here, so the map, the card and the
/// side files agree. A complex whose stations are all being built is one too.
pub(super) fn demote_construction(slug: &str, sc: &Scan, stations: &mut [Value], complexes: &mut [Value]) {
    let pax_tree = Index::new(sc.pax.iter().map(|(g, _)| geomx::bbox(g)));
    let con_tree = Index::new(sc.con_pax.iter().map(|(g, _)| geomx::bbox(g)));
    let mut cx_open: FxHashMap<String, bool> = FxHashMap::default(); // complex -> has a station that is open
    let mut demoted = 0;
    for stv in stations.iter_mut() {
        let p = &stv["properties"];
        if p["construction"].as_bool() != Some(true) && p["line_counts"].as_object().is_none_or(|o| o.is_empty()) {
            let allowed: FxHashSet<&str> =
                p["modes"].as_array().into_iter().flatten().filter_map(Value::as_str).flat_map(|m| mode_types(m).iter().copied()).collect();
            let pg = Geometry::Point(station_pt(stv));
            let near = |tree: &Index, rows: &[(geomx::G, String)]| -> BTreeSet<String> {
                tree.near(&pg, NEAR)
                    .into_iter()
                    .filter(|&i| allowed.contains(rows[i].1.as_str()) && geomx::dist(&rows[i].0, &pg) <= NEAR)
                    .map(|i| rows[i].1.clone())
                    .collect()
            };
            let con_ty = if near(&pax_tree, &sc.pax).is_empty() { near(&con_tree, &sc.con_pax) } else { BTreeSet::new() };
            if !con_ty.is_empty() {
                let pm = stv["properties"].as_object_mut().unwrap();
                pm.insert("construction".into(), json!(true));
                pm.insert("rail_types".into(), json!(con_ty));
                demoted += 1;
            }
        }
        let open = stv["properties"]["construction"].as_bool() != Some(true);
        *cx_open.entry(stv["properties"]["complex"].as_str().unwrap_or("").to_string()).or_insert(false) |= open;
    }
    for c in complexes.iter_mut() {
        if cx_open.get(c["properties"]["id"].as_str().unwrap_or("")) == Some(&false) {
            c["properties"]["construction"] = json!(true);
        }
    }
    eprintln!("{slug}: {demoted} stations with only construction track beside them shown as under construction");
}

/// The `stations` and `complexes` layer rows. Run `demote_construction` first.
pub(super) fn rows(sc: &Scan, stations: &[Value], complexes: &[Value], airports: &Airports, logos: &Logos, lk: &Lookup) -> StationRows {
    // rail types serving each station (its lines, and passenger track beside it)
    let pax_tree = Index::new(sc.pax.iter().map(|(g, _)| geomx::bbox(g)));
    let frt_tree = Index::new(sc.frt.iter().map(|(g, _)| geomx::bbox(g)));
    let con_tree = Index::new(sc.con_pax.iter().map(|(g, _)| geomx::bbox(g)));
    let mut cx_types: FxHashMap<String, BTreeSet<String>> = FxHashMap::default();
    let mut cx_sb: FxHashMap<String, BTreeSet<u8>> = FxHashMap::default();
    let (mut st_rows, mut cx_rows) = (String::new(), String::new());
    let mut airport_of: Vec<Option<String>> = Vec::with_capacity(stations.len());
    for stv in stations.iter() {
        let p = &stv["properties"];
        let mut sbs: BTreeSet<u8> = BTreeSet::new(); // speed bands of the track serving the station
        let ty: BTreeSet<String> = if p["construction"].as_bool() == Some(true) {
            let allowed: FxHashSet<&str> = p["rail_types"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            let pg = Geometry::Point(station_pt(stv));
            for i in con_tree.near(&pg, NEAR) {
                if allowed.contains(sc.con_pax[i].1.as_str()) && geomx::dist(&sc.con_pax[i].0, &pg) <= NEAR {
                    sbs.insert(sc.con_sb[i]);
                }
            }
            p["rail_types"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect()
        } else {
            let mut ty: BTreeSet<String> = p["line_counts"].as_object().into_iter().flatten().map(|(k, _)| k.clone()).collect();
            let allowed: FxHashSet<&str> =
                p["modes"].as_array().into_iter().flatten().filter_map(Value::as_str).flat_map(|m| mode_types(m).iter().copied()).collect();
            let pg = Geometry::Point(station_pt(stv));
            for i in pax_tree.near(&pg, NEAR) {
                if allowed.contains(sc.pax[i].1.as_str()) && geomx::dist(&sc.pax[i].0, &pg) <= NEAR {
                    ty.insert(sc.pax[i].1.clone());
                    sbs.insert(sc.pax_sb[i]);
                }
            }
            ty
        };
        cx_types.entry(p["complex"].as_str().unwrap_or("").to_string()).or_default().extend(ty.iter().cloned());
        // no track found beside it: counts as unmapped
        if sbs.is_empty() {
            sbs.insert(SB_NONE);
        }
        cx_sb.entry(p["complex"].as_str().unwrap_or("").to_string()).or_default().extend(sbs.iter().copied());
        let mut lc: Vec<(&String, &Value)> = p["line_counts"].as_object().into_iter().flatten().collect();
        lc.sort_by_key(|(k, _)| rank(k));
        let lct = lc.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(",");
        let hs_lines = p["line_counts"]["high_speed"].as_i64().unwrap_or(0) > 0;
        let tier = p["tier"].as_i64().unwrap_or(3);
        let ap = airports.of_station(stv);
        let construction = p["construction"].as_bool() == Some(true);
        let freight = !construction && ty.is_empty() && lct.is_empty() && {
            // no service and no passenger track beside it: with freight or yard track beside it, a freight station (the tooltip says so)
            let pg = Geometry::Point(station_pt(stv));
            frt_tree.near(&pg, NEAR).into_iter().any(|i| geomx::dist(&sc.frt[i].0, &pg) <= NEAR)
        };
        let sp = StationProps {
            id: p["id"].clone(),
            cx: p["complex"].clone(),
            n: p["name"].as_str().unwrap_or("").to_string(),
            ne: p["name_en"].as_str().unwrap_or("").to_string(),
            tier,
            term: p["terminus"].as_bool().unwrap_or(false),
            hsr: p["hsr"].as_bool().unwrap_or(false) || hs_lines,
            mp: p["mainline_platforms"].clone(),
            pc: p["platform_count"].clone(),
            lc: p["line_count"].clone(),
            sb: sb_str(&sbs),
            ty: if ty.is_empty() { String::new() } else { format!(",{},", ty.iter().cloned().collect::<Vec<_>>().join(",")) },
            modes: p["modes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(","),
            lg: logos.for_station(p, lk),
            ap: ap.clone(),
            con: construction,
            fr: freight,
            lct,
        };
        airport_of.push(ap);
        let minz = match tier {
            1 => 5,
            2 => 7,
            _ => 9,
        };
        feat(&mut st_rows, &sp.to_map(), &stv["geometry"].to_string(), Some(minz), None);
    }
    for c in complexes.iter() {
        let p = &c["properties"];
        let tier = p["tier"].as_i64().unwrap_or(3);
        let ty = cx_types.get(p["id"].as_str().unwrap_or("")).filter(|t| !t.is_empty());
        let cp = ComplexProps {
            id: p["id"].clone(),
            n: p["name"].as_str().unwrap_or("").to_string(),
            ne: p["name_en"].as_str().unwrap_or("").to_string(),
            tier,
            term: p["terminus"].as_bool().unwrap_or(false),
            ns: p["station_count"].clone(),
            con: p["construction"].as_bool() == Some(true),
            sb: format!(
                ",{},",
                cx_sb.get(p["id"].as_str().unwrap_or("")).map_or("9".to_string(), |v| v.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(","))
            ),
            ty: ty.map_or(String::new(), |t| format!(",{},", t.iter().cloned().collect::<Vec<_>>().join(","))),
        };
        let minz = match tier {
            1 => 5,
            2 => 8,
            _ => 10,
        };
        feat(&mut cx_rows, &cp.to_map(), &c["geometry"].to_string(), Some(minz), None);
    }
    StationRows { stations: st_rows, complexes: cx_rows, airport_of }
}
