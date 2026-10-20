//! Small helpers of the tile input and side files: rail type order, colours and labels, shard files, one GeoJSON feature line.

use crate::geomx::{self, round};
use regex::Regex;
use serde_json::{Map, Value};
use std::fs;
use std::path::Path;

const TYPE_ORDER: [&str; 8] = ["high_speed", "intercity", "regional", "metro", "light_rail", "monorail", "funicular", "other"];
const SHARD_TARGET_BYTES: usize = 256 * 1024; // side-file shards hold about this much JSON each (the app fetches one per click)
const CSS_NAMES: [&str; 32] = ["red", "blue", "green", "yellow", "orange", "purple", "pink", "brown", "black", "grey", "gray", "darkblue", "skyblue",
    "lightblue", "darkgreen", "lightgreen", "cyan", "magenta", "violet", "gold", "navy", "teal", "maroon", "lime", "olive", "silver",
    "turquoise", "crimson", "indigo", "darkred", "darkorange", "white"];

pub(super) fn rank(t: &str) -> usize {
    TYPE_ORDER.iter().position(|x| *x == t).unwrap_or(9)
}

pub(super) fn track_minzoom(t: &str) -> i64 {
    match t {
        "high_speed" => 0,
        "intercity" => 3,
        "regional" => 5,
        "metro" | "freight" => 7,
        "yard" => 12,
        _ => 8,
    }
}

pub(super) fn colour(c: Option<&str>) -> Option<String> {
    let c = c?.trim().to_lowercase();
    if c.is_empty() {
        return None;
    }
    let hex = |s: &str| s.chars().all(|ch| ch.is_ascii_hexdigit());
    if c.starts_with('#') && (c.len() == 4 || c.len() == 7) && hex(&c[1..]) {
        return Some(c);
    }
    if c.len() == 6 && hex(&c) {
        return Some(format!("#{c}"));
    }
    CSS_NAMES.contains(&c.as_str()).then_some(c)
}

/// short on-map name of a line: the name before any ": from ↔ to" suffix, else its ref (30 characters at most)
pub(super) fn label_of(name: Option<&str>, r: Option<&str>) -> String {
    thread_local!(static RE: Regex = Regex::new(r"\s*[:：]\s*").unwrap());
    let n = RE.with(|re| re.split(name.unwrap_or("")).next().unwrap_or("").trim().to_string());
    let v = if !n.is_empty() { n } else { r.unwrap_or("").to_string() };
    v.chars().take(30).collect()
}

/// the line's number for its on-map badge: a ref with a digit (4 characters at most: 2, S5, RE1) or a one or two letter code (M, NS).
/// Longer letter-only codes (Hong Kong's TCL, AEL) are abbreviations, not numbers: those lines show their name instead.
pub(super) fn badge_of(r: Option<&str>) -> Option<String> {
    let r = r?.trim();
    let n = r.chars().count();
    let ok = !r.contains(';') && n > 0 && (n <= 2 || (n <= 4 && r.chars().any(|c| c.is_ascii_digit())));
    ok.then(|| r.to_string())
}

/// true when a #rgb / #rrggbb colour is light enough that a badge needs dark text on it
pub(super) fn is_light(c: &str) -> bool {
    if ["white", "yellow", "lime", "cyan", "aqua", "gold", "silver", "lightgreen", "skyblue", "lightblue", "pink", "orange", "beige", "ivory", "khaki", "lightgray", "lightgrey"].contains(&c) {
        return true;
    }
    let h = c.trim_start_matches('#');
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let v = |i: usize, w: usize| u32::from_str_radix(&h[i..i + w], 16).ok().map(|x| if w == 1 { x * 17 } else { x } as f64);
    let (r, g, b) = if h.len() == 3 { (v(0, 1), v(1, 1), v(2, 1)) } else if h.len() == 6 { (v(0, 2), v(2, 2), v(4, 2)) } else { return false };
    match (r, g, b) {
        (Some(r), Some(g), Some(b)) => 0.299 * r + 0.587 * g + 0.114 * b > 160.0,
        _ => false,
    }
}

