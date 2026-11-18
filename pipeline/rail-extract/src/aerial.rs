//! Gondolas and aerial cable cars that run as public transit (docs/00-overview.md, 2026-11-15).
//!
//! OSM tags cannot tell a transit gondola from a sightseeing or ski one, so the cut keeps only the lifts on the hand-kept allowlist
//! `pipeline/config/_aerialway.json`. The cache holds every `aerialway=gondola|cable_car|mixed_lift` way and every `aerialway=station`,
//! so a change to the list needs a new cut, not a new planet read.
//!
//! A kept lift is handed to the rest of the pipeline as track of the mode `gondola`: its tags get `railway=gondola` (or
//! `railway=construction` + `construction=gondola` while it is being built), and its stations get `railway=station`. `gondola` is the
//! pipeline's own mode name, not an OSM railway value; the OSM value stays in `aerialway`, which only these features carry.

use crate::rules::{self, is, Tags};
use regex::Regex;
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::Value;
use std::path::Path;

/// a station this close to a kept lift is one of its stations
pub const STATION_NEAR_M: f64 = 150.0;

pub struct System {
    pub name: String,
    /// the slug of the country cut the system lies in: that cut warns when the system matches nothing
    pub country: String,
    /// west, south, east, north (degrees)
    pub bbox: [f64; 4],
    pub name_re: Option<Regex>,
    pub ways: Vec<i64>,
    pub exclude_ways: Vec<i64>,
}

#[derive(Default)]
pub struct Cfg {
    /// a way with no name (its own or a route relation's) shorter than this is dropped
    pub min_unnamed_km: f64,
    pub systems: Vec<System>,
}

impl Cfg {
    /// `pipeline/config/_aerialway.json`; no file means no lifts are kept
    pub fn load(path: &Path) -> Cfg {
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("  WARNING: no {}: no gondola or cable car is kept", path.display());
            return Cfg::default();
        };
        Cfg::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Cfg, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let ids = |x: &Value| -> Vec<i64> { x.as_array().into_iter().flatten().filter_map(Value::as_i64).collect() };
        let mut systems = Vec::new();
        for s in v["systems"].as_array().ok_or("no \"systems\" list")? {
            let name = s["name"].as_str().ok_or("a system has no name")?.to_string();
            let b: Vec<f64> = s["bbox"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
            let bbox: [f64; 4] = b.try_into().map_err(|_| format!("{name}: bbox must be [west, south, east, north]"))?;
            if bbox[0] > bbox[2] || bbox[1] > bbox[3] {
                return Err(format!("{name}: bbox must be [west, south, east, north]"));
            }
            let name_re = match s["name_re"].as_str() {
                Some(p) => Some(Regex::new(p).map_err(|e| format!("{name}: bad name_re: {e}"))?),
                None => None,
            };
            let country = s["country"].as_str().ok_or_else(|| format!("{name}: no country"))?.to_string();
            systems.push(System { name, country, bbox, name_re, ways: ids(&s["ways"]), exclude_ways: ids(&s["exclude_ways"]) });
        }
        Ok(Cfg { min_unnamed_km: v["min_unnamed_km"].as_f64().unwrap_or(0.0), systems })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Open,
    Construction,
}

/// Open, being built, or None for a lift that is not kept: proposed, abandoned, disused or closed to the public.
/// OSM maps a lift being built as `aerialway=construction` + `construction=gondola`, or as `aerialway=gondola` + `construction=*`.
pub fn status<T: Tags + ?Sized>(t: &T) -> Option<Status> {
    let set = |k: &[u8]| t.get(k).is_some_and(|v| !v.is_empty() && v != b"no");
    if set(b"proposed") || is(t, b"abandoned", b"yes") || is(t, b"disused", b"yes") || is(t, b"access", b"no") {
        return None;
    }
    if is(t, b"aerialway", b"construction") || set(b"construction") {
        return Some(Status::Construction);
    }
    Some(Status::Open)
}

/// one lift way of the region, as the matcher sees it
pub struct Lift<'a> {
    pub id: i64,
    pub name: Option<&'a str>,
    /// names and refs of the `route=aerialway` relations the way is in
    pub rel_names: &'a [String],
    pub rel_refs: &'a [String],
    /// (lat, lon) in degrees of the first and last node
    pub ends: [(f64, f64); 2],
    pub km: f64,
    pub status: Option<Status>,
}

