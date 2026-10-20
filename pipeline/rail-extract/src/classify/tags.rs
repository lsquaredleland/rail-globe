//! Tag access, the type vocabulary (ORDER, railway and service tag types), maxspeed parsing and the construction-tag rewrite.

use super::*;

/// most prominent first; track takes the highest type that runs on it (regional includes commuter, merged 2026-10-12)
pub(super) const ORDER: [&str; 8] = ["high_speed", "intercity", "regional", "metro", "light_rail", "monorail", "funicular", "other"];
pub(super) const TRACK_RW: [&str; 9] = ["rail", "subway", "light_rail", "tram", "monorail", "funicular", "narrow_gauge", "construction", "proposed"];
pub(super) const SERVICE_TRACK: [&str; 4] = ["yard", "siding", "spur", "crossover"];
pub(super) const FREIGHT_USAGE: [&str; 4] = ["industrial", "freight", "military", "farm"];

pub(super) fn rank(t: &str) -> usize {
    ORDER.iter().position(|x| *x == t).unwrap_or(ORDER.len())
}

pub(super) fn route_mode_type(m: &str) -> Option<&'static str> {
    Some(match m {
        "subway" => "metro",
        "light_rail" | "tram" => "light_rail",
        "monorail" => "monorail",
        "funicular" => "funicular",
        _ => return None,
    })
}

pub(super) fn railway_type(rw: &str) -> Option<&'static str> {
    route_mode_type(rw)
}

/// The type of a route's `service` tag. Several values (`international;long_distance`) take the most prominent type any of them gives.
pub(super) fn service_type(s: &str) -> Option<&'static str> {
    highest(s.split(';').filter_map(|v| service_type_one(v.trim())))
}

pub(super) fn service_type_one(s: &str) -> Option<&'static str> {
    Some(match s {
        "high_speed" | "highspeed" | "high-speed" => "high_speed", // KTX relations in Korea say service=highspeed
        "long_distance" | "night" => "intercity",
        "regional" | "commuter" => "regional",
        "tourism" | "car_shuttle" => "other", // Eurotunnel Le Shuttle carries cars, not a passenger service
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

/// tags merged for a line override: the route's tags, overridden by the line source's
pub(super) struct Merged<'a>(pub(super) &'a Owned, pub(super) &'a Owned);

impl Get for Merged<'_> {
    fn g(&self, k: &str) -> Option<&str> {
        self.1.g(k).or_else(|| self.0.g(k))
    }
}

/// a line's merged tags plus its id, for exemptions
pub(super) struct LineTags<'a>(pub(super) &'a Merged<'a>, pub(super) &'a str);

impl Get for LineTags<'_> {
    fn g(&self, k: &str) -> Option<&str> {
        if k == "osm_id" {
            return Some(self.1);
        }
        self.0.g(k)
    }
}

/// leading number (any Unicode decimal digits), optional "mph"; None without a number
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

/// None and 0 are false
pub(super) fn at_least(ms: Option<f64>, v: f64) -> bool {
    ms.is_some_and(|m| m != 0.0 && m >= v)
}

pub(super) fn highest<'a>(types: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    types.filter(|t| !t.is_empty()).min_by_key(|t| rank(t))
}

/// a, or b when a is missing or empty
pub(super) fn or<'a>(a: Option<&'a str>, b: Option<&'a str>) -> Option<&'a str> {
    if a.is_some_and(|s| !s.is_empty()) {
        a
    } else {
        b
    }
}

pub(super) fn english(t: &Owned) -> Option<&str> {
    crate::rules::english_name_kv(t.0.iter().map(|(k, v)| (k.as_bytes(), v.as_bytes()))).and_then(|v| std::str::from_utf8(v).ok())
}

/// by count, ties in first-seen order
pub(super) fn most_common(v: &[(String, i64)]) -> Vec<(String, J)> {
    let mut v = v.to_vec();
    v.sort_by_key(|a| std::cmp::Reverse(a.1));
    v.into_iter().map(|(k, n)| (k, J::Int(n))).collect()
}

