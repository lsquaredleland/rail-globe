//! Rail type classification (docs/01-features.md, section 2), ported from pipeline/classify.py (removed 2026-10-12) and run at
//! the end of every cut:
//! the cut already holds the features, relations and tags in memory, so no JSON is written and read back in between.
//!
//! Writes data/<slug>/features.typed.geojsonl (features.geojsonl with `construction_type`, `type`, `type_source`, `own_type`
//! added to track), lines.json (one record per line: route_master, else route) and classify_report.json, in the same bytes as
//! classify.py wrote them. Per-region rules stay in pipeline/config/<slug>.json.

use crate::out::{js, py_str, J};
use crate::region::Region;
use crate::rules::{List, Tags};
use regex::Regex;
use rustc_hash::FxHashSet;
use serde_json::Value;
use std::path::Path;

/// most prominent first; track takes the highest type that runs on it (regional includes commuter, merged 2026-10-12)
const ORDER: [&str; 8] = ["high_speed", "intercity", "regional", "metro", "light_rail", "monorail", "funicular", "other"];
const TRACK_RW: [&str; 9] = ["rail", "subway", "light_rail", "tram", "monorail", "funicular", "narrow_gauge", "construction", "proposed"];
const SERVICE_TRACK: [&str; 4] = ["yard", "siding", "spur", "crossover"];
const FREIGHT_USAGE: [&str; 4] = ["industrial", "freight", "military", "farm"];

fn rank(t: &str) -> usize {
    ORDER.iter().position(|x| *x == t).unwrap_or(ORDER.len())
}

fn route_mode_type(m: &str) -> Option<&'static str> {
    Some(match m {
        "subway" => "metro",
        "light_rail" | "tram" => "light_rail",
        "monorail" => "monorail",
        "funicular" => "funicular",
        _ => return None,
    })
}

fn railway_type(rw: &str) -> Option<&'static str> {
    route_mode_type(rw).or(if rw == "narrow_gauge" { Some("other") } else { None })
}

fn service_type(s: &str) -> Option<&'static str> {
    Some(match s {
        "high_speed" => "high_speed",
        "long_distance" | "night" => "intercity",
        "regional" | "commuter" => "regional",
        "tourism" => "other",
        _ => return None,
    })
}

/// string tag access for the rules
pub trait Get {
    fn g(&self, k: &str) -> Option<&str>;
}

/// relation side-table tags (owned, in side-table order)
pub struct Owned(pub Vec<(String, String)>);

impl Get for Owned {
    fn g(&self, k: &str) -> Option<&str> {
        self.0.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }
}

/// a track feature's properties: its tags plus `osm_id`
pub struct FeatProps<'a> {
    pub tags: List<'a>,
    pub osm_id: String,
}

impl Get for FeatProps<'_> {
    fn g(&self, k: &str) -> Option<&str> {
        if k == "osm_id" {
            return Some(&self.osm_id);
        }
        self.tags.get(k.as_bytes()).and_then(|v| std::str::from_utf8(v).ok())
    }
}

/// tags merged for a line override: the route's tags, overridden by the line source's ({**route, **src} in classify.py)
struct Merged<'a>(&'a Owned, &'a Owned);

impl Get for Merged<'_> {
    fn g(&self, k: &str) -> Option<&str> {
        self.1.g(k).or_else(|| self.0.g(k))
    }
}

/// classify.py matches(): every key of the rule must match
struct Rule {
    network: Option<String>,
    network_re: Option<Regex>,
    name_re: Option<Regex>,
    operator_re: Option<Regex>,
    osm_id: Option<Vec<String>>,
}

impl Rule {
    fn parse(v: &Value) -> Rule {
        let mut r = Rule { network: None, network_re: None, name_re: None, operator_re: None, osm_id: None };
        let re = |x: &Value| Regex::new(x.as_str().expect("pattern")).unwrap_or_else(|e| panic!("bad pattern in config: {e}"));
        for (k, x) in v.as_object().expect("match rule") {
            match k.as_str() {
                "network" => r.network = Some(x.as_str().expect("network").to_string()),
                "network_re" => r.network_re = Some(re(x)),
                "name_re" => r.name_re = Some(re(x)),
                "operator_re" => r.operator_re = Some(re(x)),
                "osm_id" => {
                    r.osm_id = Some(match x {
                        Value::String(s) => vec![s.clone()],
                        Value::Array(a) => a.iter().filter_map(|s| s.as_str().map(String::from)).collect(),
                        _ => panic!("osm_id must be a string or a list"),
                    })
                }
                other => panic!("unknown match key {other} in config"),
            }
        }
        r
    }
    fn matches<T: Get + ?Sized>(&self, t: &T) -> bool {
        (self.network.as_deref().map_or(true, |n| t.g("network") == Some(n)))
            && self.network_re.as_ref().map_or(true, |r| r.is_match(t.g("network").unwrap_or("")))
            && self.name_re.as_ref().map_or(true, |r| r.is_match(t.g("name").unwrap_or("")))
            && self.operator_re.as_ref().map_or(true, |r| r.is_match(t.g("operator").unwrap_or("")))
            && self.osm_id.as_ref().map_or(true, |ids| t.g("osm_id").map_or(false, |o| ids.iter().any(|i| i == o)))
    }
}

/// pipeline/config/<slug>.json, as classify.py reads it
pub struct Cfg {
    china: bool,
    type_overrides: Vec<(Rule, String)>,
    track_type_overrides: Vec<(Rule, String)>,
    hs_track_overrides: Vec<Rule>,
    hs_networks: Vec<Regex>,
    unrouted_main_type: String,
    /// `high_speed_min_kmh`: when set, track is high-speed only if its own maxspeed reaches this (route-derived high speed included)
    hs_min_kmh: Option<f64>,
    /// `freight_name_re`: an unrouted Intercity/Regional railway with a name matching this and no passenger station beside it is Freight
    freight_name: Option<Regex>,
    /// `freight_operator_exempt_re`: ways whose operator matches (passenger agencies) are never made Freight by the name rule
    freight_exempt: Option<Regex>,
    /// `freight_name_per_way`: judge each way of a matching railway by the stations beside that way alone, not by the whole named railway (Mexico)
    freight_name_per_way: bool,
    /// `freight_unrouted_no_station` (pass 2, E1): an unrouted Intercity/Regional way typed from its own tags becomes Freight unless a passenger
    /// station serves its railway within NAME_SHIELD_KM, named or not
    freight_unrouted_no_station: bool,
    /// `freight_isolated_km` (pass 2, E2): connected passenger-typed track under this many km with no route, no station and no exempt operator is Freight
    freight_isolated_km: Option<f64>,
    /// `passenger_station_level` (pass 2): 0 strict (public_transport=station, train=yes, uic_ref; default), 1 also station=train or an operator or network
    /// tag, 2 every railway=station|halt|public_transport=station not tagged freight. Only for stations the freight rules look at.
    station_level: u8,
    proposed: Option<Region>,
    /// names of the stations that carry passengers (China: the 12306 ticketing list); None: judge a station by its tags
    passenger_names: Option<std::collections::HashSet<String>>,
}