impl System {
    pub fn matches(&self, w: &Lift) -> bool {
        if self.exclude_ways.contains(&w.id) {
            return false;
        }
        if self.ways.contains(&w.id) {
            return true;
        }
        let [west, south, east, north] = self.bbox;
        let inside = w.ends.iter().any(|&(lat, lon)| west <= lon && lon <= east && south <= lat && lat <= north);
        inside && self.name_re.as_ref().is_none_or(|re| w.name.into_iter().chain(w.rel_names.iter().chain(w.rel_refs).map(String::as_str)).any(|n| re.is_match(n)))
    }
}

/// the lifts of one region that are kept
#[derive(Default)]
pub struct Picked {
    pub keep: FxHashMap<i64, Status>,
    /// `route=aerialway` relations with a kept way
    pub routes: FxHashSet<i64>,
    /// the kept ways' segments, (lat, lon) in 1e-7 degrees, for the station test
    segs: Vec<[(i32, i32); 2]>,
    /// (lat, lon) of the station nodes on a kept lift
    pub station_nodes: Vec<(i32, i32)>,
    /// systems of this country that matched nothing, as warning text
    pub stale: Vec<String>,
    /// kept ways per system name (for the summary)
    pub by_system: Vec<(String, usize)>,
    pub dropped_status: usize,
    pub dropped_unnamed: usize,
}

impl Picked {
    /// Keep the lifts a system matches. `slug` is the region being cut: its systems must match something.
    pub fn pick(cfg: &Cfg, slug: &str, lifts: &[Lift]) -> Picked {
        let mut p = Picked::default();
        let mut n = vec![0usize; cfg.systems.len()];
        for w in lifts {
            let Some(si) = cfg.systems.iter().position(|s| s.matches(w)) else { continue };
            let Some(st) = w.status else {
                p.dropped_status += 1;
                continue;
            };
            let named = w.name.is_some_and(|n| !n.is_empty()) || w.rel_names.iter().any(|n| !n.is_empty());
            if !named && w.km < cfg.min_unnamed_km {
                p.dropped_unnamed += 1;
                continue;
            }
            p.keep.insert(w.id, st);
            n[si] += 1;
        }
        for (s, &k) in cfg.systems.iter().zip(&n) {
            if k > 0 {
                p.by_system.push((s.name.clone(), k));
            } else if s.country == slug {
                p.stale.push(format!("aerialway system \"{}\" ({}) matched no lift: fix or remove it in pipeline/config/_aerialway.json", s.name, s.country));
            }
        }
        p
    }

    pub fn add_geometry(&mut self, ll: &[(i32, i32)]) {
        self.segs.extend(ll.windows(2).filter(|w| w[0].0 != i32::MIN && w[1].0 != i32::MIN).map(|w| [w[0], w[1]]));
    }

    /// is the point within STATION_NEAR_M of a kept lift?
    pub fn near(&self, lat: i32, lon: i32) -> bool {
        lat != i32::MIN && self.segs.iter().any(|s| seg_dist_m((lat, lon), s[0], s[1]) <= STATION_NEAR_M)
    }
}

impl Picked {
    /// Does a kept station node stand in (or within about 30 m of) the box of this way? The way is then that station's building, not a
    /// second station (mappers name the two differently: "Fátima" and "Estación Mexicable Fátima").
    pub fn has_station_node(&self, ll: &[(i32, i32)]) -> bool {
        const M: i32 = 3000; // 1e-7 degrees
        let mut b = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        for p in ll.iter().filter(|p| p.0 != i32::MIN) {
            b = [b[0].min(p.0), b[1].min(p.1), b[2].max(p.0), b[3].max(p.1)];
        }
        self.station_nodes.iter().any(|n| b[0] - M <= n.0 && n.0 <= b[2] + M && b[1] - M <= n.1 && n.1 <= b[3] + M)
    }
}