pub(super) fn bump(v: &mut Vec<(String, i64)>, k: &str) {
    match v.iter_mut().find(|x| x.0 == k) {
        Some(x) => x.1 += 1,
        None => v.push((k.to_string(), 1)),
    }
}

/// A way mapped as an open railway kind plus `construction=*` (West Cairo monorail: `railway=monorail` + `construction=yes`) is being built:
/// the mode the railway key names, read like `railway=construction` + `construction=<mode>`. New-build modes (monorail, light_rail, subway, tram)
/// close on `construction=yes` or a rail kind. `railway=rail|narrow_gauge` closes only on a rail-kind value, with no `ref` and an `operator` or
/// `highspeed=yes` (`yes`, and the value `rail` on a way with a ref, are usually a doubling or electrification left on a line that runs; the
/// caller also keeps it open on a running route). `no`, `minor` and `electrification` never count.
pub(super) fn con_by_tags<T: Get + ?Sized>(p: &T) -> Option<&str> {
    let rw = p.g("railway").filter(|r| matches!(*r, "rail" | "monorail" | "light_rail" | "subway" | "tram" | "narrow_gauge"))?;
    let c = p.g("construction")?;
    let kind = matches!(c, "rail" | "narrow_gauge" | "subway" | "light_rail" | "tram" | "monorail");
    if matches!(rw, "rail" | "narrow_gauge") {
        let new_build = p.g("ref").is_none_or(str::is_empty) && (p.g("operator").is_some_and(|o| !o.is_empty()) || p.g("highspeed") == Some("yes"));
        return (kind && new_build).then_some(rw);
    }
    (kind || matches!(c, "yes" | "railway")).then_some(rw)
}

/// A route relation that is not running yet: it says so in its name ("(en construccion)", "under construction", "proyectado", "planned")
/// or in `state` / `railway`. Such a relation is no service: it is not a line and gives its ways no route type.
pub fn route_under_construction(t: &Owned) -> bool {
    use std::sync::OnceLock;
    static NAME: OnceLock<Regex> = OnceLock::new();
    let re = NAME.get_or_init(|| {
        Regex::new(r"(?i)construcci[oó]n|\bconstruction\b|en obras|proyectad|\bplanned\b|\bproposed\b|em constru[cç][aã]o|in costruzione|\bim bau\b|en cours de construction|w budowie|v výstavbě").unwrap()
    });
    matches!(t.g("state"), Some("construction" | "proposed" | "planned"))
        || matches!(t.g("railway"), Some("construction" | "proposed"))
        || t.g("name").is_some_and(|n| re.is_match(n))
}

/// Track tagged as an open railway kind plus `construction=*` is construction track from here on, unless it is on a running route
/// relation (it then stays open, whatever its mode). The mode per track, in order.
pub(super) fn construction_modes<'a>(tracks: &'a [TrackIn], rows: &[RelRow]) -> Vec<Option<&'a str>> {
    let svc_routes: FxHashSet<&str> = rows.iter().filter(|r| r.route && !route_under_construction(&r.tags)).map(|r| r.id.as_str()).collect();
    tracks.iter().map(|tr| con_by_tags(&tr.props).filter(|_| !tr.routes.iter().any(|r| svc_routes.contains(r.as_str())))).collect()
}

/// The tracks with the construction ways' tags rewritten to `railway=construction` + `construction=<mode>`.
pub(super) fn with_construction_tags<'a>(tracks: &'a [TrackIn], con_mode: &[Option<&'a str>]) -> Vec<TrackIn<'a>> {
    tracks
        .iter()
        .zip(con_mode)
        .map(|(tr, cm)| {
            let mut tags = tr.props.tags.0.clone();
            if let Some(rw) = cm {
                tags.retain(|(k, _)| *k != b"railway" && *k != b"construction");
                tags.push((b"railway", b"construction"));
                tags.push((b"construction", rw.as_bytes()));
            }
            TrackIn {
                start: tr.start,
                end: tr.end,
                pos: tr.pos,
                props: FeatProps { tags: List(tags), osm_id: tr.props.osm_id.clone() },
                routes: tr.routes.clone(),
                first: tr.first,
                ll: tr.ll.clone(),
            }
        })
        .collect()
}
