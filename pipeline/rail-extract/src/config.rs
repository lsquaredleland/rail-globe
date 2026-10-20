//! Config layering (docs/12-config-and-exemptions.md). A file in pipeline/config/ may say `"extends": "<name>"` (another file of that
//! folder, without `.json`); the parent is resolved first (its own `extends` too), then the file is merged over it:
//!   - objects merge key by key, recursively;
//!   - scalars (and `null`) replace;
//!   - arrays replace, unless the key is written `"+key": [...]`, which appends to the parent's `key`;
//!   - the `extends` key itself is consumed.
//!
//! Every key of a file (with a `+` prefix dropped) must be in KNOWN_KEYS; an unknown one stops the run, naming the file and the key.
//! A region with no file of its own uses `_default.json` (see site.rs); a file only gets `_default.json` beneath it by saying
//! `"extends": "_default"` (directly or through a parent).

use serde_json::{Map, Value};
use std::path::Path;

/// Every top-level key a region config may use, with what reads it (details: docs/12-config-and-exemptions.md, "Config keys"). A key
/// nothing reads must not be added here; remove it from the config instead. `extends` is consumed by `resolve_text`, never merged.
const KNOWN_KEYS: &[&str] = &[
    "_comment",                   // free text, read by nobody
    "china",                      // classify.rs, tiles.rs: China rules (train-number and 高速线/客专 name rules, intercity tier at 200 km/h) and the China-only app styling
    "exemptions",                 // classify.rs: named, justified exceptions to the generic rules
    "high_speed_min_kmh",         // classify.rs, stats.rs: the region's one high-speed floor in km/h (default 200; China 250): classify types maxspeed at or above it High-speed, stats.json carries it as hs_min_kmh
    "high_speed_networks_re",     // classify.rs: route network/name patterns that make a route High-speed
    "high_speed_track_overrides", // classify.rs: tracks promoted to High-speed by match rule (name_re, osm_id)
    "main_station_re",            // stations.rs, construction.rs: station names that are always major stations
    "main_terminals",             // stations.rs: terminal station names that count as major
    "passenger_needs_evidence",   // classify.rs: true / "routes" / "stations": passenger rail needs evidence (docs/15)
    "passenger_operator_re",      // classify.rs: operator patterns that count as passenger evidence
    "passenger_stations_file",    // classify.rs: a text file (beside the config) of passenger station names, one per line
    "proposed_region",            // classify.rs: outline file in pipeline/regions inside which proposed track is kept
    "stat_areas",                 // stats.rs: extra areas (slug, name, local) for the region card
    "stat_auto",                  // stats.rs, site.rs: areas picked automatically from data/regions/index.json
    "stat_local",                 // stats.rs, site.rs: the region's name in its own language
    "stat_name",                  // stats.rs, site.rs: the region's English name
    "track_type_overrides",       // classify.rs: retype track (not routes) by match rule
    "type_overrides",             // classify.rs: retype route lines by match rule
    "unrouted_main_type",         // classify.rs: type of main track on no route (default intercity)
];

/// The first key of `own` (one config file, before merging) that is not in KNOWN_KEYS, as an error message.
fn check_keys(name: &str, own: &Map<String, Value>) -> Result<(), String> {
    match own.keys().find(|k| !KNOWN_KEYS.contains(&k.strip_prefix('+').unwrap_or(k))) {
        Some(k) => Err(format!("{name}.json: unknown key \"{k}\" (a typo, or a key nothing reads any more; the known keys are in config.rs KNOWN_KEYS)")),
        None => Ok(()),
    }
}

/// how a config flag such as `"china"` is read: false, 0, "", "false", null and empty lists or objects are off
pub fn truthy(x: &Value) -> bool {
    match x {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty() && s != "false",
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::Null => false,
    }
}

/// Merge `over` onto `base` with the rules above. Errors when a `+key` is not an array.
pub fn merge(base: &mut Map<String, Value>, over: Map<String, Value>) -> Result<(), String> {
    for (k, v) in over {
        if let Some(key) = k.strip_prefix('+') {
            let Value::Array(add) = v else { return Err(format!("\"{k}\" must be an array (it appends to \"{key}\")")) };
            match base.get_mut(key) {
                Some(Value::Array(a)) => a.extend(add),
                _ => {
                    base.insert(key.to_string(), Value::Array(add));
                }
            }
            continue;
        }
        match (base.get_mut(&k), v) {
            (Some(Value::Object(b)), Value::Object(o)) => merge(b, o)?,
            (_, Value::Object(o)) => {
                let mut fresh = Map::new(); // an object with no parent: still resolves its own "+key" entries
                merge(&mut fresh, o)?;
                base.insert(k, Value::Object(fresh));
            }
            (_, v) => {
                base.insert(k, v);
            }
        }
    }
    Ok(())
}

fn resolve(dir: &Path, name: &str, strict: bool, stack: &mut Vec<String>) -> Result<Map<String, Value>, String> {
    if stack.iter().any(|s| s == name) {
        return Err(format!("config extends cycle: {} -> {name}", stack.join(" -> ")));
    }
    let path = dir.join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).map_err(|e| match stack.last() {
        Some(child) => format!("config {child}.json extends \"{name}\", but {} cannot be read: {e}", path.display()),
        None => format!("read {}: {e}", path.display()),
    })?;
    resolve_text(dir, name, &text, strict, stack)
}

