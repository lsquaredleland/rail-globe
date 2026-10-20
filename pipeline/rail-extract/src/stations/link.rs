//! What is tied to the final stations: stop positions, the lines through each station, and the complexes.

use super::names::{english_name, norm_name};
use super::size::Size;
use super::{groups, max_radius, members, station_id, station_wikidata, Model, GROUP_RADIUS};
use crate::geomx::{self, UnionFind};

use geo::{Geometry, Point};
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// (osm type, id) of a stop position, platform, station feature or other stop-area member -> station index
pub(super) type NodeStation = FxHashMap<(String, i64), usize>;

/// stop positions -> stations: through the station's own features, its stop area and its platforms, else the nearest station within
/// its radius (a same-named one wins). Returns the lookup and the number of stop positions that stayed unmapped.
pub(super) fn stop_positions(m: &Model, proj: &super::features::Proj) -> (NodeStation, usize) {
    let (st, station_feats) = (&m.st, &m.feats.station_feats);
    let mut node_station: NodeStation = FxHashMap::default();
    for (si, x) in st.iter().enumerate() {
        for &k in &x.feats {
            let (t, n) = station_feats[k].id().split_once('/').unwrap_or(("", "0"));
            node_station.insert((t.to_string(), n.parse().unwrap_or(0)), si);
        }
    }
    for (key, sa) in &m.rels.member_area {
        if let Some(&si) = m.area_to_station.get(sa) {
            node_station.entry(key.clone()).or_insert(si);
        }
    }
    for (pid, si) in &m.plat_station {
        let (t, n) = pid.split_once('/').unwrap_or(("", "0"));
        node_station.entry((t.to_string(), n.parse().unwrap_or(0))).or_insert(*si);
    }
    let mut unmapped = 0usize;
    for sp in &m.inp.stops {
        let id: i64 = sp["osm_id"].as_str().and_then(|v| v.split_once('/')).and_then(|(_, n)| n.parse().ok()).unwrap_or(0);
        let key = ("node".to_string(), id);
        if node_station.contains_key(&key) {
            continue;
        }
        let (x, y) = proj.fwd(sp["lon"].as_f64().unwrap_or(0.0), sp["lat"].as_f64().unwrap_or(0.0));
        let pg = Geometry::Point(Point::new(x, y));
        let pm: BTreeSet<&str> = ["subway", "light_rail", "train"].into_iter().filter(|k| sp["modes"].get(*k).and_then(Value::as_str) == Some("yes")).collect();
        let nn = norm_name(sp["name"].as_str().unwrap_or(""));
        let mut best: Option<(usize, f64)> = None;
        for i in m.st_tree.near(&pg, 300.0) {
            if !pm.is_empty() && !pm.iter().any(|md| st[i].modes.contains(*md)) {
                continue;
            }
            let d = geomx::dist_pt(&pg, &st[i].pt);
            if d > max_radius(&st[i].modes) {
                continue;
            }
            let score = d - if !nn.is_empty() && nn == st[i].norm { 500.0 } else { 0.0 };
            if best.is_none_or(|(_, b)| score < b) {
                best = Some((i, score));
            }
        }
        match best {
            Some((i, _)) => {
                node_station.insert(key, i);
            }
            None => unmapped += 1,
        }
    }
    (node_station, unmapped)
}

/// step 4: lines per station, the order of stations along each route, and the named infrastructure lines through each station
pub(super) struct Lines {
    pub st_lines: Vec<BTreeSet<String>>,
    pub st_rail: Vec<Map<String, Value>>,
    pub route_order: Map<String, Value>,
    pub routes_total: usize,
    pub routes_unmapped: usize, // routes with a stop that maps to no station
}

