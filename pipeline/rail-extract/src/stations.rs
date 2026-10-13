//! Station model (docs/04-station-model.md), ported from pipeline/stations.py.
//!
//! Levels: Complex (the place a traveller names) > Station (one per mode group) > stop points. Rank comes from physical size only;
//! lines are counted for the panel and never set rank. Same steps, rules and outputs as stations.py; where Python took the first of
//! equal candidates in shapely's tree order (or iterated a set), this takes the lowest index (or sorted order), so runs repeat.

use crate::geomx::{self, Index, G};
use crate::pyjson::{f, py_round};
use crate::site::{s, Feat, Input};
use geo::{Geometry, MultiPolygon, Point};
use regex::Regex;
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::{char::canonical_combining_class, UnicodeNormalization};

const RADIUS_TRAIN: f64 = 300.0; // metres, tuned on the pilots
const RADIUS_OTHER: f64 = 100.0;
const GROUP_RADIUS: f64 = 300.0;
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
const STOPWORDS: [&str; 26] = ["gare", "du", "de", "d", "la", "le", "les", "l", "des", "station", "paris", "stazione", "railway", "rer", "metro",
    "subway", "bart", "caltrain", "amtrak", "light", "rail", "vta", "muni", "st", "saint", ""];

fn radius(m: &str) -> f64 {
    if m == "train" { RADIUS_TRAIN } else { RADIUS_OTHER }
}

fn max_radius(ms: &BTreeSet<String>) -> f64 {
    ms.iter().map(|m| radius(m)).fold(f64::MIN, f64::max)
}

fn mode_key(k: &str) -> Option<&'static str> {
    Some(match k {
        "subway" => "subway",
        "light_rail" => "light_rail",
        "train" => "train",
        "monorail" => "monorail",
        "funicular" => "funicular",
        _ => return None,
    })
}

fn track_mode(t: &str) -> &'static str {
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

/// stations.py norm_name: lower case, accents off, bracketed parts and punctuation out, a trailing 站 off, stopwords out
pub fn norm_name(name: &str) -> String {
    thread_local! {
        static RE: (Regex, Regex, Regex) = (
            Regex::new(r"\(.*?\)").unwrap(),
            Regex::new(r"[^\p{L}\p{N}_一-鿿]+").unwrap(),   // Python's \w: letters, numbers, underscore
            Regex::new(r"(地铁站|火车站|客站|站)$").unwrap(),
        );
    }
    if name.is_empty() {
        return String::new();
    }
    let s: String = name.to_lowercase().nfkd().filter(|c| canonical_combining_class(*c) == 0).collect();
    RE.with(|(paren, punct, tail)| {
        let s = paren.replace_all(&s, " ");
        let s = punct.replace_all(&s, " ");
        let s = tail.replace(s.trim(), "");
        s.split_whitespace().filter(|t| !STOPWORDS.contains(t)).collect::<Vec<_>>().join(" ")
    })
}

/// names.py english_name over a property map
pub fn english_name(t: &Map<String, Value>) -> Option<String> {
    let g = |k: &str| t.get(k).and_then(Value::as_str).filter(|v| !v.is_empty());
    if let Some(v) = g("name:en") {
        return Some(v.to_string());
    }
    if let Some(v) = g("int_name") {
        return Some(v.to_string());
    }
    for (k, v) in t {
        if let Some(v) = v.as_str() {
            if k.starts_with("name:") && !v.is_empty() && v.chars().all(|c| (c as u32) <= 0x24f || (0x1e00..=0x1eff).contains(&(c as u32))) {
                return Some(v.to_string());
            }
        }
    }
    None
}

pub fn ref_tokens(r: Option<&str>) -> BTreeSet<String> {
    r.unwrap_or("").split([';', ',', '/']).map(str::trim).filter(|t| !t.is_empty()).map(String::from).collect()
}

/// sort key of platform refs: numbers first, by value, then the rest
fn ref_key(r: &str) -> (bool, u128, String) {
    let num = !r.is_empty() && r.chars().all(|c| c.is_ascii_digit());
    (!num, if num { r.parse().unwrap_or(u128::MAX) } else { 0 }, r.to_string())
}

struct UF(Vec<usize>);
impl UF {
    fn new(n: usize) -> UF {
        UF((0..n).collect())
    }
    fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[b] = a;
        }
    }
}

/// groups in order of their first member
fn groups(uf: &mut UF, n: usize) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = Vec::new();
    let mut by_root: FxHashMap<usize, usize> = FxHashMap::default();
    let mut out: Vec<Vec<usize>> = Vec::new();
    for i in 0..n {
        let r = uf.find(i);
        let k = *by_root.entry(r).or_insert_with(|| {
            order.push(r);
            out.push(Vec::new());
            out.len() - 1
        });
        out[k].push(i);
    }
    out
}