fn resolve_text(dir: &Path, name: &str, text: &str, strict: bool, stack: &mut Vec<String>) -> Result<Map<String, Value>, String> {
    let Value::Object(mut own) = serde_json::from_str::<Value>(text).map_err(|e| format!("parse {name}.json: {e}"))? else {
        return Err(format!("{name}.json must be a JSON object"));
    };
    stack.push(name.to_string());
    let extends = own.remove("extends");
    if strict {
        check_keys(name, &own)?;
    }
    let mut base = match extends {
        None => Map::new(),
        Some(Value::String(parent)) => resolve(dir, &parent, strict, stack)?,
        Some(_) => return Err(format!("{name}.json: \"extends\" must be a file name without .json")),
    };
    stack.pop();
    merge(&mut base, own).map_err(|e| format!("{name}.json: {e}"))?;
    Ok(base)
}

/// The resolved config of one file: the file merged over its `extends` chain. Panics with a clear message on a missing parent, a cycle, bad JSON
/// or an unknown key
/// (a broken config must stop the run, like every other config error here).
pub fn load_file(path: &Path) -> Map<String, Value> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    resolve(dir, name, true, &mut Vec::new()).unwrap_or_else(|e| panic!("{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp(files: &[(&str, &str)]) -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("rail-config-test-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(&d).unwrap();
        for (n, t) in files {
            std::fs::write(d.join(format!("{n}.json")), t).unwrap();
        }
        d
    }

    fn load(files: &[(&str, &str)], name: &str) -> Result<Map<String, Value>, String> {
        let d = tmp(files);
        resolve(&d, name, false, &mut Vec::new())
    }

    #[test]
    fn scalars_replace_objects_merge() {
        let c = load(&[("a", r#"{"x": 1, "keep": "a", "o": {"p": 1, "q": 2}}"#), ("b", r#"{"extends": "a", "x": 2, "o": {"q": 3, "r": 4}}"#)], "b").unwrap();
        assert_eq!(Value::Object(c), json!({"x": 2, "keep": "a", "o": {"p": 1, "q": 3, "r": 4}}));
    }

    #[test]
    fn arrays_replace_unless_plus_key() {
        let c = load(
            &[("a", r#"{"l": [1, 2], "m": [1], "o": {"l": ["a"]}}"#), ("b", r#"{"extends": "a", "l": [9], "+m": [2, 3], "o": {"+l": ["b"]}, "+fresh": [7]}"#)],
            "b",
        )
        .unwrap();
        assert_eq!(Value::Object(c), json!({"l": [9], "m": [1, 2, 3], "o": {"l": ["a", "b"]}, "fresh": [7]}));
    }

    #[test]
    fn plus_key_must_be_an_array() {
        let e = load(&[("a", r#"{"+l": 1}"#)], "a").unwrap_err();
        assert!(e.contains("must be an array"), "{e}");
    }

    #[test]
    fn chains_resolve_in_order() {
        let c =
            load(&[("a", r#"{"+l": ["a"], "s": "a"}"#), ("b", r#"{"extends": "a", "+l": ["b"]}"#), ("c", r#"{"extends": "b", "+l": ["c"], "s": "c"}"#)], "c")
                .unwrap();
        assert_eq!(Value::Object(c), json!({"l": ["a", "b", "c"], "s": "c"}));
    }

    #[test]
    fn north_american_configs_resolve() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../config");
        for (slug, country, n_type, n_track) in [("us", "US", 5, 3), ("ca", "CA", 2, 3)] {
            let c = load_file(&dir.join(format!("{slug}.json")));
            assert_eq!(c["stat_auto"], json!({"levels": ["4"], "country": country}), "{slug}");
            assert_eq!(c["type_overrides"].as_array().unwrap().len(), n_type, "{slug}");
            assert_eq!(c["track_type_overrides"].as_array().unwrap().len(), n_track, "{slug}");
            assert_eq!(c["high_speed_min_kmh"], 200);
            assert!(c.get("extends").is_none() && c["passenger_operator_re"].is_array(), "{slug}");
        }
    }

    #[test]
    fn unknown_key_names_file_and_key() {
        let d = tmp(&[
            ("a", r#"{"stat_name": "A", "+type_overrides": [], "commuter_networks": []}"#),
            ("b", r#"{"extends": "a", "stat_nmae": "B"}"#),
            ("ok", r#"{"extends": "b2", "+type_overrides": []}"#),
            ("b2", r#"{"stat_name": "A"}"#),
        ]);
        let e = resolve(&d, "b", true, &mut Vec::new()).unwrap_err();
        assert!(e.contains("b.json") && e.contains("\"stat_nmae\""), "{e}");
        std::fs::write(d.join("b.json"), r#"{"extends": "a"}"#).unwrap();
        let e = resolve(&d, "b", true, &mut Vec::new()).unwrap_err();
        assert!(e.contains("a.json") && e.contains("\"commuter_networks\""), "{e}"); // the parent is checked too
        assert!(resolve(&d, "ok", true, &mut Vec::new()).is_ok());
    }

    #[test]
    fn every_shipped_region_config_has_only_known_keys() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../config");
        let mut n = 0;
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            let f = p.file_name().unwrap().to_string_lossy().into_owned();
            // _groups.json and _station-names.json have their own formats and are not region configs
            if f.ends_with(".json") && f != "_groups.json" && f != "_station-names.json" {
                load_file(&p); // panics naming file and key
                n += 1;
            }
        }
        assert!(n > 50, "{n}");
    }

    #[test]
    fn cycle_and_missing_parent_are_errors() {
        let e = load(&[("a", r#"{"extends": "b"}"#), ("b", r#"{"extends": "a"}"#)], "a").unwrap_err();
        assert!(e.contains("cycle") && e.contains("a -> b -> a"), "{e}");
        let e = load(&[("a", r#"{"extends": "nope"}"#)], "a").unwrap_err();
        assert!(e.contains("a.json extends \"nope\""), "{e}");
    }
}