impl Cfg {
    /// Is this station (its name, and whether its tags say passengers) a passenger station? With a passenger list the list decides;
    /// without one the tags do.
    pub fn passenger_station(&self, name: Option<&str>, tags_say_passengers: bool) -> bool {
        match &self.passenger_names {
            Some(names) => name.map_or(false, |n| names.contains(n.trim().trim_end_matches('站'))),
            None => tags_say_passengers,
        }
    }

    /// With `high_speed_min_kmh` set: does this maxspeed reach it? Without it every way passes.
    pub fn hs_fast(&self, maxspeed: Option<&str>) -> bool {
        self.hs_min_kmh.map_or(true, |m| at_least(speed_kmh(maxspeed), m))
    }

    pub fn station_level(&self) -> u8 {
        self.station_level
    }

    pub fn load(path: &Path, regions_dir: &Path) -> Cfg {
        let mut v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display())))
            .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        // experiments: RAIL_CFG_SET='{"freight_isolated_km": 10}' replaces keys of every config loaded (measuring options without editing configs)
        if let Ok(o) = std::env::var("RAIL_CFG_SET") {
            let o: Value = serde_json::from_str(&o).expect("RAIL_CFG_SET: a JSON object");
            for (k, x) in o.as_object().expect("RAIL_CFG_SET: a JSON object") {
                v[k] = x.clone();
            }
        }
        let truthy = |x: &Value| match x {
            Value::Bool(b) => *b,
            Value::Number(n) => n.as_f64().map_or(false, |f| f != 0.0),
            Value::String(s) => !s.is_empty(),
            Value::Array(a) => !a.is_empty(),
            Value::Object(o) => !o.is_empty(),
            Value::Null => false,
        };
        let overrides = |k: &str| -> Vec<(Rule, String)> {
            v.get(k).and_then(Value::as_array).map_or(Vec::new(), |a| {
                a.iter().map(|o| (Rule::parse(&o["match"]), o["type"].as_str().expect("override type").to_string())).collect()
            })
        };
        // proposed track is only kept inside this area: a Nominatim dump ([{..., "geojson": geometry}]) in pipeline/regions/
        let proposed = v.get("proposed_region").and_then(Value::as_str).filter(|s| !s.is_empty()).map(|r| {
            let p = regions_dir.join(r);
            let d: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display())))
                .expect("parse proposed_region");
            Region::new(r, crate::region::rings_of(&d[0]["geojson"]))
        });
        let passenger_names = v.get("passenger_stations_file").and_then(Value::as_str).map(|f| {
            let p = path.parent().unwrap_or(Path::new(".")).join(f);
            std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display())).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
        });
        Cfg {
            passenger_names,
            china: v.get("china").map_or(false, truthy),
            type_overrides: overrides("type_overrides"),
            track_type_overrides: overrides("track_type_overrides"),
            hs_track_overrides: v.get("high_speed_track_overrides").and_then(Value::as_array).map_or(Vec::new(), |a| a.iter().map(Rule::parse).collect()),
            hs_networks: v.get("high_speed_networks_re").and_then(Value::as_array).map_or(Vec::new(), |a| {
                a.iter().map(|p| Regex::new(p.as_str().expect("pattern")).expect("high_speed_networks_re")).collect()
            }),
            unrouted_main_type: v.get("unrouted_main_type").and_then(Value::as_str).unwrap_or("intercity").to_string(),
            hs_min_kmh: v.get("high_speed_min_kmh").map(|x| match x {
                Value::Number(n) => n.as_f64().expect("high_speed_min_kmh"),
                Value::String(s) => s.trim().parse().unwrap_or_else(|_| panic!("high_speed_min_kmh: not a number: {s}")),
                _ => panic!("high_speed_min_kmh must be a number"),
            }),
            freight_exempt: v.get("freight_operator_exempt_re").and_then(Value::as_str).map(|p| Regex::new(p).expect("freight_operator_exempt_re")),
            freight_name_per_way: v.get("freight_name_per_way").map_or(false, truthy),
            freight_unrouted_no_station: v.get("freight_unrouted_no_station").map_or(false, truthy),
            freight_isolated_km: v.get("freight_isolated_km").and_then(Value::as_f64),
            station_level: match v.get("passenger_station_level").and_then(Value::as_str) {
                None | Some("strict") => 0,
                Some("operator") => 1,
                Some("any") => 2,
                Some(o) => panic!("passenger_station_level: strict, operator or any, not {o}"),
            },
            freight_name: v.get("freight_name_re").and_then(Value::as_str).map(|p| Regex::new(p).expect("freight_name_re")),
            proposed,
        }
    }
}

/// classify.py speed_kmh: leading number (any Unicode decimal digits, as Python's \d), optional "mph"; None without a number
pub fn speed_kmh(v: Option<&str>) -> Option<f64> {
    let s = v?.trim_start();
    let digit = |c: char| -> Option<u32> {
        let base = [0x30, 0x660, 0x6F0, 0x7C0, 0x966, 0x9E6, 0xA66, 0xAE6, 0xB66, 0xBE6, 0xC66, 0xCE6, 0xD66, 0xE50, 0xED0, 0xF20, 0x1040, 0xFF10];
        let u = c as u32;
        base.iter().find(|&&b| u >= b && u < b + 10).map(|&b| u - b)
    };
    let mut n = 0f64;
    let mut len = 0;
    for c in s.chars() {
        match digit(c) {
            Some(d) => {
                n = n * 10.0 + d as f64;
                len += c.len_utf8();
            }
            None => break,
        }
    }
    if len == 0 {
        return None;
    }
    Some(if s[len..].trim_start().starts_with("mph") { n * 1.609 } else { n })
}

/// Python truthiness of a speed: None and 0 are false
fn at_least(ms: Option<f64>, v: f64) -> bool {
    ms.map_or(false, |m| m != 0.0 && m >= v)
}

fn highest<'a>(types: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    types.filter(|t| !t.is_empty()).min_by_key(|t| rank(t))
}

/// type of one route relation: (type, source); first matching rule wins
fn route_type(t: &Owned, cfg: &Cfg, track_hs: bool, allow_hs: bool) -> (String, &'static str) {
    let (ty, src) = route_type_any(t, cfg, track_hs);
    // high_speed_min_kmh and not enough of the route is fast enough: Intercity, the next type down. (With the option set the track-tag step is
    // off, so a route is High-speed only by network, service or an override.)
    if ty == "high_speed" && !allow_hs {
        return ("intercity".into(), src);
    }
    (ty, src)
}

fn route_type_any(t: &Owned, cfg: &Cfg, track_hs: bool) -> (String, &'static str) {
    for (r, ty) in &cfg.type_overrides {
        if r.matches(t) {
            return (ty.clone(), "override");
        }
    }
    if let Some(m) = t.g("route").and_then(route_mode_type) {
        return (m.into(), "mode");
    }
    let net = t.g("network").unwrap_or("");
    let svc = t.g("service");
    if let Some(s) = svc.and_then(service_type) {
        return (s.into(), "service");
    }
    let hs_net = cfg.hs_networks.iter().any(|r| r.is_match(net));
    if matches!(svc, Some("national" | "international")) {
        return ((if hs_net { "high_speed" } else { "intercity" }).into(), "service+network");
    }
    if hs_net {
        return ("high_speed".into(), "network");
    }
    if cfg.china {
        let r = t.g("ref").unwrap_or("");
        let mut ch = r.chars();
        if ch.next() == Some('C') && ch.next().map_or(false, |c| !c.is_whitespace() && speed_kmh(Some(&c.to_string())).is_some()) {
            return ("intercity".into(), "train_number");
        }
    }
    if track_hs {
        return ("high_speed".into(), "track");
    }
    ("regional".into(), "default")
}

