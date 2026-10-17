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
const KEY_MERGE_RADIUS: f64 = 1000.0; // stations sharing a wikidata or uic_ref merge only within this distance
const KEY_MERGE_RADIUS_NAMED: f64 = 3000.0; // ... or within this one when their normalised names are equal
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
const STOPWORDS: [&str; 29] = ["gare", "du", "de", "d", "la", "le", "les", "l", "des", "station", "paris", "stazione", "railway", "rer", "metro",
    "subway", "bart", "caltrain", "amtrak", "light", "rail", "vta", "muni", "st", "saint", "bahnhof", "bhf", "di", ""];

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
        static RE: (Regex, Regex, Regex, Regex) = (
            Regex::new(r"\(.*?\)").unwrap(),
            Regex::new(r"[^\p{L}\p{N}_一-鿿]+").unwrap(),   // Python's \w: letters, numbers, underscore
            Regex::new(r"(地铁站|火车站|客站|站|駅)$").unwrap(),
            Regex::new(r"^jr([぀-ヿ一-鿿])").unwrap(), // "JR宝塚駅" is 宝塚
        );
    }
    if name.is_empty() {
        return String::new();
    }
    let s: String = name.to_lowercase().nfkd().filter(|c| canonical_combining_class(*c) == 0).collect();
    RE.with(|(paren, punct, tail, jr)| {
        let s = jr.replace(&s, "$1");
        let s = paren.replace_all(&s, " ");
        let s = punct.replace_all(&s, " ");
        let s = tail.replace(s.trim(), "");
        s.split_whitespace().filter(|t| !STOPWORDS.contains(t)).collect::<Vec<_>>().join(" ")
    })
}

/// the English label of a property map (rules::english_name_kv: name:en, int_name, a romanised name:* for a non-Latin name; else None)
pub fn english_name(t: &Map<String, Value>) -> Option<String> {
    crate::rules::english_name_kv(t.iter().filter_map(|(k, v)| Some((k.as_bytes(), v.as_str()?.as_bytes())))).and_then(|v| std::str::from_utf8(v).ok()).map(String::from)
}

pub fn ref_tokens(r: Option<&str>) -> BTreeSet<String> {
    r.unwrap_or("").split([';', ',', '/']).map(str::trim).filter(|t| !t.is_empty()).map(String::from).collect()
}

/// sort key of platform refs: numbers first, by value, then the rest
fn ref_key(r: &str) -> (bool, u128, String) {
    let num = !r.is_empty() && r.chars().all(|c| c.is_ascii_digit());
    (!num, if num { r.parse().unwrap_or(u128::MAX) } else { 0 }, r.to_string())
}

/// "1a" and "1b" are sections of platform 1: the platform's number is its digits when only one or two letters follow them
fn plat_key(t: &str) -> String {
    let d: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &t[d.len()..];
    if !d.is_empty() && (1..=2).contains(&rest.chars().count()) && rest.chars().all(|c| c.is_ascii_alphabetic()) { d } else { t.to_string() }
}

/// a bracket that names a line, mode or operator ("(National Rail)", "(Green line)", "(東武東上線)", "(A,C,E)"), not a place or a
/// direction ("(Cheshire)", "(West)", "(Beverly Hills)")
fn lineish(c: &str) -> bool {
    thread_local! {
        static RE: Regex = Regex::new(r"(?i)^(.*\b(line|ligne|linie|lijn)\b.*|m[eé]tro|u|s|u-?bahn|s-?bahn|tram|nr|national rail|underground|overground|rer|m\d+|.+線|.+线|国铁|國鐵|地铁|地鐵)$").unwrap();
        static LET: Regex = Regex::new(r"^[A-Z0-9]{1,2}([,/ ]+[A-Z0-9]{1,2})*$").unwrap();
    }
    let c = c.trim();
    RE.with(|r| r.is_match(c)) || LET.with(|r| r.is_match(c))
}