pub(super) fn short(rid: &str) -> i64 {
    rid.split_once('/').and_then(|(_, n)| n.parse().ok()).unwrap_or(0)
}

pub(super) fn coords(g: &Value) -> Vec<(f64, f64)> {
    g["coordinates"].as_array().into_iter().flatten().filter_map(|c| Some((c[0].as_f64()?, c[1].as_f64()?))).collect()
}

pub(crate) fn haversine_len(c: &[(f64, f64)]) -> f64 {
    c.windows(2)
        .map(|w| {
            let ((x1, y1), (x2, y2)) = (w[0], w[1]);
            let dx = (x2 - x1).to_radians() * ((y1 + y2) / 2.0).to_radians().cos();
            let dy = (y2 - y1).to_radians();
            dx.hypot(dy) * 6371.0088
        })
        .sum()
}

/// polygon area in hectares from lon/lat (local equirectangular scale at the centroid), as build_tiles.py area_ha
pub(super) fn area_ha(g: &Value) -> f64 {
    let Some(geo) = geomx::from_geojson(g, geomx::id) else { return 0.0 };
    let lat = geomx::centroid(&geo).map_or(0.0, |c| c.y());
    round(geomx::area(&geo) * 111320.0 * lat.to_radians().cos() * 110540.0 / 10000.0, 2)
}

pub(super) fn fnv1a(s: &str) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    for b in s.as_bytes() {
        h = (h ^ *b as u32).wrapping_mul(0x01000193);
    }
    h
}

/// shard count for `bytes` of records: a power of two, so a shard holds about SHARD_TARGET_BYTES (1 to 1024)
pub(super) fn shard_count(bytes: usize) -> u32 {
    (bytes.div_ceil(SHARD_TARGET_BYTES).max(1) as u32).next_power_of_two().min(1024)
}

/// Writes `recs` (key, record as JSON text) to `dir/<n>.json`, each file one JSON object, a record in shard `fnv1a(key) % count`.
/// Returns the count (the app reads it from shards.json). Every shard file is written, empty ones as `{}`, so a fetch never 404s.
pub(super) fn write_shards(dir: &Path, recs: &[(String, String)]) -> u32 {
    let n = shard_count(recs.iter().map(|r| r.1.len()).sum());
    let mut out: Vec<String> = vec![String::new(); n as usize];
    for (k, v) in recs {
        let sh = &mut out[(fnv1a(k) % n) as usize];
        sh.push(if sh.is_empty() { '{' } else { ',' });
        sh.push_str(&Value::String(k.clone()).to_string());
        sh.push(':');
        sh.push_str(v);
    }
    for (i, mut sh) in out.into_iter().enumerate() {
        if sh.is_empty() {
            sh.push('{');
        }
        sh.push('}');
        let path = dir.join(format!("{i}.json"));
        fs::write(&path, sh).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }
    n
}

/// one tile input line: {"type":"Feature","properties":...,"geometry":...,"zoom":{"minzoom":..,"maxzoom":..}} ("zoom" is read by tilewriter.rs)
pub(super) fn feat(out: &mut String, props: &Map<String, Value>, geom: &str, minzoom: Option<i64>, maxzoom: Option<i64>) {
    out.push_str("{\"type\":\"Feature\",\"properties\":");
    out.push_str(&serde_json::to_string(props).unwrap());
    out.push_str(",\"geometry\":");
    out.push_str(geom);
    if minzoom.is_some() || maxzoom.is_some() {
        out.push_str(",\"zoom\":{");
        if let Some(z) = minzoom {
            out.push_str(&format!("\"minzoom\":{z}"));
        }
        if let Some(z) = maxzoom {
            out.push_str(&format!("{}\"maxzoom\":{z}", if minzoom.is_some() { "," } else { "" }));
        }
        out.push('}');
    }
    out.push_str("}\n");
}

pub(super) fn write_json(path: &Path, v: &Value) {
    fs::write(path, v.to_string()).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

/// a string property of a feature, when it is there and not empty
pub(super) fn nonempty(p: &Map<String, Value>, key: &str) -> Option<String> {
    p.get(key).and_then(Value::as_str).filter(|x| !x.is_empty()).map(String::from)
}