/// track type from its own tags only
fn own_tag_type<T: Get + ?Sized>(p: &T, cfg: &Cfg) -> (String, &'static str) {
    let mut rw = p.g("railway").map(str::to_string);
    if matches!(rw.as_deref(), Some("construction" | "proposed")) {
        let mode = p.g(rw.as_deref().unwrap()).map(str::to_string); // the mode lives in construction=* / proposed=*
        rw = match mode {
            Some(m) => Some(m),
            None => {
                let truthy = |k: &str| p.g(k).map_or(false, |v| !v.is_empty());
                if !(truthy("usage") || truthy("electrified") || truthy("gauge")) {
                    return ("other".into(), "default");
                }
                Some("rail".into())
            }
        };
    }
    let rw = rw.unwrap_or_default();
    if let Some(t) = railway_type(&rw) {
        return (t.into(), "tags");
    }
    if rw != "rail" && rw != "narrow_gauge" {
        return ("other".into(), "default");
    }
    if p.g("usage") == Some("tourism") {
        return ("other".into(), "tags");
    }
    let ms = speed_kmh(p.g("maxspeed"));
    let name = p.g("name").unwrap_or("");
    if cfg.china {
        if cfg.hs_track_overrides.iter().any(|r| r.matches(p)) {
            return ("high_speed".into(), "override");
        }
        let lower = name.to_lowercase();
        let hs_name = name.contains("高速铁路") || name.contains("高速鐵路") || lower.contains("high speed rail") || lower.contains("high-speed rail")
            || lower.contains("express rail link");
        if at_least(ms, 250.0) || name.contains("高速线") || name.contains("客专") || name.contains("客运专线") || hs_name {
            return ("high_speed".into(), "tags");
        }
        if p.g("highspeed") == Some("yes") || name.contains("城际") || at_least(ms, 200.0) {
            return ("intercity".into(), "tags");
        }
    } else if (p.g("highspeed") == Some("yes") && cfg.hs_fast(p.g("maxspeed"))) || at_least(ms, cfg.hs_min_kmh.map_or(200.0, |m| m.max(200.0))) {
        return ("high_speed".into(), "tags");
    }
    let usage = p.g("usage");
    if usage.map_or(false, |u| FREIGHT_USAGE.contains(&u)) || p.g("railway:traffic_mode") == Some("freight") {
        return ("freight".into(), "tags");
    }
    if usage == Some("main") {
        return (cfg.unrouted_main_type.clone(), "tags");
    }
    if usage == Some("branch") {
        return ("regional".into(), "tags");
    }
    ("regional".into(), "default")
}

const PASSENGER_NAME_WORDS: [&str; 9] = ["联络线", "直通线", "环线", "高铁", "高速", "客专", "客运专线", "城际", "动车"];

struct Decided {
    t: String,
    s: &'static str,
    own: Option<String>,
}
/// with `high_speed_min_kmh`: the share of a route's track that must reach it for the route to be High-speed (Acela 41%, SEPTA Trenton 26%)
const HS_MIN_SHARE: f64 = 0.35;
const SNIPPET_KM: f64 = 2.0; // a piece of track shorter than this that differs from all its neighbours takes their type

fn ll_km(ll: &[(i32, i32)]) -> f64 {
    let pts: Vec<(f64, f64)> = ll.iter().filter(|p| p.0 != i32::MIN).map(|p| (p.1 as f64 / 1e7, p.0 as f64 / 1e7)).collect();
    crate::tiles::haversine_len(&pts)
}

const PAX_NEAR_M: f64 = 200.0; // a passenger station this close to a track (to the line between its points, not just its points) serves it
const SHORT_SPUR_KM: f64 = 3.0;
const MAX_FREIGHT_BRANCH_KM: f64 = 15.0; // a longer branch with no station is as likely a passenger line whose stations are not mapped (or not near)
const ROUTED_BESIDE_M: f64 = 20.0; // unrouted track this close to passenger-route track, along its length, is part of the same corridor (name rule)
const NAME_SHIELD_KM: f64 = 5.0; // a passenger station of the same railway name this close keeps an unrouted way passenger (name rule)
const JUNCTION_CLEAR_M: f64 = 800.0; // a station this close to where the railway joins another track belongs to that track

fn deg(p: (i32, i32)) -> (f64, f64) {
    (p.1 as f64 / 1e7, p.0 as f64 / 1e7) // (lon, lat)
}

/// metres between two (lon, lat) points, on a plane around their mean latitude
fn dist_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let c = ((a.1 + b.1) / 2.0).to_radians().cos();
    let (ax, ay, bx, by) = (a.0 * 111_320.0 * c, a.1 * 110_540.0, b.0 * 111_320.0 * c, b.1 * 110_540.0);
    ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt()
}

/// a way's points every `step_m` metres or so
fn densify(ll: &[(i32, i32)], step_m: f64) -> Vec<(f64, f64)> {
    let pts: Vec<(f64, f64)> = ll.iter().filter(|p| p.0 != i32::MIN).map(|&p| deg(p)).collect();
    let mut v = Vec::new();
    for w in pts.windows(2) {
        let n = ((dist_m(w[0], w[1]) / step_m).ceil() as usize).max(1);
        for k in 0..n {
            let f = k as f64 / n as f64;
            v.push((w[0].0 + (w[1].0 - w[0].0) * f, w[0].1 + (w[1].1 - w[0].1) * f));
        }
    }
    if let Some(&l) = pts.last() {
        v.push(l);
    }
    v
}

/// Passenger-route track as points every ~20 m (for "runs beside it") and as exact vertices (for "touches it"): protects the unrouted track
/// that belongs to a passenger corridor from the evidence rules.
struct Beside {
    rgrid: rustc_hash::FxHashMap<(i32, i32), Vec<(f64, f64)>>,
    rverts: FxHashSet<(i32, i32)>,
}

impl Beside {
    fn new(tracks: &[TrackIn], routed: &[bool]) -> Beside {
        let rcell = |p: (f64, f64)| ((p.1 / 0.0005).floor() as i32, (p.0 / 0.0005).floor() as i32);
        let mut b = Beside { rgrid: Default::default(), rverts: Default::default() };
        for (_, t) in tracks.iter().enumerate().filter(|(i, _)| routed[*i]) {
            b.rverts.extend(t.ll.iter().copied());
            for p in densify(&t.ll, 20.0) {
                b.rgrid.entry(rcell(p)).or_default().push(p);
            }
        }
        b
    }