/// a projected feature (metres in a local equirectangular projection)
struct It<'a> {
    f: &'a Feat,
    g: G,
    modes: BTreeSet<String>, // platforms
}

impl It<'_> {
    fn p(&self, k: &str) -> Option<&str> {
        s(&self.f.p, k)
    }
    fn id(&self) -> &str {
        s(&self.f.p, "osm_id").unwrap_or("")
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
}

/// what stations.py writes, kept in memory for the next steps
pub struct Out {
    pub stations: Vec<Value>,  // GeoJSON features, stations.geojsonl
    pub complexes: Vec<Value>, // complexes.geojsonl
    pub route_order: Map<String, Value>,
    pub report: Value,
}

pub fn run(slug: &str, inp: &Input) -> Out {
    // ---- local metric projection
    let lats: Vec<f64> = inp.feats.iter().filter(|f| f.gtype == "Point").filter_map(|f| f.g["coordinates"][1].as_f64()).collect();
    let lat0 = if lats.is_empty() { 40.0 } else { lats.iter().sum::<f64>() / lats.len() as f64 };
    let (kx, ky) = (111320.0 * lat0.to_radians().cos(), 110540.0);
    let proj = move |x: f64, y: f64| (x * kx, y * ky);
    let unproj = |p: &Point<f64>| json!([f(py_round(p.x() / kx, 5)), f(py_round(p.y() / ky, 5))]);

    // ---- sort features
    let (mut tracks, mut platforms, mut buildings, mut lands, mut station_feats): (Vec<It>, Vec<It>, Vec<It>, Vec<It>, Vec<It>) =
        Default::default();
    for fe in &inp.feats {
        let Some(g) = geomx::from_geojson(&fe.g, proj) else { continue };
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
        } else if it.p("building") == Some("train_station") {
            buildings.push(it);
        } else if it.p("landuse") == Some("railway") {
            lands.push(it);
        } else if matches!(rw, Some("station" | "halt" | "tram_stop")) || pt == Some("station") {
            if rw == Some("construction") {
                continue;
            }
            station_feats.push(it);
        }
    }
    let track_tree = Index::new(tracks.iter().map(|t| geomx::bbox(&t.g)));
    let land_tree = Index::new(lands.iter().map(|t| geomx::bbox(&t.g)));
    let bld_tree = Index::new(buildings.iter().map(|t| geomx::bbox(&t.g)));

    let nearest_track_mode = |g: &G, r: f64| -> Option<&'static str> {
        let mut best: Option<(usize, f64)> = None;
        for i in track_tree.near(g, r) {
            let d = geomx::dist(&tracks[i].g, g);
            if best.map_or(true, |(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        let (i, d) = best?;
        if d > r {
            return None;
        }
        Some(track_mode(tracks[i].p("type").unwrap_or("")))
    };
    let feature_modes = |it: &It| -> BTreeSet<String> {
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
        if ms.is_empty() {
            ms.insert(nearest_track_mode(&it.g, 150.0).unwrap_or("train").to_string());
        }
        ms
    };

    // ---- relation lookups
    let mut member_area: Vec<((String, i64), String)> = Vec::new(); // in relation order; a later stop area wins (dict semantics)
    let mut member_area_ix: FxHashMap<(String, i64), usize> = FxHashMap::default();
    let mut area_rows: FxHashMap<String, &Value> = FxHashMap::default();
    let mut group_rows: Vec<&Value> = Vec::new();
    for r in &inp.rels {
        match r["kind"].as_str() {
            Some("stop_area") => {
                let rid = r["id"].as_str().unwrap_or("").to_string();
                area_rows.insert(rid.clone(), r);
                for m in r["members"].as_array().into_iter().flatten() {
                    let key = (m["type"].as_str().unwrap_or("").to_string(), m["id"].as_i64().unwrap_or(0));
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

    // ---- steps 1 + 2: stations from stop areas, else from station nodes
    let mut st: Vec<Station> = Vec::new();
    let mut by_area: Vec<(String, usize)> = Vec::new();
    let mut by_area_ix: FxHashMap<String, usize> = FxHashMap::default();
    for (k, it) in station_feats.iter().enumerate() {
        let sa = it.p("_stop_area").filter(|v| !v.is_empty());
        let modes = feature_modes(it);
        if let Some(&i) = sa.and_then(|a| by_area_ix.get(a)) {
            st[i].feats.push(k);
            st[i].modes.extend(modes);
            continue;
        }
        st.push(Station {
            feats: vec![k],
            modes,
            areas: sa.map(|a| BTreeSet::from([a.to_string()])).unwrap_or_default(),
            platforms: vec![],
            grouping: if sa.is_some() { "mapped" } else { "inferred" },
            pt: Point::new(0.0, 0.0),
            norm: String::new(),
            merged_from: 1,
        });
        if let Some(a) = sa {
            by_area_ix.insert(a.to_string(), st.len() - 1);
            by_area.push((a.to_string(), st.len() - 1));
        }
    }
    for s_ in st.iter_mut() {
        let pts: Vec<Point<f64>> = s_.feats.iter().filter_map(|&k| if let Geometry::Point(p) = station_feats[k].g { Some(p) } else { None }).collect();
        s_.pt = pts.first().copied().or_else(|| geomx::rep_point(&station_feats[s_.feats[0]].g)).unwrap_or(Point::new(0.0, 0.0));
    }
    let s_tree = Index::new(st.iter().map(|x| geomx::bbox(&Geometry::Point(x.pt))));

    // platforms: mapped via stop_area first; the rest go to the station whose footprint (point + mapped platforms) is nearest and
    // within its radius, else the station whose railway land holds them
    let mut plat_station: Vec<(String, usize)> = Vec::new();
    let mut plat_station_ix: FxHashMap<String, usize> = FxHashMap::default();
    for k in 0..platforms.len() {
        let mut pm: BTreeSet<String> = ["subway", "light_rail", "train"].iter().filter(|m| platforms[k].p(m) == Some("yes")).map(|m| m.to_string()).collect();
        if pm.is_empty() {
            if let Some(m) = nearest_track_mode(&platforms[k].g, 25.0) {
                pm.insert(m.to_string());
            }
        }
        platforms[k].modes = pm;
        if let Some(&i) = platforms[k].p("_stop_area").and_then(|a| by_area_ix.get(a)) {
            st[i].platforms.push(k);
            plat_station_ix.insert(platforms[k].id().to_string(), plat_station.len());
            plat_station.push((platforms[k].id().to_string(), i));
        }
    }
    let mut anchors: Vec<G> = Vec::new();
    let mut anchor_owner: Vec<usize> = Vec::new();
    for (i, s_) in st.iter().enumerate() {
        anchors.push(Geometry::Point(s_.pt));
        anchor_owner.push(i);
        for &x in &s_.platforms {
            anchors.push(platforms[x].g.clone());
            anchor_owner.push(i);
        }
    }
    let anchor_tree = Index::new(anchors.iter().map(geomx::bbox));
    for k in 0..platforms.len() {
        if plat_station_ix.contains_key(platforms[k].id()) {
            continue;
        }
        let pm = &platforms[k].modes;
        let Some(c) = geomx::rep_point(&platforms[k].g) else { continue };
        let cg = Geometry::Point(c);
        let mut best: Option<(usize, f64)> = None;
        for ai in anchor_tree.near(&cg, 300.0) {
            let i = anchor_owner[ai];
            if !pm.is_empty() && pm.is_disjoint(&st[i].modes) {
                continue;
            }
            let d = geomx::dist(&anchors[ai], &cg);
            if d <= max_radius(&st[i].modes) * if pm.is_empty() { 0.5 } else { 1.0 } && best.map_or(true, |(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        let mut best = best.map(|b| b.0);
        if best.is_none() {
            for li in land_tree.at(&c) {
                if geomx::contains_pt(&lands[li].g, &c) {
                    let near: Vec<usize> = s_tree
                        .near(&cg, 200.0)
                        .into_iter()
                        .filter(|&i| geomx::contains_pt(&lands[li].g, &st[i].pt) && (pm.is_empty() || !pm.is_disjoint(&st[i].modes)))
                        .collect();
                    if !near.is_empty() {
                        best = near.into_iter().map(|i| (i, geomx::dist_pt(&cg, &st[i].pt))).fold(None, |a: Option<(usize, f64)>, (i, d)| {
                            if a.map_or(true, |(_, bd)| d < bd) { Some((i, d)) } else { a }
                        }).map(|x| x.0);
                        break;
                    }
                }
            }
        }
        if let Some(b) = best {
            st[b].platforms.push(k);
            plat_station_ix.insert(platforms[k].id().to_string(), plat_station.len());
            plat_station.push((platforms[k].id().to_string(), b));
        }
    }

    // ---- step 3: merge duplicates of the same mode
    let mut uf = UF::new(st.len());
    let mut keys_of: Vec<BTreeSet<(String, String)>> = Vec::with_capacity(st.len());
    for s_ in st.iter_mut() {
        let name = s_.feats.iter().find_map(|&k| station_feats[k].p("name").filter(|n| !n.is_empty())).unwrap_or("");
        s_.norm = norm_name(name);
        let mut ks = BTreeSet::new();
        for &k in &s_.feats {
            for key in ["wikidata", "uic_ref", "railway:ref"] {
                if let Some(v) = station_feats[k].p(key).filter(|v| !v.is_empty()) {
                    ks.insert((key.to_string(), v.to_string()));
                }
            }
        }
        keys_of.push(ks);
    }
    let mut key_index: BTreeMap<&(String, String), Vec<usize>> = BTreeMap::new();
    for (i, ks) in keys_of.iter().enumerate() {
        for k in ks {
            key_index.entry(k).or_default().push(i);
        }
    }
    for idxs in key_index.values() {
        for &a in &idxs[1..] {
            if !st[a].modes.is_disjoint(&st[idxs[0]].modes) {
                uf.union(idxs[0], a);
            }
        }
    }
    for i in 0..st.len() {
        if st[i].norm.is_empty() {
            continue;
        }
        let r = max_radius(&st[i].modes);
        for j in s_tree.near(&Geometry::Point(st[i].pt), r) {
            if j <= i || st[j].norm != st[i].norm || st[j].modes.is_disjoint(&st[i].modes) {
                continue;
            }
            if geomx::dist_pt(&Geometry::Point(st[i].pt), &st[j].pt) <= r {
                uf.union(i, j);
            }
        }
    }
    let mut stn: Vec<Station> = Vec::new(); // final stations
    let mut old_to_new = vec![0usize; st.len()];
    for members in groups(&mut uf, st.len()) {
        let ms: Vec<&Station> = members.iter().map(|&i| &st[i]).collect();
        let all_mapped = ms.iter().all(|m| m.grouping == "mapped");
        let any_mapped = ms.iter().any(|m| m.grouping == "mapped");
        let grouping = if all_mapped && ms.len() == 1 { "mapped" } else if ms.len() > 1 && !any_mapped { "inferred" } else { "mapped" };
        let mut seen = FxHashSet::default();
        let plats: Vec<usize> = ms.iter().flat_map(|m| m.platforms.iter().copied()).filter(|&x| seen.insert(platforms[x].id().to_string())).collect();
        // the member with the most platforms gives the point and the name (the first such, as Python's max)
        let lead = ms.iter().fold(None, |a: Option<&&Station>, m| if a.map_or(true, |b| m.platforms.len() > b.platforms.len()) { Some(m) } else { a }).unwrap();
        for &i in &members {
            old_to_new[i] = stn.len();
        }
        stn.push(Station {
            feats: ms.iter().flat_map(|m| m.feats.iter().copied()).collect(),
            modes: ms.iter().flat_map(|m| m.modes.iter().cloned()).collect(),
            areas: ms.iter().flat_map(|m| m.areas.iter().cloned()).collect(),
            platforms: plats,
            grouping,
            pt: lead.pt,
            norm: lead.norm.clone(),
            merged_from: ms.len(),
        });
    }
    let st = stn;
    let st_tree = Index::new(st.iter().map(|x| geomx::bbox(&Geometry::Point(x.pt))));
    let area_to_station: FxHashMap<String, usize> = by_area.iter().map(|(a, i)| (a.clone(), old_to_new[*i])).collect();
    for x in plat_station.iter_mut() {
        x.1 = old_to_new[x.1];
    }

    // ---- size inputs: track dead ends (terminus flag): ends used by exactly one mainline way
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

    // ---- major station rules (docs/00-overview.md, 2026-10-11 "Major station rules")
    let main_re = inp.cfg.get("main_station_re").and_then(Value::as_str).filter(|v| !v.is_empty()).map(|p| Regex::new(p).expect("main_station_re"));
    let main_norms: BTreeSet<String> =
        inp.cfg.get("main_terminals").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(norm_name).filter(|n| !n.is_empty()).collect();
    // high-speed track by its OWN tags (a classic line that TGVs merely use does not count)
    let hs: Vec<usize> = (0..tracks.len()).filter(|&i| tracks[i].p("own_type") == Some("high_speed") && tracks[i].p("railway") != Some("construction")).collect();
    let hs_tree = Index::new(hs.iter().map(|&i| geomx::bbox(&tracks[i].g)));

    struct Size {
        mainline_platforms: i64,
        platform_count: i64,
        land_ha: f64,
        building_ha: f64,
        mainline_track_ways: i64,
        dead_ends: i64,
        terminus: bool,
        score: f64,
        tier: i64,
        hsr: bool,
        major: Option<&'static str>,
    }
    use rayon::prelude::*;
    let sizes: Vec<Size> = st
        .par_iter()
        .map(|x| {
            let mp: Vec<usize> = x.platforms.iter().copied().filter(|&k| platforms[k].modes.contains("train")).collect();
            let (mut toks, mut unreffed) = (BTreeSet::new(), 0i64);
            for &k in &mp {
                let t = ref_tokens(platforms[k].p("ref"));
                if t.is_empty() {
                    unreffed += 1;
                } else {
                    toks.extend(t);
                }
            }
            let mainline_platforms = toks.len() as i64 + unreffed;
            let all_toks: BTreeSet<String> = x.platforms.iter().flat_map(|&k| ref_tokens(platforms[k].p("ref"))).collect();
            let platform_count = all_toks.len() as i64 + x.platforms.iter().filter(|&&k| ref_tokens(platforms[k].p("ref")).is_empty()).count() as i64;
            // railway land and building holding the station's point or its platforms
            let mut probes = vec![x.pt];
            probes.extend(x.platforms.iter().take(60).filter_map(|&k| geomx::rep_point(&platforms[k].g)));
            let (mut land_ids, mut bld_ids) = (BTreeSet::new(), BTreeSet::new());
            for pr in &probes {
                for li in land_tree.at(pr) {
                    if geomx::contains_pt(&lands[li].g, pr) {
                        land_ids.insert(li);
                    }
                }
                for bi in bld_tree.at(pr) {
                    if geomx::contains_pt(&buildings[bi].g, pr) {
                        bld_ids.insert(bi);
                    }
                }
            }
            let land_ha = if land_ids.is_empty() {
                0.0
            } else {
                let u = geomx::union_all(land_ids.iter().map(|&i| geomx::polys(&lands[i].g)).collect());
                let local: MultiPolygon<f64> = geomx::intersection(&u, &geomx::circle(x.pt, LAND_DISC));
                if local.0.is_empty() { 0.0 } else { py_round(geomx::area(&Geometry::MultiPolygon(local)) / 10000.0, 2) }
            };
            let building_ha = py_round(bld_ids.iter().map(|&i| geomx::area(&buildings[i].g) / 10000.0).sum::<f64>(), 3);
            // mainline tracks serving the platforms (or near the point when no platforms are mapped)
            let mut near: BTreeSet<&str> = BTreeSet::new();
            let pg = Geometry::Point(x.pt);
            if !mp.is_empty() && x.modes.contains("train") {
                for &k in &mp {
                    for ti in mt_tree.near(&platforms[k].g, 20.0) {
                        if geomx::dist(&tracks[main_tracks[ti]].g, &platforms[k].g) <= 20.0 {
                            near.insert(tracks[main_tracks[ti]].id());
                        }
                    }
                }
            } else if x.modes.contains("train") {
                for ti in mt_tree.near(&pg, 80.0) {
                    if geomx::dist(&tracks[main_tracks[ti]].g, &pg) <= 80.0 {
                        near.insert(tracks[main_tracks[ti]].id());
                    }
                }
            }
            // terminus: at least two mainline dead ends within TERMINUS_ZONE of the station's mainline platforms or point
            // (stations.py buffers their union by 120 m and tests containment)
            let mut dead = 0i64;
            if x.modes.contains("train") {
                let zone: Vec<&G> = mp.iter().map(|&k| &platforms[k].g).collect();
                let mut cands: BTreeSet<usize> = dead_tree.near(&pg, TERMINUS_ZONE).into_iter().collect();
                for g in &zone {
                    cands.extend(dead_tree.near(g, TERMINUS_ZONE));
                }
                for di in cands {
                    let p = &dead_pts[di];
                    if geomx::dist_pt(&pg, p) < TERMINUS_ZONE || zone.iter().any(|g| geomx::dist_pt(g, p) < TERMINUS_ZONE) {
                        dead += 1;
                    }
                }
            }
            let terminus = dead >= 2;
            let score = py_round(
                W_PLATFORM * mainline_platforms as f64 + W_TRACK * (near.len() as f64).min(W_TRACK_CAP) + W_LAND * land_ha.sqrt()
                    + W_BUILDING * building_ha.sqrt() + if terminus { W_TERMINUS } else { 0.0 },
                1,
            );
            let mut tier = if score >= 60.0 { 1 } else if score >= 25.0 { 2 } else { 3 };
            let hsr = hs_tree.near(&pg, HS_NEAR).into_iter().any(|ti| geomx::dist(&tracks[hs[ti]].g, &pg) <= HS_NEAR);
            let names: Vec<&str> = x.feats.iter().map(|&k| station_feats[k].p("name").unwrap_or("")).collect();
            let mut norms: BTreeSet<String> = names.iter().map(|n| norm_name(n)).collect();
            norms.insert(x.norm.clone());
            let mut major = None;
            if x.modes.contains("train") {
                if main_re.as_ref().map_or(false, |re| names.iter().any(|n| re.is_match(n))) {
                    major = Some("name_pattern");
                } else if norms.iter().any(|n| main_norms.contains(n)) {
                    major = Some("canonical_list");
                } else if hsr && (terminus || mainline_platforms >= HS_MAJOR_PLATFORMS) {
                    major = Some("hsr_infrastructure");
                }
            }
            if major.is_some() {
                tier = 1;
            }
            Size { mainline_platforms, platform_count, land_ha, building_ha, mainline_track_ways: near.len() as i64, dead_ends: dead, terminus, score, tier, hsr, major }
        })
        .collect();

    // ---- stop positions -> stations
    let mut node_station: FxHashMap<(String, i64), usize> = FxHashMap::default();
    for (si, x) in st.iter().enumerate() {
        for &k in &x.feats {
            let (t, n) = station_feats[k].id().split_once('/').unwrap_or(("", "0"));
            node_station.insert((t.to_string(), n.parse().unwrap_or(0)), si);
        }
    }
    for (key, sa) in &member_area {
        if let Some(&si) = area_to_station.get(sa) {
            node_station.entry(key.clone()).or_insert(si);
        }
    }
    for (pid, si) in &plat_station {
        let (t, n) = pid.split_once('/').unwrap_or(("", "0"));
        node_station.entry((t.to_string(), n.parse().unwrap_or(0))).or_insert(*si);
    }
    let mut stop_unmapped = 0usize;
    for sp in &inp.stops {
        let id: i64 = sp["osm_id"].as_str().and_then(|v| v.split_once('/')).and_then(|(_, n)| n.parse().ok()).unwrap_or(0);
        let key = ("node".to_string(), id);
        if node_station.contains_key(&key) {
            continue;
        }
        let (x, y) = proj(sp["lon"].as_f64().unwrap_or(0.0), sp["lat"].as_f64().unwrap_or(0.0));
        let pg = Geometry::Point(Point::new(x, y));
        let pm: BTreeSet<&str> = ["subway", "light_rail", "train"].into_iter().filter(|k| sp["modes"].get(*k).and_then(Value::as_str) == Some("yes")).collect();
        let nn = norm_name(sp["name"].as_str().unwrap_or(""));
        let mut best: Option<(usize, f64)> = None;
        for i in st_tree.near(&pg, 300.0) {
            if !pm.is_empty() && !pm.iter().any(|m| st[i].modes.contains(*m)) {
                continue;
            }
            let d = geomx::dist_pt(&pg, &st[i].pt);
            if d > max_radius(&st[i].modes) {
                continue;
            }
            let score = d - if !nn.is_empty() && nn == st[i].norm { 500.0 } else { 0.0 };
            if best.map_or(true, |(_, b)| score < b) {
                best = Some((i, score));
            }
        }
        match best {
            Some((i, _)) => {
                node_station.insert(key, i);
            }
            None => stop_unmapped += 1,
        }
    }

    // ---- step 4: lines per station
    let mut route_line: FxHashMap<&str, &str> = FxHashMap::default();
    for l in &inp.lines {
        for r in l["routes"].as_array().into_iter().flatten() {
            route_line.insert(r.as_str().unwrap_or(""), l["id"].as_str().unwrap_or(""));
        }
    }
    let sid_of = |i: usize| -> String { st[i].areas.iter().next().cloned().unwrap_or_else(|| station_feats[st[i].feats[0]].id().to_string()) };
    let mut st_lines: Vec<BTreeSet<String>> = vec![BTreeSet::new(); st.len()];
    let mut route_unmapped: FxHashSet<&str> = FxHashSet::default();
    let mut route_order = Map::new();
    let mut routes_total = 0;
    for r in inp.rels.iter().filter(|r| r["kind"] == "route") {
        routes_total += 1;
        let rid = r["id"].as_str().unwrap_or("");
        let lid = route_line.get(rid).copied();
        let mut order: Vec<usize> = Vec::new();
        for m in r["members"].as_array().into_iter().flatten() {
            let role = m["role"].as_str().unwrap_or("");
            if !(role.starts_with("stop") || role.starts_with("platform")) {
                continue;
            }
            match node_station.get(&(m["type"].as_str().unwrap_or("").to_string(), m["id"].as_i64().unwrap_or(0))) {
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
        route_order.insert(rid.to_string(), Value::Array(order.iter().map(|&i| Value::String(sid_of(i))).collect()));
    }
    // named infrastructure lines (route=railway) that list the station as a member
    let mut st_rail: Vec<Map<String, Value>> = vec![Map::new(); st.len()];
    for r in inp.rels.iter().filter(|r| r["kind"] == "railway_line") {
        let rid = r["id"].as_str().unwrap_or("");
        let tags = r["tags"].as_object().cloned().unwrap_or_default();
        for m in r["members"].as_array().into_iter().flatten() {
            if let Some(&si) = node_station.get(&(m["type"].as_str().unwrap_or("").to_string(), m["id"].as_i64().unwrap_or(0))) {
                st_rail[si].insert(rid.to_string(), json!({"id": rid, "name": tags.get("name").cloned().unwrap_or(Value::Null), "name_en": english_name(&tags)}));
            }
        }
    }

    // ---- step 5: complexes
    let mut cuf = UF::new(st.len());
    let mut mapped_pairs = 0;
    let member_key = |m: &Value| format!("{}/{}", m["type"].as_str().unwrap_or(""), m["id"].as_i64().unwrap_or(0));
    for g in &group_rows {
        let idx: BTreeSet<usize> = g["members"].as_array().into_iter().flatten().filter_map(|m| area_to_station.get(&member_key(m)).copied()).collect();
        let idx: Vec<usize> = idx.into_iter().collect();
        for &a in idx.iter().skip(1) {
            cuf.union(idx[0], a);
            mapped_pairs += 1;
        }
    }
    let wd: Vec<BTreeSet<String>> = st.iter().map(|x| x.feats.iter().filter_map(|&k| station_feats[k].p("wikidata").filter(|v| !v.is_empty()).map(String::from)).collect()).collect();
    // inferred: different modes, and shared wikidata, or the same normalised name within 300 m, or the same station building
    let mut by_bld: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, x) in st.iter().enumerate() {
        if let Some(bi) = bld_tree.at(&x.pt).into_iter().find(|&bi| geomx::contains_pt(&buildings[bi].g, &x.pt)) {
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
        for j in st_tree.near(&pg, GROUP_RADIUS) {
            if j <= i || st[j].modes == st[i].modes {
                continue;
            }
            let same_name = !st[i].norm.is_empty() && st[i].norm == st[j].norm && geomx::dist_pt(&pg, &st[j].pt) <= GROUP_RADIUS;
            if !wd[i].is_disjoint(&wd[j]) || same_name {
                cuf.union(i, j);
            }
        }
    }
    let complexes = groups(&mut cuf, st.len());

    // ---- write
    // name: a stop area's name first, else the station's best-named feature (named first, then points); stations.py iterated the
    // stop areas as a set, in an order that changes between runs; here they are sorted
    let stn_name = |x: &Station| -> (Option<String>, Map<String, Value>) {
        for sa in &x.areas {
            if let Some(r) = area_rows.get(sa) {
                if let Some(nm) = r["tags"]["name"].as_str().filter(|v| !v.is_empty()) {
                    return (Some(nm.to_string()), r["tags"].as_object().cloned().unwrap_or_default());
                }
            }
        }
        let best = x.feats.iter().copied().fold(None, |a: Option<usize>, k| {
            let rank = |k: usize| (station_feats[k].p("name").map_or(false, |v| !v.is_empty()), matches!(station_feats[k].g, Geometry::Point(_)));
            if a.map_or(true, |b| rank(k) > rank(b)) { Some(k) } else { a }
        }).unwrap();
        (station_feats[best].p("name").map(String::from), station_feats[best].f.p.clone())
    };
    let name_tags = |x: &Station| -> Map<String, Value> {
        let mut t = Map::new();
        for &k in &x.feats {
            for (key, v) in &station_feats[k].f.p {
                if key.starts_with("name:") || key == "int_name" || key == "official_name" {
                    t.insert(key.clone(), v.clone());
                }
            }
        }
        for sa in &x.areas {
            if let Some(r) = area_rows.get(sa) {
                for (key, v) in r["tags"].as_object().into_iter().flatten() {
                    if key.starts_with("name:") {
                        t.insert(key.clone(), v.clone());
                    }
                }
            }
        }
        t
    };
    // {**tg, **nt}: tg's keys in order, nt's values winning, nt's new keys appended
    let merge = |mut a: Map<String, Value>, b: Map<String, Value>| {
        for (k, v) in b {
            a.insert(k, v);
        }
        a
    };
    let mut complex_of = vec![String::new(); st.len()];
    let mut cplx: Vec<(String, &Vec<usize>, usize, &'static str)> = Vec::new();
    for idxs in &complexes {
        let biggest = idxs.iter().copied().fold(None, |a: Option<usize>, i| {
            if a.map_or(true, |b| (sizes[i].score, sizes[i].mainline_platforms) > (sizes[b].score, sizes[b].mainline_platforms)) { Some(i) } else { a }
        }).unwrap();
        let gid = group_rows.iter().find(|g| g["members"].as_array().into_iter().flatten().any(|m| area_to_station.get(&member_key(m)).map_or(false, |si| idxs.contains(si))))
            .and_then(|g| g["id"].as_str()).map(String::from);
        let cid = gid.clone().unwrap_or_else(|| sid_of(biggest));
        for &i in idxs {
            complex_of[i] = cid.clone();
        }
        cplx.push((cid, idxs, biggest, if gid.is_some() { "mapped" } else { "inferred" }));
    }
    let lines_by_id: FxHashMap<&str, &Value> = inp.lines.iter().map(|l| (l["id"].as_str().unwrap_or(""), l)).collect();
    let mut stations_out = Vec::with_capacity(st.len());
    for (i, x) in st.iter().enumerate() {
        let (nm, tg) = stn_name(x);
        let lids: Vec<&String> = st_lines[i].iter().filter(|l| !l.is_empty()).collect();
        let mut by_type: Vec<(String, i64)> = Vec::new();
        for l in &lids {
            if let Some(t) = lines_by_id.get(l.as_str()).and_then(|l| l["type"].as_str()) {
                match by_type.iter_mut().find(|e| e.0 == t) {
                    Some(e) => e.1 += 1,
                    None => by_type.push((t.to_string(), 1)),
                }
            }
        }
        let z = &sizes[i];
        let mut rail: Vec<&Value> = st_rail[i].values().collect();
        rail.sort_by(|a, b| (a["name"].as_str().unwrap_or(""), a["id"].as_str()).cmp(&(b["name"].as_str().unwrap_or(""), b["id"].as_str())));
        let refs = |train: bool| -> Value {
            let set: BTreeSet<String> = x.platforms.iter().filter(|&&k| platforms[k].modes.contains("train") == train).flat_map(|&k| ref_tokens(platforms[k].p("ref"))).collect();
            let mut v: Vec<String> = set.into_iter().collect();
            v.sort_by_key(|r| ref_key(r));
            json!(v)
        };
        let mut sources: Vec<Value> = x.feats.iter().map(|&k| json!(station_feats[k].id())).collect();
        sources.extend(x.areas.iter().map(|a| json!(a)));
        let props = json!({
            "id": sid_of(i), "complex": complex_of[i], "name": nm, "name_en": english_name(&merge(tg, name_tags(x))),
            "modes": x.modes.iter().collect::<Vec<_>>(), "tier": z.tier, "score": f(z.score), "terminus": z.terminus,
            "mainline_platforms": z.mainline_platforms, "platform_count": z.platform_count,
            "land_ha": f(z.land_ha), "building_ha": f(z.building_ha), "mainline_track_ways": z.mainline_track_ways,
            "dead_ends": z.dead_ends, "hsr": z.hsr, "major": z.major,
            "rail_lines": rail, "platform_refs": refs(true), "platform_refs_other": refs(false),
            "line_count": lids.len(), "line_counts": Value::Object(by_type.into_iter().map(|(k, n)| (k, json!(n))).collect()), "line_ids": lids,
            "wikidata": if wd[i].is_empty() { Value::Null } else { json!([wd[i].iter().next().unwrap()]) }, "grouping": x.grouping,
            "source_ids": sources,
        });
        stations_out.push(json!({"type": "Feature", "properties": props, "geometry": {"type": "Point", "coordinates": unproj(&x.pt)}}));
    }
    let mut complexes_out = Vec::with_capacity(cplx.len());
    for (cid, idxs, biggest, how) in &cplx {
        let x = &st[*biggest];
        let (nm, tg) = stn_name(x);
        let mut lids: BTreeSet<&String> = BTreeSet::new();
        for &i in idxs.iter() {
            lids.extend(st_lines[i].iter().filter(|l| !l.is_empty()));
        }
        let modes: BTreeSet<&String> = idxs.iter().flat_map(|&i| st[i].modes.iter()).collect();
        let props = json!({
            "id": cid, "name": nm, "name_en": english_name(&merge(tg, name_tags(x))), "tier": idxs.iter().map(|&i| sizes[i].tier).min(),
            "terminus": idxs.iter().any(|&i| sizes[i].terminus), "station_count": idxs.len(), "modes": modes,
            "platform_count": idxs.iter().map(|&i| sizes[i].platform_count).sum::<i64>(), "line_count": lids.len(), "grouping": how,
            "station_ids": idxs.iter().map(|&i| sid_of(i)).collect::<Vec<_>>(),
        });
        complexes_out.push(json!({"type": "Feature", "properties": props, "geometry": {"type": "Point", "coordinates": unproj(&x.pt)}}));
    }
    let mut tiers: BTreeMap<i64, i64> = BTreeMap::new();
    for z in &sizes {
        *tiers.entry(z.tier).or_default() += 1;
    }
    let report = json!({
        "region": slug, "stations": st.len(), "complexes": cplx.len(), "merged_duplicates": st.iter().filter(|x| x.merged_from > 1).count(),
        "tiers": Value::Object(tiers.into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect()),
        "terminus_flagged": sizes.iter().filter(|z| z.terminus).count(),
        "stop_positions_unmapped": stop_unmapped, "stop_positions_total": inp.stops.len(),
        "routes_with_unmapped_stops": route_unmapped.len(), "routes_total": routes_total, "mapped_group_unions": mapped_pairs,
    });
    Out { stations: stations_out, complexes: complexes_out, route_order, report }
}
