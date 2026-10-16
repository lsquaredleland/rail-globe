//! Tests of exemptions (config `exemptions`, docs/12-config-and-exemptions.md); a child module of classify.rs.

use super::*;
use serde_json::json;

fn cfg_with(exemptions: Value) -> Cfg {
    let dir = std::env::temp_dir().join(format!("rail-classify-test-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("t.json");
    std::fs::write(&p, json!({ "exemptions": exemptions }).to_string()).unwrap();
    Cfg::load(&p, &dir)
}

/// the ways (name, usage) as rail tracks of two points each, way/1, way/2, ...
fn run(cfg: &Cfg, ways: &[(&str, &str)], rows: &[RelRow]) -> (Out, Vec<Value>) {
    let mut feats = Vec::new();
    let mut spans = Vec::new(); // (start, end, properties close)
    for i in 0..ways.len() {
        let line = format!("{{\"type\":\"Feature\",\"properties\":{{\"osm_id\":\"way/{}\"}},\"geometry\":null}}\n", i + 1);
        spans.push((feats.len(), feats.len() + line.len(), feats.len() + line.find("},\"geometry\"").unwrap()));
        feats.extend_from_slice(line.as_bytes());
    }
    let tracks: Vec<TrackIn> = ways
        .iter()
        .enumerate()
        .map(|(i, (n, u))| {
            let mut t: Vec<(&[u8], &[u8])> = vec![(b"railway", b"rail"), (b"name", n.as_bytes())];
            if !u.is_empty() {
                t.push((b"usage", u.as_bytes()));
            }
            TrackIn {
                start: spans[i].0,
                end: spans[i].1,
                pos: spans[i].2,
                props: FeatProps { tags: List(t), osm_id: format!("way/{}", i + 1) },
                routes: Vec::new(),
                first: (0, 0),
                ll: vec![(i as i32 * 1_000_000, 0), (i as i32 * 1_000_000, 10_000)], // apart, so no neighbour smoothing
            }
        })
        .collect();
    let out = classify("t", cfg, rows, &feats, &tracks, &FxHashSet::default(), &[]);
    let typed = String::from_utf8(out.typed.clone()).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    (out, typed)
}

#[test]
fn exemptions_win_drop_and_go_stale() {
    let cfg = cfg_with(json!([
        { "id": "alpha-not-freight", "match": { "osm_id": "way/1", "type": "freight" }, "action": { "freight": false }, "reason": "test" },
        { "id": "drop-beta", "match": { "name_re": "^Beta$" }, "action": { "drop": true }, "reason": "test" },
        { "id": "gamma-metro", "match": { "osm_ids": ["way/3", "way/9"] }, "action": { "type": "metro" }, "reason": "test" },
        { "id": "stale", "match": { "name_re": "^Nope$" }, "action": { "type": "metro" }, "reason": "test", "expect_min": 2 },
        { "id": "wrong-was", "match": { "name_re": "^Gamma$", "type": "freight" }, "action": { "type": "other" }, "reason": "test", "expect_min": 0 }
    ]));
    let (out, typed) = run(&cfg, &[("Alpha", "industrial"), ("Beta", ""), ("Gamma", "")], &[]);
    // Beta is dropped; Alpha goes from freight (its own tags) to Regional; Gamma takes the type of the exemption whose condition holds
    assert_eq!(typed.len(), 2);
    assert_eq!((typed[0]["properties"]["type"].as_str(), typed[0]["properties"]["type_source"].as_str()), (Some("regional"), Some("exemption")));
    assert_eq!((typed[1]["properties"]["osm_id"].as_str(), typed[1]["properties"]["type"].as_str()), (Some("way/3"), Some("metro")));
    assert_eq!(out.stale, vec!["stale exemption \"stale\" matched 0 features, expected at least 2".to_string()]);
    let rep: Value = serde_json::from_slice(&out.report).unwrap();
    assert_eq!(rep["exemptions"]["alpha-not-freight"]["matched"], 1);
    assert_eq!(rep["exemptions"]["alpha-not-freight"]["expect_min_met"], true);
    assert_eq!(rep["exemptions"]["stale"]["expect_min_met"], false);
    assert_eq!(rep["exemptions"]["wrong-was"]["matched"], 0);
    assert!(rep["exemptions"]["drop-beta"]["km"].as_f64().unwrap() > 0.0);
}

#[test]
fn exemptions_retype_lines_and_no_exemptions_no_section() {
    let rows = [RelRow {
        id: "relation/9".into(),
        route: true,
        master: false,
        tags: Owned(vec![("route".into(), "train".into()), ("name".into(), "Line".into())]),
        rel_members: vec![],
    }];
    let cfg = cfg_with(json!([{ "id": "line-intercity", "match": { "osm_id": "relation/9" }, "action": { "type": "intercity" }, "reason": "test" }]));
    let (out, _) = run(&cfg, &[], &rows);
    let lines: Vec<Value> = serde_json::from_slice(&out.lines).unwrap();
    assert_eq!(lines[0]["type"], "intercity");
    assert_eq!(lines[0]["type_source"], "exemption");
    let rep: Value = serde_json::from_slice(&out.report).unwrap();
    assert_eq!(rep["exemptions"]["line-intercity"]["lines"], 1);
    let (out, _) = run(&cfg_with(json!([])), &[], &rows);
    assert!(serde_json::from_slice::<Value>(&out.report).unwrap().get("exemptions").is_none());
}

#[test]
#[should_panic(expected = "reason is required")]
fn exemption_needs_a_reason() {
    Exemption::parse(&json!({ "id": "x", "match": { "name_re": "a" }, "action": { "drop": true } }));
}

#[test]
#[should_panic(expected = "must look like way/123")]
fn exemption_osm_id_must_be_namespaced() {
    Exemption::parse(&json!({ "id": "x", "match": { "osm_id": "123" }, "action": { "drop": true }, "reason": "r" }));
}

#[test]
#[should_panic(expected = "action must be exactly one of")]
fn exemption_action_must_be_one_thing() {
    Exemption::parse(&json!({ "id": "x", "match": { "name_re": "a" }, "action": { "drop": true, "freight": true }, "reason": "r" }));
}

#[test]
#[should_panic(expected = "used twice")]
fn exemption_ids_are_unique() {
    let e = json!({ "id": "x", "match": { "name_re": "a" }, "action": { "drop": true }, "reason": "r" });
    cfg_with(json!([e.clone(), e]));
}