    /// beside a passenger route: most of the way (leaving 100 m at each end, where branches leave; a way under 200 m: all of it) is within
    /// ROUTED_BESIDE_M of it; or in a gap of one: both ends touch passenger-route track
    fn is_beside(&self, tracks: &[TrackIn], i: usize) -> bool {
        let rcell = |p: (f64, f64)| ((p.1 / 0.0005).floor() as i32, (p.0 / 0.0005).floor() as i32);
        let near_routed = |p: (f64, f64)| {
            let c = rcell(p);
            (-1..=1).any(|dy| (-2..=2).any(|dx| self.rgrid.get(&(c.0 + dy, c.1 + dx)).map_or(false, |v| v.iter().any(|&q| dist_m(p, q) <= ROUTED_BESIDE_M))))
        };
        let t = &tracks[i];
        if t.ll.len() < 2 {
            return false;
        }
        if t.ll.iter().all(|p| p.0 != i32::MIN) && self.rverts.contains(&t.ll[0]) && self.rverts.contains(&t.ll[t.ll.len() - 1]) {
            return true;
        }
        let pts = densify(&t.ll, 30.0);
        let real: Vec<&(i32, i32)> = t.ll.iter().filter(|p| p.0 != i32::MIN).collect(); // missing nodes are gap markers
        let (Some(&&first), Some(&&last)) = (real.first(), real.last()) else { return false };
        let (a, b) = (deg(first), deg(last));
        let mut inner: Vec<&(f64, f64)> = pts.iter().filter(|&&p| dist_m(p, a) > 100.0 && dist_m(p, b) > 100.0).collect();
        if inner.is_empty() {
            inner = pts.iter().collect(); // a short way (a crossover, a link): all of it
        }
        inner.iter().filter(|&&&p| near_routed(p)).count() * 5 >= inner.len() * 4
    }
}