/// a station label cleaned of operator and format noise. Always: a leading "Bf. "/"Bhf. ", a trailing ", Bahnhof", the VBB form
/// "Berlin, U Voltastraße" / "Berlin, S+U Wedding". Only when the shorter name is the name of a station within GROUP_RADIUS
/// (`bare_near`, never region-wide: "103rd Street (Beverly Hills)" keeps its place): a trailing bracket that names a line, mode or
/// operator (`is_op` = an operator or network of this station), and the Korean type words 기차역/기차 ("서울역 기차역")
fn clean_name(name: &str, bare_near: &dyn Fn(&str) -> bool, is_op: &dyn Fn(&str) -> bool) -> String {
    thread_local! {
        static RE: (Regex, Regex) = (Regex::new(r"^[^,]+, (?:S\+U|U\+S|[US]) (\S.*)$").unwrap(), Regex::new(r"^(.*\S)\s*[(（]([^()（）]*)[)）]$").unwrap());
    }
    let mut n = name.trim();
    if let Some(r) = n.strip_prefix("Bf. ").or_else(|| n.strip_prefix("Bhf. ")) {
        n = r.trim();
    }
    if let Some(r) = n.strip_suffix(", Bahnhof") {
        n = r.trim();
    }
    let vbb;
    if let Some(c) = RE.with(|(v, _)| v.captures(n).map(|c| c[1].to_string())) {
        vbb = c;
        n = vbb.trim();
    }
    let mut cands: Vec<String> = Vec::new();
    if let Some((bare, inner)) = RE.with(|(_, p)| p.captures(n).map(|c| (c[1].to_string(), c[2].to_string()))) {
        if lineish(&inner) || is_op(inner.trim()) {
            cands.push(bare);
        }
    }
    let toks: Vec<&str> = n.split_whitespace().collect();
    if toks.len() >= 2 {
        if matches!(toks[toks.len() - 1], "기차역" | "기차") {
            cands.push(toks[..toks.len() - 1].join(" "));
        }
        if matches!(toks[0], "기차역" | "기차") {
            cands.push(toks[1..].join(" "));
        }
    }
    cands.into_iter().find(|c| bare_near(c)).unwrap_or_else(|| n.to_string())
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
    stated: bool, // some feature's tags state the mode
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
            if is_station && rw != Some("construction") {
                station_feats.push(it);
            }
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
    // (modes, whether the tags state them): without a mode tag the mode comes from the nearest track, a guess that platforms may correct
    let feature_modes = |it: &It| -> (BTreeSet<String>, bool) {
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
            ms.insert(nearest_track_mode(&it.g, 150.0).unwrap_or("train").to_string());
        }
        (ms, stated)
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
        let (modes, stated) = feature_modes(it);
        if let Some(&i) = sa.and_then(|a| by_area_ix.get(a)) {
            st[i].feats.push(k);
            if stated && !st[i].stated {
                st[i].modes.clear(); // a guessed mode gives way to a stated one
            }
            if stated || !st[i].stated {
                st[i].modes.extend(modes);
            }
            st[i].stated |= stated;
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
            stated,
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
    // a station whose mode is only a guess (nearest track, often the tram in front of Basel SBB) takes the modes of its own mapped
    // platforms: a main-line station with train platforms is a train station, not a tram stop
    for s_ in st.iter_mut().filter(|s_| !s_.stated) {
        let pm: BTreeSet<String> = s_.platforms.iter().flat_map(|&k| platforms[k].modes.iter().cloned()).filter(|m| matches!(m.as_str(), "train" | "subway" | "light_rail")).collect();
        if !pm.is_empty() {
            s_.modes = pm;
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
            // not railway:ref: it is a per-network abbreviation ("Ch" is Chociszew, Ciechanow, Chybie...), not an identifier
            for key in ["wikidata", "uic_ref"] {
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
    // a shared id only merges stations that are also close: ids are copied between far-apart objects (uic_ref in Russia, wikidata in
    // Poland), and an unlimited union fused up to 600 stations into one
    for idxs in key_index.values() {
        for (n, &a) in idxs.iter().enumerate() {
            for &b in &idxs[n + 1..] {
                if st[a].modes.is_disjoint(&st[b].modes) {
                    continue;
                }
                let d = geomx::dist_pt(&Geometry::Point(st[a].pt), &st[b].pt);
                if d <= KEY_MERGE_RADIUS || (d <= KEY_MERGE_RADIUS_NAMED && !st[a].norm.is_empty() && st[a].norm == st[b].norm) {
                    uf.union(a, b);
                }
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
    // a station area with no platforms, stop area or second feature (a building also tagged railway=station: 三次駅 beside the node 三次,
    // New York "Pennsylvania Station") joins the nearest same-mode station within 100 m of it, preferring one with platforms, when the
    // names agree once normalised or the area has no name; else (named differently) the one station with platforms inside the area
    for i in 0..st.len() {
        if !st[i].areas.is_empty() || st[i].feats.len() != 1 {
            continue;
        }
        let g = &station_feats[st[i].feats[0]].g;
        if !matches!(g, Geometry::Polygon(_) | Geometry::MultiPolygon(_)) {
            continue;
        }
        let mut cands: Vec<(bool, f64, usize)> = Vec::new();
        for j in s_tree.near(g, 100.0) {
            if j == i || st[j].modes.is_disjoint(&st[i].modes) {
                continue;
            }
            let d = geomx::dist(g, &Geometry::Point(st[j].pt));
            if d <= 100.0 {
                cands.push((st[j].platforms.is_empty(), d, j));
            }
        }
        cands.sort_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).unwrap());
        let inside: Vec<usize> = cands.iter().filter(|c| !c.0 && geomx::contains_pt(g, &st[c.2].pt)).map(|c| c.2).collect();
        let pick = cands.iter().find(|c| (st[i].norm.is_empty() && (st[i].platforms.is_empty() || st[c.2].platforms.is_empty())) || st[c.2].norm == st[i].norm).map(|c| c.2).or(if inside.len() == 1 && st[i].platforms.is_empty() { Some(inside[0]) } else { None });
        if let Some(j) = pick {
            uf.union(i, j);
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
            stated: ms.iter().any(|m| m.stated),
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
            // distinct platform numbers ("1a"/"1b" are one), plus every platform way without a ref
            let count = |ix: &[usize]| -> i64 {
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
            };
            let mainline_platforms = count(&mp);
            let platform_count = count(&x.platforms);
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
            }
            // no platform mapped, or none beside a mainline track (Nanjing South: one platform away from the tracks): look near the point
            // (only when no platform has a heavy-rail or heritage track beside it (a metro track does not count): a heritage station next to a main line stays without passenger track)
            let platform_by_track = mp.iter().any(|&k| track_tree.near(&platforms[k].g, 20.0).into_iter().any(|ti| matches!(track_mode(tracks[ti].p("type").unwrap_or("")), "train" | "other") && geomx::dist(&tracks[ti].g, &platforms[k].g) <= 20.0));
            if near.is_empty() && x.modes.contains("train") && !platform_by_track {
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
    // the operator's Wikidata id (operator:wikidata, else brand:wikidata) of the station's features: the logo key
    let op_wd: Vec<Option<String>> = st.iter().map(|x| ["operator:wikidata", "brand:wikidata"].iter().find_map(|key|
        x.feats.iter().find_map(|&k| station_feats[k].p(key).filter(|v| !v.is_empty()).map(String::from)))).collect();
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
    // is `cand` the name of a station within GROUP_RADIUS of x; is `c` an operator or network of x's features
    let bare_near = |x: &Station, cand: &str| -> bool {
        st_tree.near(&Geometry::Point(x.pt), GROUP_RADIUS).into_iter().any(|j| {
            geomx::dist_pt(&Geometry::Point(x.pt), &st[j].pt) <= GROUP_RADIUS
                && (st[j].feats.iter().any(|&k| station_feats[k].p("name") == Some(cand) || station_feats[k].p("name:en") == Some(cand))
                    || st[j].areas.iter().any(|a| area_rows.get(a).map_or(false, |r| r["tags"]["name"].as_str() == Some(cand))))
        })
    };
    let is_op = |x: &Station, c: &str| -> bool {
        x.feats.iter().any(|&k| ["operator", "network", "operator:short", "brand"].iter().any(|key| station_feats[k].p(key).map_or(false, |v| v.split(';').any(|t| t.trim() == c))))
    };
    let label = |x: &Station, n: &str| clean_name(n, &|c| bare_near(x, c), &|c| is_op(x, c));
    let stn_name = |x: &Station| -> (Option<String>, Map<String, Value>) {
        let best = x.feats.iter().copied().fold(None, |a: Option<usize>, k| {
            let rank = |k: usize| (station_feats[k].p("name").map_or(false, |v| !v.is_empty()), matches!(station_feats[k].g, Geometry::Point(_)));
            if a.map_or(true, |b| rank(k) > rank(b)) { Some(k) } else { a }
        }).unwrap();
        let own = station_feats[best].p("name").filter(|v| !v.is_empty());
        let area = x.areas.iter().find_map(|sa| {
            let r = area_rows.get(sa)?;
            Some((r["tags"]["name"].as_str().filter(|v| !v.is_empty())?, r["tags"].as_object().cloned().unwrap_or_default()))
        });
        // the station's own name, not the stop area's (a mislabelled relation named Poznań Główny "Potęgowo"); the area's name only when
        // the station has none, or when the area's name is the own name with a place in front ("Hannover Hauptbahnhof" for "Hauptbahnhof")
        match (own, area) {
            (Some(o), Some((a, at))) if a.to_lowercase().ends_with(&format!(" {}", o.to_lowercase())) => (Some(label(x, a)), at),
            (Some(o), _) => (Some(label(x, o)), station_feats[best].f.p.clone()),
            (None, Some((a, at))) => (Some(label(x, a)), at),
            (None, None) => (None, station_feats[best].f.p.clone()),
        }
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
            "id": sid_of(i), "complex": complex_of[i], "name": nm, "name_en": english_name(&merge(tg, name_tags(x))).map(|e| label(x, &e)),
            "modes": x.modes.iter().collect::<Vec<_>>(), "tier": z.tier, "score": f(z.score), "terminus": z.terminus,
            "mainline_platforms": z.mainline_platforms, "platform_count": z.platform_count,
            "land_ha": f(z.land_ha), "building_ha": f(z.building_ha), "mainline_track_ways": z.mainline_track_ways,
            "dead_ends": z.dead_ends, "hsr": z.hsr, "major": z.major,
            "rail_lines": rail, "platform_refs": refs(true), "platform_refs_other": refs(false),
            "line_count": lids.len(), "line_counts": Value::Object(by_type.into_iter().map(|(k, n)| (k, json!(n))).collect()), "line_ids": lids,
            "operator_wd": op_wd[i], "wikidata": if wd[i].is_empty() { Value::Null } else { json!([wd[i].iter().next().unwrap()]) }, "grouping": x.grouping,
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
            "id": cid, "name": nm, "name_en": english_name(&merge(tg, name_tags(x))).map(|e| label(x, &e)), "tier": idxs.iter().map(|&i| sizes[i].tier).min(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_sections_count_once() {
        assert_eq!(plat_key("1a"), "1");
        assert_eq!(plat_key("12B"), "12");
        assert_eq!(plat_key("3"), "3");
        assert_eq!(plat_key("A"), "A");
        assert_eq!(plat_key("1north"), "1north");
    }

    #[test]
    fn labels_lose_noise_only_beside_the_bare_station() {
        let near = |c: &str| ["Victoria", "池袋", "서울역", "Goregaon", "Baruth"].contains(&c);
        let op = |c: &str| c == "Elizabeth";
        let cn = |n: &str| clean_name(n, &near, &op);
        assert_eq!(cn("Bf. Dresden Hbf"), "Dresden Hbf");
        assert_eq!(cn("Dießen, Bahnhof"), "Dießen");
        assert_eq!(cn("Berlin, U Voltastraße"), "Voltastraße");
        assert_eq!(cn("Berlin, S+U Wedding"), "Wedding");
        assert_eq!(cn("Victoria (National Rail)"), "Victoria");
        assert_eq!(cn("池袋 (東武東上線)"), "池袋");
        assert_eq!(cn("서울역 기차역"), "서울역");
        assert_eq!(cn("Victoria (Elizabeth)"), "Victoria");
        // places and directions are not lines; the bare station must be near
        assert_eq!(cn("Goregaon (East)"), "Goregaon (East)");
        assert_eq!(cn("Baruth (Mark)"), "Baruth (Mark)");
        assert_eq!(cn("103rd Street (Beverly Hills)"), "103rd Street (Beverly Hills)");
        assert_eq!(cn("Adlington (Cheshire)"), "Adlington (Cheshire)");
        assert_eq!(cn("Liverpool Street (Elizabeth line)"), "Liverpool Street (Elizabeth line)");
        assert_eq!(cn("기차 부전역"), "기차 부전역");
    }

    #[test]
    fn japanese_and_german_suffixes_normalise() {
        assert_eq!(norm_name("三次駅"), norm_name("三次"));
        assert_eq!(norm_name("Bahnhof Mittweida"), norm_name("Mittweida"));
        assert_eq!(norm_name("JR宝塚駅"), norm_name("宝塚"));
        assert_eq!(norm_name("Stazione di Oristano"), norm_name("Oristano"));
    }

    #[test]
    fn english_name_uses_the_shared_rules() {
        let mut t = Map::new();
        t.insert("name".into(), json!("Hauptbahnhof"));
        t.insert("name:et".into(), json!("Waterloo metroojaam"));
        t.insert("name:etymology:wikidata".into(), json!("Q735271"));
        assert_eq!(english_name(&t), None);
        t.insert("name:en".into(), json!("Central Station"));
        assert_eq!(english_name(&t).as_deref(), Some("Central Station"));
    }
}