pub(super) fn lines_per_station(m: &Model, node_station: &NodeStation) -> Lines {
    let (inp, st) = (m.inp, &m.st);
    let mut route_line: FxHashMap<&str, &str> = FxHashMap::default();
    for l in &inp.lines {
        for r in l["routes"].as_array().into_iter().flatten() {
            route_line.insert(r.as_str().unwrap_or(""), l["id"].as_str().unwrap_or(""));
        }
    }
    let mut st_lines: Vec<BTreeSet<String>> = vec![BTreeSet::new(); st.len()];
    let mut route_unmapped: FxHashSet<&str> = FxHashSet::default();
    let mut route_order = Map::new();
    let mut routes_total = 0;
    for r in inp.rels.iter().filter(|r| r["kind"] == "route") {
        routes_total += 1;
        let rid = r["id"].as_str().unwrap_or("");
        let lid = route_line.get(rid).copied();
        let mut order: Vec<usize> = Vec::new();
        for mb in members(r) {
            if !(mb.role.starts_with("stop") || mb.role.starts_with("platform")) {
                continue;
            }
            match node_station.get(&mb.key()) {
                None => {
                    route_unmapped.insert(rid);
                }
                Some(&si) => {
                    if let Some(l) = lid {
                        st_lines[si].insert(l.to_string());
                    }
                    if order.last() != Some(&si) {
                        order.push(si);
                    }
                }
            }
        }
        route_order.insert(rid.to_string(), Value::Array(order.iter().map(|&i| Value::String(station_id(&m.feats, &st[i]))).collect()));
    }
    // named infrastructure lines (route=railway) that list the station as a member
    let mut st_rail: Vec<Map<String, Value>> = vec![Map::new(); st.len()];
    for r in inp.rels.iter().filter(|r| r["kind"] == "railway_line") {
        let rid = r["id"].as_str().unwrap_or("");
        let tags = r["tags"].as_object().cloned().unwrap_or_default();
        for mb in members(r) {
            if let Some(&si) = node_station.get(&mb.key()) {
                st_rail[si]
                    .insert(rid.to_string(), json!({"id": rid, "name": tags.get("name").cloned().unwrap_or(Value::Null), "name_en": english_name(&tags)}));
            }
        }
    }
    Lines { st_lines, st_rail, route_order, routes_total, routes_unmapped: route_unmapped.len() }
}

pub(super) struct Complex {
    pub id: String,
    pub members: Vec<usize>, // station indices
    pub biggest: usize,      // gives the point and the name
    pub how: &'static str,   // "mapped" (a stop_area_group) or "inferred"
}

pub(super) struct Complexes {
    pub rows: Vec<Complex>,
    pub of: Vec<String>, // complex id of each station
    pub mapped_pairs: usize,
}

/// step 5: complexes: stop-area groups, else different modes with a shared wikidata, the same normalised name within GROUP_RADIUS or
/// the same station building
pub(super) fn complexes(m: &Model, sizes: &[Size]) -> Complexes {
    let (st, buildings) = (&m.st, &m.feats.buildings);
    let mut cuf = UnionFind::new(st.len());
    let mut mapped_pairs = 0;
    let group_stations = |g: &Value| -> BTreeSet<usize> {
        members(g).filter_map(|mb| m.area_to_station.get(&format!("{}/{}", mb.kind.unwrap_or(""), mb.id.unwrap_or(0))).copied()).collect()
    };
    for g in &m.rels.group_rows {
        let idx: Vec<usize> = group_stations(g).into_iter().collect();
        for &a in idx.iter().skip(1) {
            cuf.union(idx[0], a);
            mapped_pairs += 1;
        }
    }
    let wd: Vec<BTreeSet<String>> = st.iter().map(|x| station_wikidata(&m.feats, x)).collect();
    // inferred: different modes, and shared wikidata, or the same normalised name within 300 m, or the same station building
    let mut by_bld: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, x) in st.iter().enumerate() {
        if let Some(bi) = m.feats.bld_tree.at(&x.pt).into_iter().find(|&bi| geomx::contains_pt(&buildings[bi].g, &x.pt)) {
            by_bld.entry(bi).or_default().push(i);
        }
    }
    for idxs in by_bld.values() {
        for &a in &idxs[1..] {
            if st[a].modes != st[idxs[0]].modes {
                cuf.union(idxs[0], a);
            }
        }
    }
    for i in 0..st.len() {
        let pg = Geometry::Point(st[i].pt);
        for j in m.st_tree.near(&pg, GROUP_RADIUS) {
            if j <= i || st[j].modes == st[i].modes {
                continue;
            }
            let same_name = !st[i].norm.is_empty() && st[i].norm == st[j].norm && geomx::dist_pt(&pg, &st[j].pt) <= GROUP_RADIUS;
            if !wd[i].is_disjoint(&wd[j]) || same_name {
                cuf.union(i, j);
            }
        }
    }
    let mut of = vec![String::new(); st.len()];
    let mut rows: Vec<Complex> = Vec::new();
    for idxs in groups(&mut cuf, st.len()) {
        let biggest = idxs
            .iter()
            .copied()
            .fold(None, |a: Option<usize>, i| {
                if a.is_none_or(|b| (sizes[i].score, sizes[i].mainline_platforms) > (sizes[b].score, sizes[b].mainline_platforms)) {
                    Some(i)
                } else {
                    a
                }
            })
            .unwrap();
        let gid = m.rels.group_rows.iter().find(|g| group_stations(g).iter().any(|si| idxs.contains(si))).and_then(|g| g["id"].as_str()).map(String::from);
        let id = gid.clone().unwrap_or_else(|| station_id(&m.feats, &st[biggest]));
        for &i in &idxs {
            of[i] = id.clone();
        }
        rows.push(Complex { id, members: idxs, biggest, how: if gid.is_some() { "mapped" } else { "inferred" } });
    }
    Complexes { rows, of, mapped_pairs }
}