/// metres from p to the segment a-b ((lat, lon) in 1e-7 degrees), on a local flat projection
fn seg_dist_m(p: (i32, i32), a: (i32, i32), b: (i32, i32)) -> f64 {
    let kx = 0.011132 * (p.0 as f64 * 1e-7).to_radians().cos(); // metres per 1e-7 degree of longitude
    let ky = 0.011054;
    let xy = |q: (i32, i32)| ((q.1 as f64 - p.1 as f64) * kx, (q.0 as f64 - p.0 as f64) * ky);
    let ((ax, ay), (bx, by)) = (xy(a), xy(b));
    let (dx, dy) = (bx - ax, by - ay);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 { 0.0 } else { ((-ax * dx - ay * dy) / l2).clamp(0.0, 1.0) };
    (ax + t * dx).hypot(ay + t * dy)
}

/// The tags of a kept lift as track of the mode `gondola` (see the module text).
pub fn track_tags<'a>(tags: &[(&'a [u8], &'a [u8])], st: Status) -> Vec<(&'a [u8], &'a [u8])> {
    let mut t: Vec<(&[u8], &[u8])> = tags.iter().copied().filter(|(k, _)| !matches!(*k, b"railway" | b"construction")).collect();
    match st {
        Status::Open => t.push((b"railway", b"gondola")),
        Status::Construction => {
            t.push((b"railway", b"construction"));
            t.push((b"construction", b"gondola"));
        }
    }
    t
}

/// The tags of a lift station as a station: `railway=station` beside its `aerialway=station`, which gives it the mode `gondola`.
pub fn station_tags<'a>(tags: &[(&'a [u8], &'a [u8])]) -> Vec<(&'a [u8], &'a [u8])> {
    let mut t: Vec<(&[u8], &[u8])> = tags.iter().copied().filter(|(k, _)| *k != b"railway").collect();
    t.push((b"railway", b"station"));
    t
}

/// The tags of a lift station's building when the station itself is a node: a station building, whose footprint counts for the station's size.
pub fn station_building_tags<'a>(tags: &[(&'a [u8], &'a [u8])]) -> Vec<(&'a [u8], &'a [u8])> {
    let mut t: Vec<(&[u8], &[u8])> = tags.iter().copied().filter(|(k, _)| !matches!(*k, b"building" | b"public_transport")).collect();
    t.push((b"building", b"train_station"));
    t
}

/// a way tagged `aerialway=station` that is no railway station, platform or railway land by its other tags
pub fn is_station_only<T: Tags + ?Sized>(t: &T) -> bool {
    is(t, b"aerialway", b"station") && rules::classify_poly(t).is_none()
}