/// Unrouted main-line and branch track that no passenger train can use, found from what is (not) beside it. Returns the track indices to
/// type as Freight, with the reason. A railway (its ways with the same name and ref; unnamed ways joined end to end) is freight when
/// (1) it is a branch (it has a dead end) that no passenger station serves, or (2) it is a short spur (under SHORT_SPUR_KM) with a dead end that has no passenger station
/// beside it. A station serves the railway of the track nearest to it (within PAX_NEAR_M), so a branch beside a main line does not get the
/// main line's stations. Only ways that would otherwise
/// be Intercity or Regional from their own tags, with no passenger route and no passenger traffic tag, are looked at.
fn freight_by_evidence(tracks: &[TrackIn], cand: &[bool], pax: &[(i32, i32)], name_re: Option<&Regex>, exempt_re: Option<&Regex>, beside: Option<&Beside>, per_way: bool, unrouted_no_station: bool) -> rustc_hash::FxHashMap<usize, &'static str> {
    use rustc_hash::FxHashMap;
    let deg = |p: (i32, i32)| (p.1 as f64 / 1e7, p.0 as f64 / 1e7); // (lon, lat)
    // metres on a plane around `lat0`
    let xy = |p: (f64, f64), lat0: f64| ((p.0) * 111_320.0 * lat0.to_radians().cos(), p.1 * 110_540.0);
    let cell = |p: (f64, f64)| ((p.1 / 0.01).floor() as i32, (p.0 / 0.01).floor() as i32);
    let stations: Vec<(f64, f64)> = pax.iter().filter(|p| p.0 != i32::MIN).map(|&p| deg(p)).collect();
    let mut grid: FxHashMap<(i32, i32), Vec<usize>> = FxHashMap::default();
    for (i, &p) in stations.iter().enumerate() {
        grid.entry(cell(p)).or_default().push(i);
    }
    let dist_m = |a: (f64, f64), b: (f64, f64)| {
        let l0 = (a.1 + b.1) / 2.0;
        let (ax, ay) = xy(a, l0);
        let (bx, by) = xy(b, l0);
        ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt()
    };
    let seg_dist_m = |p: (f64, f64), a: (f64, f64), b: (f64, f64)| {
        let (px, py) = xy(p, p.1);
        let (ax, ay) = xy(a, p.1);
        let (bx, by) = xy(b, p.1);
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let t = if len2 == 0.0 { 0.0 } else { (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0) };
        ((px - (ax + t * dx)).powi(2) + (py - (ay + t * dy)).powi(2)).sqrt()
    };
    // Each station belongs to the track nearest to it (not a service track: yards and sidings crowd the stations), within PAX_NEAR_M: a
    // branch that runs beside a main line does not get the main line's stations.
    let mut owner: Vec<Option<(f64, usize)>> = vec![None; stations.len()];
    for (ti, t) in tracks.iter().enumerate() {
        if t.props.g("railway") != Some("rail") || t.props.g("service").map_or(false, |v| SERVICE_TRACK.contains(&v)) {
            continue;
        }
        let pts: Vec<(f64, f64)> = t.ll.iter().filter(|p| p.0 != i32::MIN).map(|&p| deg(p)).collect();
        for w in pts.windows(2) {
            let n = ((dist_m(w[0], w[1]) / 400.0).ceil() as usize).max(1);
            let mut seen: Vec<(i32, i32)> = Vec::new();
            for k in 0..=n {
                let f = k as f64 / n as f64;
                let c = cell((w[0].0 + (w[1].0 - w[0].0) * f, w[0].1 + (w[1].1 - w[0].1) * f));
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let key = (c.0 + dy, c.1 + dx);
                        if seen.contains(&key) {
                            continue;
                        }
                        seen.push(key);
                        for &si in grid.get(&key).into_iter().flatten() {
                            let d = seg_dist_m(stations[si], w[0], w[1]);
                            if d <= PAX_NEAR_M && owner[si].map_or(true, |(bd, _)| d < bd) {
                                owner[si] = Some((d, ti));
                            }
                        }
                    }
                }
            }
        }
    }
    let mut owned: FxHashMap<usize, Vec<usize>> = FxHashMap::default(); // track -> the stations that belong to it
    for (si, o) in owner.iter().enumerate() {
        if let Some((_, ti)) = o {
            owned.entry(*ti).or_default().push(si);
        }
    }
    // passenger stations beside the ways of each railway name, routed or not (an Amtrak stop on a subdivision shields the unrouted ways near it)
    let mut name_stations: FxHashMap<&str, Vec<usize>> = FxHashMap::default();
    for (&ti, sis) in &owned {
        if let Some(n) = tracks[ti].props.g("name").filter(|n| !n.is_empty()) {
            name_stations.entry(n).or_default().extend(sis.iter().copied());
        }
    }
    // how many ways end at each point, over all track: an end no other way touches is a dead end
    let mut ends: FxHashMap<(i32, i32), u32> = FxHashMap::default();
    for t in tracks {
        if let (Some(&a), Some(&b)) = (t.ll.first(), t.ll.last()) {
            *ends.entry(a).or_default() += 1;
            *ends.entry(b).or_default() += 1;
        }
    }
    // unnamed ways without a ref are joined end to end into one railway
    let mut parent: Vec<usize> = (0..tracks.len()).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let anon = |t: &TrackIn| t.props.g("name").map_or(true, str::is_empty) && t.props.g("ref").map_or(true, str::is_empty);
    let mut at: FxHashMap<(i32, i32), usize> = FxHashMap::default();
    for (i, t) in tracks.iter().enumerate() {
        if !cand[i] || !anon(t) {
            continue;
        }
        for e in [t.ll.first(), t.ll.last()].into_iter().flatten() {
            match at.get(e) {
                Some(&j) => {
                    let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                    if a != b {
                        parent[a.max(b)] = a.min(b);
                    }
                }
                None => {
                    at.insert(*e, i);
                }
            }
        }
    }
    let mut groups: FxHashMap<String, Vec<usize>> = FxHashMap::default();
    for (i, t) in tracks.iter().enumerate() {
        if !cand[i] {
            continue;
        }
        let key = if anon(t) { format!("a:{}", find(&mut parent, i)) } else { format!("n:{}|{}", t.props.g("name").unwrap_or(""), t.props.g("ref").unwrap_or("")) };
        groups.entry(key).or_default().push(i);
    }
    let len_km = |ll: &[(i32, i32)]| -> f64 {
        let pts: Vec<(f64, f64)> = ll.iter().filter(|p| p.0 != i32::MIN).map(|&p| deg(p)).collect();
        crate::tiles::haversine_len(&pts)
    };
    // a way's ends and middle, where a station's shield is measured from
    let way_pts = |i: usize| -> Vec<(f64, f64)> {
        [tracks[i].ll.first(), tracks[i].ll.get(tracks[i].ll.len() / 2), tracks[i].ll.last()].into_iter().flatten().filter(|p| p.0 != i32::MIN).map(|&p| deg(p)).collect()
    };
    let mut out = FxHashMap::default();
    for members in groups.values() {
        // the ends of the railway as a whole: a dead end (a terminus), or a junction with another track (a station there belongs to the line
        // it joins: a freight branch starts in a passenger station's yard)
        let mut own_ends: FxHashMap<(i32, i32), u32> = FxHashMap::default();
        for &i in members {
            for e in [tracks[i].ll.first(), tracks[i].ll.last()].into_iter().flatten() {
                *own_ends.entry(*e).or_default() += 1;
            }
        }
        let chain_ends: Vec<((i32, i32), bool)> = own_ends.iter().filter(|(e, n)| e.0 != i32::MIN && **n == 1).map(|(e, _)| (*e, ends.get(e).copied().unwrap_or(0) > 1)).collect();
        let junctions: Vec<(f64, f64)> = chain_ends.iter().filter(|c| c.1).map(|c| deg(c.0)).collect();
        let group_stations: Vec<usize> = members.iter().flat_map(|i| owned.get(i).into_iter().flatten().copied()).collect();
        let served = group_stations.iter().any(|&si| junctions.iter().all(|&j| dist_m(stations[si], j) > JUNCTION_CLEAR_M));
        let dead_ends: Vec<(f64, f64)> = chain_ends.iter().filter(|c| !c.1).map(|c| deg(c.0)).collect();
        // Only a branch is judged: it has a free end (a dead end). A through piece with no station of its own, joined to track at both ends,
        // is most likely a stretch of a passenger line whose stations the neighbouring ways own.
        let reason = if dead_ends.is_empty() {
            None
        } else if !served && members.iter().map(|&i| len_km(&tracks[i].ll)).sum::<f64>() < MAX_FREIGHT_BRANCH_KM {
            Some("no-station")
        } else {
            // a short spur whose dead end has no station of its own
            let km: f64 = members.iter().map(|&i| len_km(&tracks[i].ll)).sum();
            let end_served = |d: (f64, f64)| group_stations.iter().any(|&si| dist_m(stations[si], d) <= PAX_NEAR_M * 3.0);
            (km < SHORT_SPUR_KM && dead_ends.iter().any(|&d| !end_served(d))).then_some("short-spur")
        };
        // a railway named like a freight line (Subdivision, District, ...): each of its unrouted ways is freight unless a passenger station of that
        // name is within NAME_SHIELD_KM of it (stations at a junction of the railway, within JUNCTION_CLEAR_M, do not count), any length, branch or not
        let first = tracks[members[0]].props.g("name");
        if reason.is_none() && first.map_or(false, |n| name_re.map_or(false, |re| re.is_match(n))) {
            let shield: Vec<(f64, f64)> = if per_way {
                Vec::new()
            } else {
                first.and_then(|n| name_stations.get(n)).into_iter().flatten().map(|&si| stations[si]).filter(|&st| junctions.iter().all(|&j| dist_m(st, j) > JUNCTION_CLEAR_M)).collect()
            };
            for &i in members {
                let covered = if per_way {
                    owned.contains_key(&i)
                } else {
                    let pts = way_pts(i);
                    shield.iter().any(|&st| pts.iter().any(|&p| dist_m(st, p) <= NAME_SHIELD_KM * 1000.0))
                };
                let exempt = exempt_re.map_or(false, |re| tracks[i].props.g("operator").map_or(false, |o| re.is_match(o)));
                if !covered && !exempt && !beside.map_or(false, |b| b.is_beside(tracks, i)) {
                    out.insert(i, "name-no-station");
                }
            }
        }
        // freight_unrouted_no_station: any unrouted railway, named or not, is freight per way unless a passenger station of that railway (its own
        // stations, or those of the same name) is within NAME_SHIELD_KM of the way; stations in a junction's yard (JUNCTION_CLEAR_M) do not count
        if unrouted_no_station && reason.is_none() {
            let mut sis: Vec<usize> = group_stations.clone();
            if let Some(n) = first.filter(|n| !n.is_empty()) {
                sis.extend(name_stations.get(n).into_iter().flatten().copied());
            }
            let shield: Vec<(f64, f64)> = sis.iter().map(|&si| stations[si]).filter(|&st| junctions.iter().all(|&j| dist_m(st, j) > JUNCTION_CLEAR_M)).collect();
            for &i in members {
                if out.contains_key(&i) {
                    continue;
                }
                let covered = if per_way { owned.contains_key(&i) } else { let pts = way_pts(i); shield.iter().any(|&st| pts.iter().any(|&p| dist_m(st, p) <= NAME_SHIELD_KM * 1000.0)) };
                let exempt = exempt_re.map_or(false, |re| tracks[i].props.g("operator").map_or(false, |o| re.is_match(o)));
                if !covered && !exempt && !beside.map_or(false, |b| b.is_beside(tracks, i)) {
                    out.insert(i, "unrouted-no-station");
                }
            }
        }
        if let Some(r) = reason {
            for &i in members {
                out.insert(i, r);
            }
        }
    }
    out
}

/// Pass 2, E2: connected pieces (ways sharing a node, or ends within 2 m) of passenger-typed track (Intercity, Regional, High-speed; not metro, light rail,
/// monorail, funicular) under `max_km` in all, with no way on a passenger route, none beside passenger-route track, none set by an override or run by an
/// exempt operator, and no passenger station within about 200 m of any way. Returns the way indices to make Freight.
fn isolated_strands(tracks: &[TrackIn], decided: &[Option<Decided>], routed: &[bool], pax: &[(i32, i32)], beside: &Beside, exempt_re: Option<&Regex>, max_km: f64) -> Vec<usize> {
    use rustc_hash::FxHashMap;
    let members: Vec<usize> = (0..tracks.len())
        .filter(|&i| {
            tracks[i].props.g("railway") == Some("rail") && tracks[i].ll.len() >= 2 && decided[i].as_ref().map_or(false, |d| matches!(d.t.as_str(), "intercity" | "regional" | "high_speed"))
        })
        .collect();
    let mut parent: Vec<usize> = (0..tracks.len()).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let union = |parent: &mut Vec<usize>, a: usize, b: usize| {
        let (a, b) = (find(parent, a), find(parent, b));
        if a != b {
            parent[a.max(b)] = a.min(b);
        }
    };
    let mut at: FxHashMap<(i32, i32), usize> = FxHashMap::default();
    let ecell = |p: (f64, f64)| ((p.1 / 3e-5).floor() as i32, (p.0 / 3e-5).floor() as i32);
    let mut ends: FxHashMap<(i32, i32), Vec<(f64, f64, usize)>> = FxHashMap::default();
    for &i in &members {
        for &v in tracks[i].ll.iter().filter(|p| p.0 != i32::MIN) {
            match at.get(&v) {
                Some(&j) => union(&mut parent, i, j),
                None => {
                    at.insert(v, i);
                }
            }
        }
        for e in [tracks[i].ll.first(), tracks[i].ll.last()].into_iter().flatten().filter(|p| p.0 != i32::MIN) {
            let q = deg(*e);
            let c = ecell(q);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    for &(x, y, j) in ends.get(&(c.0 + dy, c.1 + dx)).into_iter().flatten() {
                        if dist_m(q, (x, y)) <= 2.0 {
                            union(&mut parent, i, j);
                        }
                    }
                }
            }
            ends.entry(c).or_default().push((q.0, q.1, i));
        }
    }
    let scell = |p: (f64, f64)| ((p.1 / 0.01).floor() as i32, (p.0 / 0.01).floor() as i32);
    let mut sgrid: FxHashMap<(i32, i32), Vec<(f64, f64)>> = FxHashMap::default();
    for &p in pax.iter().filter(|p| p.0 != i32::MIN) {
        sgrid.entry(scell(deg(p))).or_default().push(deg(p));
    }
    let has_station = |i: usize| {
        densify(&tracks[i].ll, 40.0).into_iter().any(|p| {
            let c = scell(p);
            (-1..=1).any(|dy| (-1..=1).any(|dx| sgrid.get(&(c.0 + dy, c.1 + dx)).map_or(false, |v| v.iter().any(|&q| dist_m(p, q) <= PAX_NEAR_M))))
        })
    };
    // component -> (km, protected)
    let mut comp: FxHashMap<usize, (f64, bool)> = FxHashMap::default();
    for &i in &members {
        let r = find(&mut parent, i);
        let d = decided[i].as_ref().unwrap();
        let protect = routed[i]
            || d.s == "override"
            || exempt_re.map_or(false, |re| tracks[i].props.g("operator").map_or(false, |o| re.is_match(o)))
            || beside.is_beside(tracks, i)
            || has_station(i);
        let e = comp.entry(r).or_insert((0.0, false));
        e.0 += ll_km(&tracks[i].ll);
        e.1 |= protect;
    }
    members.into_iter().filter(|&i| comp.get(&find(&mut parent, i)).map_or(false, |&(km, prot)| !prot && km < max_km)).collect()
}

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
    pub pos: usize, // byte position in the features buffer where its properties object closes
    pub props: FeatProps<'a>,
    pub routes: Vec<String>,
    pub first: (i32, i32), // first point (lat, lon), for the proposed-area test
    pub ll: Vec<(i32, i32)>, // every point (lat, lon, 1e-7 degrees; lat = i32::MIN when the node is missing), for the passenger-station test
}

pub struct Out {
    pub typed: Vec<u8>,
    pub lines: Vec<u8>,
    pub report: Vec<u8>,
    pub n_lines: usize,
}

/// a or b with Python truthiness: b when a is missing or empty
fn or<'a>(a: Option<&'a str>, b: Option<&'a str>) -> Option<&'a str> {
    if a.map_or(false, |s| !s.is_empty()) {
        a
    } else {
        b
    }
}

fn english(t: &Owned) -> Option<&str> {
    if let Some(v) = t.g("name:en").filter(|v| !v.is_empty()) {
        return Some(v);
    }
    if let Some(v) = t.g("int_name").filter(|v| !v.is_empty()) {
        return Some(v);
    }
    t.0.iter()
        .find(|(k, v)| k.starts_with("name:") && !v.is_empty() && v.chars().all(|c| (c as u32) <= 0x24f || (0x1e00..=0x1eff).contains(&(c as u32))))
        .map(|(_, v)| v.as_str())
}

/// Counter.most_common(): by count, ties in first-seen order
fn most_common(v: &[(String, i64)]) -> Vec<(String, J)> {
    let mut v = v.to_vec();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v.into_iter().map(|(k, n)| (k, J::Int(n))).collect()
}

fn bump(v: &mut Vec<(String, i64)>, k: &str) {
    match v.iter_mut().find(|x| x.0 == k) {
        Some(x) => x.1 += 1,
        None => v.push((k.to_string(), 1)),
    }
}