/// a way that is an aerial lift and no railway
pub fn is_lift<T: Tags + ?Sized>(t: &T) -> bool {
    !rules::has(t, b"railway") && rules::aerial_lift(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::List;

    const CFG: &str = r#"{ "min_unnamed_km": 0.3, "systems": [
        { "name": "Cablebús", "country": "mx", "bbox": [-99.4, 19.1, -98.9, 19.75], "name_re": "(?i)cableb[uú]s" },
        { "name": "Mexicable", "country": "mx", "bbox": [-99.4, 19.1, -98.9, 19.75], "name_re": "(?i)mexicable" },
        { "name": "Mi Teleférico", "country": "bo", "bbox": [-68.3, -16.65, -68.0, -16.4], "exclude_ways": [7] },
        { "name": "TransMiCable", "country": "co", "bbox": [-74.3, 4.4, -73.95, 4.85], "name_re": "(?i)transmicable", "ways": [9] }
    ] }"#;

    fn lift<'a>(id: i64, name: Option<&'a str>, at: (f64, f64), km: f64, rels: &'a [String], refs: &'a [String]) -> Lift<'a> {
        Lift { id, name, rel_names: rels, rel_refs: refs, ends: [(0.0, 0.0), at], km, status: Some(Status::Open) }
    }
    fn tags(t: &[(&'static str, &'static str)]) -> List<'static> {
        List(t.iter().map(|(k, v)| (k.as_bytes(), v.as_bytes())).collect())
    }
    const MX: (f64, f64) = (19.5, -99.1);
    const LP: (f64, f64) = (-16.5, -68.15);
    const BOG: (f64, f64) = (4.6, -74.1);

    #[test]
    fn bbox_is_tested_at_either_end_and_name_re_on_way_and_relation() {
        let c = Cfg::parse(CFG).unwrap();
        let none: [String; 0] = [];
        let rel = ["Cablebús Línea 1".to_string()];
        let refs = ["Mexicable 2".to_string()];
        let lifts = [
            lift(1, Some("Cablebús Linea 2"), MX, 5.0, &none, &none), // by its own name
            lift(2, None, MX, 5.0, &rel, &none),                       // by its relation's name
            lift(3, None, MX, 5.0, &none, &refs),                      // by its relation's ref (the second system)
            lift(4, Some("Teleférico turístico"), MX, 5.0, &none, &none), // in the box, no name match
            lift(5, Some("Cablebús"), (48.0, 2.0), 5.0, &none, &none), // name match, outside every box
            lift(6, Some("Línea Roja"), LP, 2.0, &none, &none),        // a box with no name_re keeps every lift
        ];
        let p = Picked::pick(&c, "mx", &lifts);
        let mut kept: Vec<i64> = p.keep.keys().copied().collect();
        kept.sort();
        assert_eq!(kept, [1, 2, 3, 6]);
        assert_eq!(p.by_system, [("Cablebús".to_string(), 2), ("Mexicable".to_string(), 1), ("Mi Teleférico".to_string(), 1)]);
        assert!(p.stale.is_empty(), "both Mexican systems matched; the others are not this cut's");
    }

    #[test]
    fn ways_always_match_and_exclude_ways_never_do() {
        let c = Cfg::parse(CFG).unwrap();
        let none: [String; 0] = [];
        let lifts = [
            lift(7, Some("Línea Azul"), LP, 2.0, &none, &none),   // excluded
            lift(9, None, (10.0, 10.0), 2.9, &none, &none),        // listed: no name, outside the box
            lift(10, Some("Monserrate"), BOG, 0.6, &none, &none),  // in the box, name_re says no
        ];
        let p = Picked::pick(&c, "bo", &lifts);
        assert_eq!(p.keep.keys().copied().collect::<Vec<_>>(), [9]);
        assert_eq!(p.stale.len(), 1, "Mi Teleférico is this cut's and matched nothing: {:?}", p.stale);
        assert!(p.stale[0].contains("Mi Teleférico"));
    }

    #[test]
    fn unnamed_short_ways_and_closed_lifts_are_dropped() {
        let c = Cfg::parse(CFG).unwrap();
        let none: [String; 0] = [];
        let rel = ["Línea Verde".to_string()];
        let mut closed = lift(14, Some("Línea Vieja"), LP, 2.0, &none, &none);
        closed.status = None;
        let lifts = [
            lift(11, None, LP, 0.1, &none, &none),      // unnamed and short
            lift(12, None, LP, 0.31, &none, &none),     // unnamed, long enough
            lift(13, None, LP, 0.1, &rel, &none),       // short, named by its relation
            lift(15, Some(""), LP, 0.1, &none, &none),  // an empty name is no name
            closed,
        ];
        let p = Picked::pick(&c, "xx", &lifts);
        let mut kept: Vec<i64> = p.keep.keys().copied().collect();
        kept.sort();
        assert_eq!(kept, [12, 13]);
        assert_eq!((p.dropped_unnamed, p.dropped_status), (2, 1));
    }

    #[test]
    fn status_from_tags() {
        assert_eq!(status(&tags(&[("aerialway", "gondola")])), Some(Status::Open));
        assert_eq!(status(&tags(&[("aerialway", "gondola"), ("construction", "no")])), Some(Status::Open));
        assert_eq!(status(&tags(&[("aerialway", "gondola"), ("construction", "gondola")])), Some(Status::Construction));
        assert_eq!(status(&tags(&[("aerialway", "gondola"), ("construction", "yes")])), Some(Status::Construction));
        assert_eq!(status(&tags(&[("aerialway", "construction"), ("construction", "cable_car")])), Some(Status::Construction));
        assert_eq!(status(&tags(&[("aerialway", "gondola"), ("proposed", "aerialway")])), None);
        assert_eq!(status(&tags(&[("aerialway", "cable_car"), ("abandoned", "yes")])), None);
        assert_eq!(status(&tags(&[("aerialway", "cable_car"), ("disused", "yes"), ("access", "no")])), None);
        assert_eq!(status(&tags(&[("aerialway", "cable_car"), ("access", "no")])), None);
    }

    #[test]
    fn lifts_and_their_pipeline_tags() {
        assert!(is_lift(&tags(&[("aerialway", "gondola")])));
        assert!(is_lift(&tags(&[("aerialway", "mixed_lift")])));
        assert!(is_lift(&tags(&[("aerialway", "construction"), ("construction", "gondola")])));
        assert!(!is_lift(&tags(&[("aerialway", "construction"), ("construction", "chair_lift")])));
        assert!(!is_lift(&tags(&[("aerialway", "chair_lift")])));
        assert!(!is_lift(&tags(&[("aerialway", "proposed")])));
        assert!(!is_lift(&tags(&[("aerialway", "gondola"), ("railway", "funicular")])), "a railway stays a railway");
        let t = tags(&[("aerialway", "gondola"), ("construction", "yes"), ("name", "L3")]);
        let open = List(track_tags(&t.0, Status::Open));
        assert_eq!((open.get(b"railway"), open.get(b"construction"), open.get(b"aerialway")), (Some(&b"gondola"[..]), None, Some(&b"gondola"[..])));
        let con = List(track_tags(&t.0, Status::Construction));
        assert_eq!((con.get(b"railway"), con.get(b"construction"), con.get(b"name")), (Some(&b"construction"[..]), Some(&b"gondola"[..]), Some(&b"L3"[..])));
        let s = List(station_tags(&tags(&[("aerialway", "station"), ("railway", "abandoned")]).0));
        assert_eq!((s.get(b"railway"), s.get(b"aerialway")), (Some(&b"station"[..]), Some(&b"station"[..])));
    }

    #[test]
    fn stations_near_a_kept_lift() {
        let mut p = Picked::default();
        // 1 km of cable running east at 19.5 N
        p.add_geometry(&[(195_000_000, -991_000_000), (195_000_000, -990_904_000)]);
        assert!(p.near(195_000_000, -990_950_000), "on the cable");
        assert!(p.near(195_010_000, -990_950_000), "110 m north of it");
        assert!(!p.near(195_020_000, -990_950_000), "220 m north of it");
        assert!(p.near(195_000_000, -991_010_000), "100 m past its end");
        assert!(!p.near(195_000_000, -991_030_000), "300 m past its end");
        assert!(!p.near(i32::MIN, 0));
    }

    #[test]
    fn a_station_way_with_a_station_node_is_a_building() {
        let mut p = Picked::default();
        p.station_nodes.push((195_000_500, -990_950_500));
        let hall = [(195_000_000, -990_951_000), (195_001_000, -990_951_000), (195_001_000, -990_950_000), (195_000_000, -990_950_000)];
        assert!(p.has_station_node(&hall), "the node is inside the hall");
        let beside: Vec<(i32, i32)> = hall.iter().map(|q| (q.0 + 3_000, q.1)).collect();
        assert!(p.has_station_node(&beside), "the node is 22 m from the hall");
        let away: Vec<(i32, i32)> = hall.iter().map(|q| (q.0 + 9_000, q.1)).collect();
        assert!(!p.has_station_node(&away), "the node is 90 m from the hall");
        let b = List(station_building_tags(&tags(&[("aerialway", "station"), ("public_transport", "station"), ("building", "yes"), ("name", "Hall")]).0));
        assert_eq!((b.get(b"building"), b.get(b"public_transport"), b.get(b"railway")), (Some(&b"train_station"[..]), None, None));
        assert_eq!(rules::classify_poly(&b), Some("area:station_building"));
    }

    #[test]
    fn the_shipped_allowlist_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../config/_aerialway.json");
        let c = Cfg::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(c.systems.len() >= 30 && c.min_unnamed_km == 0.3);
    }

    #[test]
    fn bad_config_is_an_error() {
        assert!(Cfg::parse(r#"{"systems":[{"name":"x","country":"mx","bbox":[1,2,3]}]}"#).is_err());
        assert!(Cfg::parse(r#"{"systems":[{"name":"x","country":"mx","bbox":[5,2,3,4]}]}"#).is_err());
        assert!(Cfg::parse(r#"{"systems":[{"name":"x","country":"mx","bbox":[1,2,3,4],"name_re":"("}]}"#).is_err());
        assert!(Cfg::parse(r#"{"systems":[{"name":"x","bbox":[1,2,3,4]}]}"#).is_err());
    }
}