pub fn classify(slug: &str, cfg: &Cfg, rows: &[RelRow], feats: &[u8], tracks: &[TrackIn], hs_routes: &FxHashSet<String>, pax: &[(i32, i32)]) -> Out {
    let routes: Vec<&RelRow> = rows.iter().filter(|r| r.route).collect();
    let route_pos: rustc_hash::FxHashMap<&str, usize> = routes.iter().enumerate().map(|(i, r)| (r.id.as_str(), i)).collect();
    let masters: rustc_hash::FxHashMap<&str, &RelRow> = rows.iter().filter(|r| r.master).map(|r| (r.id.as_str(), r)).collect();
    let mut route_master: rustc_hash::FxHashMap<String, &str> = rustc_hash::FxHashMap::default();
    for m in rows.iter().filter(|r| r.master) {
        for id in &m.rel_members {
            route_master.insert(format!("relation/{id}"), m.id.as_str());
        }
    }
    // high_speed_min_kmh: a route may be High-speed only if one of its ways reaches the minimum
    // (one fast stretch does not make a commuter route High-speed): at least HS_MIN_SHARE of the route's track length must be fast
    let fast_routes: FxHashSet<&str> = if cfg.hs_min_kmh.is_some() {
        let mut km: rustc_hash::FxHashMap<&str, (f64, f64)> = rustc_hash::FxHashMap::default(); // route -> (all km, fast km)
        for tr in tracks.iter().filter(|tr| tr.props.g("railway") == Some("rail") && tr.props.g("service").map_or(true, |v| !SERVICE_TRACK.contains(&v))) {
            let (l, fast) = (ll_km(&tr.ll), cfg.hs_fast(tr.props.g("maxspeed")));
            for r in &tr.routes {
                let e = km.entry(r.as_str()).or_default();
                e.0 += l;
                if fast {
                    e.1 += l;
                }
            }
        }
        km.into_iter().filter(|(_, (all, fast))| *all > 0.0 && *fast >= HS_MIN_SHARE * *all).map(|(r, _)| r).collect()
    } else {
        FxHashSet::default()
    };
    let allow = |id: &str| cfg.hs_min_kmh.is_none() || fast_routes.contains(id);
    let rtype: Vec<(String, &'static str)> = routes.iter().map(|r| route_type(&r.tags, cfg, hs_routes.contains(&r.id) && cfg.hs_min_kmh.is_none(), allow(&r.id))).collect();
    // the same routes without the high-speed step: what a slow way of a High-speed line is typed
    let rtype_slow: Vec<String> = routes.iter().map(|r| route_type(&r.tags, cfg, hs_routes.contains(&r.id) && cfg.hs_min_kmh.is_none(), false).0).collect();

    // lines: route_master where one exists, else the route; master type = most prominent of its routes
    let mut line_ids: Vec<&str> = Vec::new();
    let mut line_routes: rustc_hash::FxHashMap<&str, Vec<usize>> = rustc_hash::FxHashMap::default();
    for (i, r) in routes.iter().enumerate() {
        let lid = route_master.get(&r.id).copied().unwrap_or(r.id.as_str());
        line_routes.entry(lid).or_insert_with(|| {
            line_ids.push(lid);
            Vec::new()
        });
        line_routes.get_mut(lid).unwrap().push(i);
    }
    let mut lines_out = b"[".to_vec();
    let mut line_type: rustc_hash::FxHashMap<&str, String> = rustc_hash::FxHashMap::default();
    let mut line_type_slow: rustc_hash::FxHashMap<&str, String> = rustc_hash::FxHashMap::default();
    let (mut by_type, mut by_type_src): (Vec<(String, i64)>, Vec<(String, i64)>) = (Vec::new(), Vec::new());
    for (n, lid) in line_ids.iter().enumerate() {
        let rids = &line_routes[lid];
        let first_route = routes[rids[0]];
        let src = masters.get(lid).copied().unwrap_or(first_route);
        let mut t = highest(rids.iter().map(|&i| rtype[i].0.as_str())).unwrap_or("").to_string();
        // a master-level override (e.g. BART) beats the member routes
        let merged = Merged(&first_route.tags, &src.tags);
        let mut t_slow = highest(rids.iter().map(|&i| rtype_slow[i].as_str())).unwrap_or("").to_string();
        if let Some((_, ty)) = cfg.type_overrides.iter().find(|(r, _)| r.matches(&merged)) {
            t = ty.clone();
            if ty != "high_speed" {
                t_slow = ty.clone();
            }
        }
        let (first, tags) = (&first_route.tags, &src.tags);
        let ts = rtype[rids[0]].1;
        bump(&mut by_type, &t);
        bump(&mut by_type_src, &format!("{t}/{ts}"));
        // json.dump with Python's default separators (", " and ": ")
        let o = &mut lines_out;
        if n > 0 {
            o.extend_from_slice(b", ");
        }
        let opt = |o: &mut Vec<u8>, v: Option<&str>| match v {
            Some(s) => py_str(o, s),
            None => o.extend_from_slice(b"null"),
        };
        o.extend_from_slice(b"{\"id\": ");
        py_str(o, lid);
        o.extend_from_slice(b", \"name\": ");
        opt(o, or(tags.g("name"), first.g("name")));
        o.extend_from_slice(b", \"ref\": ");
        opt(o, or(tags.g("ref"), first.g("ref")));
        o.extend_from_slice(b", \"name_en\": ");
        opt(o, english(tags).or_else(|| english(first)));
        o.extend_from_slice(b", \"tags\": {");
        for (i, (k, v)) in tags.0.iter().enumerate() {
            if i > 0 {
                o.extend_from_slice(b", ");
            }
            py_str(o, k);
            o.extend_from_slice(b": ");
            py_str(o, v);
        }
        o.extend_from_slice(b"}, \"colour\": ");
        opt(o, or(tags.g("colour"), first.g("colour")));
        o.extend_from_slice(b", \"type\": ");
        py_str(o, &t);
        o.extend_from_slice(b", \"type_source\": ");
        py_str(o, ts);
        o.extend_from_slice(b", \"operator\": ");
        opt(o, or(tags.g("operator"), first.g("operator")));
        o.extend_from_slice(b", \"network\": ");
        opt(o, or(tags.g("network"), first.g("network")));
        o.extend_from_slice(b", \"from\": ");
        opt(o, first.g("from"));
        o.extend_from_slice(b", \"to\": ");
        opt(o, first.g("to"));
        o.extend_from_slice(b", \"routes\": [");
        for (i, &ri) in rids.iter().enumerate() {
            if i > 0 {
                o.extend_from_slice(b", ");
            }
            py_str(o, &routes[ri].id);
        }
        o.extend_from_slice(b"]}");
        line_type.insert(lid, t);
        line_type_slow.insert(lid, t_slow);
    }
    lines_out.push(b']');
    let route_line_type = |rid: &str| -> Option<&str> {
        route_pos.get(rid)?;
        line_type.get(route_master.get(rid).copied().unwrap_or(rid)).map(String::as_str)
    };

    let route_line_slow = |rid: &str| -> Option<&str> {
        route_pos.get(rid)?;
        line_type_slow.get(route_master.get(rid).copied().unwrap_or(rid)).map(String::as_str)
    };

    // track: the typed copy of features.geojsonl gets the four fields at the end of each track's properties
    let mut typed = Vec::with_capacity(feats.len() + tracks.len() * 64);
    let mut at = 0;
    let (mut counts, mut sources): (Vec<(String, i64)>, std::collections::BTreeMap<(String, String), i64>) = (Vec::new(), Default::default());
    // A named railway that is freight nearly everywhere (OSM tags `railway:traffic_mode=freight` on most of its ways, not all) is freight
    // all the way: its untagged, unrouted main-line ways would otherwise show as Intercity fragments of a hidden line (Haoji railway).
    let mut by_name: std::collections::HashMap<String, (u32, u32)> = std::collections::HashMap::new(); // name -> (freight ways, ways) with no passenger route
    for tr in tracks {
        let p = &tr.props;
        if p.g("railway") != Some("rail") || tr.routes.iter().any(|r| route_line_type(r).is_some()) {
            continue;
        }
        let Some(name) = p.g("name").filter(|n| !n.is_empty()) else { continue };
        let e = by_name.entry(name.to_string()).or_default();
        e.1 += 1;
        if own_tag_type(p, cfg).0 == "freight" {
            e.0 += 1;
        }
    }
    // candidates for the evidence test: rail with no passenger route that its own tags make Intercity or Regional, and nothing saying passengers
    let cand: Vec<bool> = tracks
        .iter()
        .map(|tr| {
            let p = &tr.props;
            p.g("railway") == Some("rail")
                && tr.ll.len() >= 2
                && !tr.routes.iter().any(|r| route_line_type(r).is_some())
                && p.g("service").map_or(true, |v| !SERVICE_TRACK.contains(&v))
                && !matches!(p.g("railway:traffic_mode"), Some("passenger" | "mixed"))
                && p.g("usage") != Some("tourism")
                && !p.g("name").map_or(false, |n| PASSENGER_NAME_WORDS.iter().any(|w| n.contains(w)))   // connectors and loops link passenger lines; the others say passengers
                && matches!(own_tag_type(p, cfg), (ref t, "tags" | "default") if t == "intercity" || t == "regional")
        })
        .collect();
    // track on a passenger route (rail only): the name rule leaves alone the unrouted track that runs beside it or fills a gap in it
    let routed: Vec<bool> = tracks.iter().map(|tr| tr.props.g("railway") == Some("rail") && tr.routes.iter().any(|r| route_line_type(r).is_some())).collect();
    let beside = (cfg.freight_name.is_some() || cfg.freight_unrouted_no_station || cfg.freight_isolated_km.is_some()).then(|| Beside::new(tracks, &routed));
    let by_evidence = freight_by_evidence(tracks, &cand, pax, cfg.freight_name.as_ref(), cfg.freight_exempt.as_ref(), beside.as_ref(), cfg.freight_name_per_way, cfg.freight_unrouted_no_station);
    let mostly_freight = |name: Option<&str>| name.and_then(|n| by_name.get(n)).map_or(false, |&(f, n)| f >= 5 && f as f64 >= 0.9 * n as f64);
    let mut decided: Vec<Option<Decided>> = Vec::with_capacity(tracks.len());
    for (ti, tr) in tracks.iter().enumerate() {
        let p = &tr.props;
        let rw = p.g("railway").unwrap_or("");
        if !TRACK_RW.contains(&rw) {
            decided.push(None);
            continue;
        }
        if rw == "proposed" && !cfg.proposed.as_ref().map_or(false, |a| a.contains(tr.first.0, tr.first.1)) {
            decided.push(None);
            continue; // proposed track is only kept inside the region's `proposed_region` (Hong Kong for now)
        }
        let mut own: Option<String> = None;
        let (t, s): (String, &str) = if let Some((_, ty)) = cfg.track_type_overrides.iter().find(|(r, _)| r.matches(p)) {
            (ty.clone(), "override")
        } else if p.g("usage") == Some("tourism") {
            ("other".into(), "tags") // tourist and heritage track is always Other, whatever routes or service tags say
        } else if p.g("service").map_or(false, |v| SERVICE_TRACK.contains(&v)) {
            ("yard".into(), "service")
        } else {
            let (o, src) = own_tag_type(p, cfg);
            // high_speed_min_kmh: a way on a High-speed line takes the line's type without the high-speed step unless its own maxspeed
            // reaches the minimum (the route stays on the way, the line keeps its type)
            let slow = !cfg.hs_fast(p.g("maxspeed"));
            let mut cands: Vec<(&str, bool)> = Vec::new(); // (type, came from a demotion)
            for r in &tr.routes {
                let Some(t) = route_line_type(r) else { continue };
                match route_line_slow(r) {
                    Some(sl) if slow && t == "high_speed" => cands.push((sl, true)),
                    _ => cands.push((t, false)),
                }
            }
            if o == "high_speed" {
                cands.push((o.as_str(), false));
            }
            let best = highest(cands.iter().map(|c| c.0)).map(str::to_string);
            let demoted = best.as_deref().map_or(false, |b| cands.iter().any(|c| c.1 && c.0 == b) && !cands.iter().any(|c| !c.1 && c.0 == b));
            own = Some(o.clone());
            match best {
                Some(b) if demoted => (b, "route-slow"),
                Some(b) => (b, "route"),
                None if rw == "rail" && matches!(o.as_str(), "intercity" | "regional") && mostly_freight(p.g("name")) => ("freight".into(), "name"),
                None if by_evidence.contains_key(&ti) => ("freight".into(), by_evidence[&ti]),
                None => (o, src),
            }
        };
        decided.push(Some(Decided { t, s, own }));
    }
    // A short piece of track whose type differs from all the track it joins, when those agree, takes their type: a few metres of
    // "Regional" in the middle of a freight line is a tagging gap, not a service.
    let mut at_end: rustc_hash::FxHashMap<(i32, i32), Vec<usize>> = rustc_hash::FxHashMap::default();
    for (i, tr) in tracks.iter().enumerate() {
        if decided[i].is_some() && tr.props.g("railway") == Some("rail") {
            for e in [tr.ll.first(), tr.ll.last()].into_iter().flatten() {
                at_end.entry(*e).or_default().push(i);
            }
        }
    }
    let smoothable = |d: &Decided| matches!(d.s, "tags" | "default" | "name" | "no-station" | "name-no-station" | "unrouted-no-station" | "short-spur") && matches!(d.t.as_str(), "intercity" | "regional" | "freight");
    for _ in 0..2 {
        let mut changes: Vec<(usize, String)> = Vec::new();
        for (i, tr) in tracks.iter().enumerate() {
            let Some(d) = decided[i].as_ref().filter(|d| smoothable(d)) else { continue };
            if tr.props.g("railway") != Some("rail") || tr.ll.len() < 2 || ll_km(&tr.ll) >= SNIPPET_KM {
                continue;
            }
            let mut around: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
            let mut both_ends = true;
            for e in [tr.ll.first().unwrap(), tr.ll.last().unwrap()] {
                let n: Vec<&str> = at_end.get(e).into_iter().flatten().filter(|&&j| j != i).filter_map(|&j| decided[j].as_ref().map(|x| x.t.as_str())).collect();
                both_ends &= !n.is_empty();
                around.extend(n);
            }
            if both_ends && around.len() == 1 {
                let u = *around.iter().next().unwrap();
                if u != d.t && matches!(u, "intercity" | "regional" | "freight" | "high_speed") && !(u == "high_speed" && !cfg.hs_fast(tr.props.g("maxspeed"))) {
                    changes.push((i, u.to_string()));
                }
            }
        }
        if changes.is_empty() {
            break;
        }
        for (i, u) in changes {
            if let Some(d) = decided[i].as_mut() {
                d.t = u;
                d.s = "neighbour";
            }
        }
    }
    if let (Some(km), Some(b)) = (cfg.freight_isolated_km, beside.as_ref()) {
        for i in isolated_strands(tracks, &decided, &routed, pax, b, cfg.freight_exempt.as_ref(), km) {
            if let Some(d) = decided[i].as_mut() {
                d.t = "freight".into();
                d.s = "isolated";
            }
        }
    }
    for (ti, tr) in tracks.iter().enumerate() {
        let Some(Decided { t, s, own }) = decided[ti].take() else { continue };
        let p = &tr.props;
        let rw = p.g("railway").unwrap_or("");
        typed.extend_from_slice(&feats[at..tr.pos]);
        at = tr.pos;
        if rw == "construction" {
            typed.extend_from_slice(b",\"construction_type\":");
            match p.g("construction") {
                Some(c) => js(&mut typed, c.as_bytes()),
                None => typed.extend_from_slice(b"null"),
            }
        }
        typed.extend_from_slice(b",\"type\":");
        js(&mut typed, t.as_bytes());
        typed.extend_from_slice(b",\"type_source\":");
        js(&mut typed, s.as_bytes());
        typed.extend_from_slice(b",\"own_type\":");
        match own.filter(|_| rw != "construction" && rw != "proposed") {
            Some(o) => js(&mut typed, o.as_bytes()),
            None => typed.extend_from_slice(b"null"),
        }
        bump(&mut counts, &t);
        *sources.entry((t, s.to_string())).or_default() += 1;
    }
    typed.extend_from_slice(&feats[at..]);

    let report = J::obj(vec![
        ("region", J::s(slug)),
        ("track_by_type", J::Obj(most_common(&counts))),
        ("track_by_type_and_source", J::Obj(sources.iter().map(|((t, s), n)| (format!("{t}/{s}"), J::Int(*n))).collect())),
        ("lines", J::Int(line_ids.len() as i64)),
        ("lines_by_type", J::Obj(most_common(&by_type))),
        ("lines_by_type_source", J::Obj(most_common(&by_type_src))),
    ]);
    let mut rep = Vec::new();
    report.write_pretty(&mut rep, 0);
    Out { typed, lines: lines_out, report: rep, n_lines: line_ids.len() }
}
