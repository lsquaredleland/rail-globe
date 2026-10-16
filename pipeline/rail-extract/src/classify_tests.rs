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

fn cfg_json(v: Value) -> Cfg {
    let dir = std::env::temp_dir().join(format!("rail-classify-test-ev-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("t.json");
    std::fs::write(&p, v.to_string()).unwrap();
    Cfg::load(&p, &dir)
}

/// `passenger_needs_evidence`: unrouted main-line track is Freight unless it runs beside passenger-routed track or its operator is an agency
#[test]
fn passenger_needs_evidence_beside_route_or_agency() {
    // way/1 is on a train route; way/2 runs 11 m beside it; way/3 is 1 km away; way/4 is 1 km away too, operated by Amtrak; way/5 is far and has no usage
    let at = |dlat: i32| vec![(400_000_000 + dlat, -1_200_000_000), (400_000_000 + dlat, -1_197_000_000)]; // 25 km long: too long for the branch-without-station test;
    let ways: Vec<(Vec<&[u8]>, Vec<(i32, i32)>, bool)> = vec![
        (vec![b"usage" as &[u8], b"main"], at(0), true),
        (vec![b"usage", b"main"], at(1_000), false),
        (vec![b"usage", b"main"], at(100_000), false),
        (vec![b"usage", b"main", b"operator", b"Amtrak"], at(200_000), false),
        (vec![b"usage", b"branch"], at(300_000), false),
        (vec![b"usage", b"main"], at(2_700), false), // 30 m beside: outside the 25 m
    ];
    let mut feats = Vec::new();
    let mut spans = Vec::new();
    for i in 0..ways.len() {
        let line = format!("{{\"type\":\"Feature\",\"properties\":{{\"osm_id\":\"way/{}\"}},\"geometry\":null}}\n", i + 1);
        spans.push((feats.len(), feats.len() + line.len(), feats.len() + line.find("},\"geometry\"").unwrap()));
        feats.extend_from_slice(line.as_bytes());
    }
    let tracks: Vec<TrackIn> = ways
        .iter()
        .enumerate()
        .map(|(i, (kv, ll, routed))| {
            let mut t: Vec<(&[u8], &[u8])> = vec![(b"railway", b"rail")];
            t.extend(kv.chunks(2).map(|c| (c[0], c[1])));
            TrackIn {
                start: spans[i].0,
                end: spans[i].1,
                pos: spans[i].2,
                props: FeatProps { tags: List(t), osm_id: format!("way/{}", i + 1) },
                routes: if *routed { vec!["relation/9".to_string()] } else { Vec::new() },
                first: ll[0],
                ll: ll.clone(),
            }
        })
        .collect();
    let rows = [RelRow {
        id: "relation/9".into(),
        route: true,
        master: false,
        tags: Owned(vec![("route".into(), "train".into())]),
        rel_members: vec![],
    }];
    let kinds_at = |cfg: &Cfg, pax: &[(i32, i32)]| -> Vec<(String, String)> {
        let out = classify("t", cfg, &rows, &feats, &tracks, &FxHashSet::default(), pax);
        String::from_utf8(out.typed).unwrap().lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).map(|v| (v["properties"]["type"].as_str().unwrap().to_string(), v["properties"]["type_source"].as_str().unwrap().to_string())).collect()
    };
    let kinds = |cfg: &Cfg| kinds_at(cfg, &[]);
    let off = kinds(&cfg_json(json!({})));
    assert!(off.iter().all(|(_, s)| s != "no-evidence"), "off by default: {off:?}");
    let on = kinds(&cfg_json(json!({ "passenger_needs_evidence": true, "passenger_operator_re": ["(?i)\\bAmtrak\\b"] })));
    assert_eq!(on[0].1, "route");
    assert_eq!(on[1].0, "intercity", "beside the routed way stays passenger: {on:?}");
    assert_eq!(on[2], ("freight".to_string(), "no-evidence".to_string()));
    assert_eq!(on[3].0, "intercity", "an agency operator stays passenger: {on:?}");
    assert_eq!(on[4], ("freight".to_string(), "no-evidence".to_string()));
    assert_eq!(on[5], ("freight".to_string(), "no-evidence".to_string()), "30 m away is not beside: {on:?}");
    // an exemption on `type_source: no-evidence` restores the demoted way to what its tags say, and only those
    let ex = kinds(&cfg_json(json!({
        "passenger_needs_evidence": true,
        "exemptions": [
            { "id": "keep-3", "match": { "osm_id": "way/3", "type_source": "no-evidence" }, "action": { "freight": false }, "reason": "test" },
            { "id": "not-route", "match": { "osm_id": "way/1", "type_source": "no-evidence" }, "action": { "freight": true }, "reason": "test", "expect_min": 0 }
        ]
    })));
    assert_eq!(ex[2], ("intercity".to_string(), "exemption".to_string()));
    assert_eq!(ex[0].1, "route", "type_source decides which ways an exemption touches");
    assert_eq!(ex[4].1, "no-evidence");
    // "stations": a passenger station within 200 m of a way is evidence too (way/3 is at +100_000 units of latitude; the station is 50 m off its middle)
    let station = [(400_000_000 + 100_000 + 450, -1_198_500_000)];
    let routes_mode = kinds_at(&cfg_json(json!({ "passenger_needs_evidence": "routes" })), &station);
    assert_eq!(routes_mode[2], ("freight".to_string(), "no-evidence".to_string()), "routes mode ignores stations: {routes_mode:?}");
    let stations_mode = kinds_at(&cfg_json(json!({ "passenger_needs_evidence": "stations" })), &station);
    assert_eq!(stations_mode[2].0, "intercity", "a station beside the way keeps it: {stations_mode:?}");
    assert_eq!(stations_mode[4], ("freight".to_string(), "no-evidence".to_string()), "no station near way/5: {stations_mode:?}");
}

/// a short Intercity piece in a Regional line takes the line's type even when a siding branches off it (the siding is no vote)
#[test]
fn short_piece_is_smoothed_past_a_siding() {
    let line = |pts: Vec<(i32, i32)>, tags: Vec<(&'static [u8], &'static [u8])>| (pts, tags);
    let main = |u: &'static [u8]| vec![(b"usage" as &[u8], u), (b"railway:traffic_mode", b"mixed")];
    let ways = vec![
        line(vec![(0, 0), (0, 100_000)], main(b"branch")),             // way/1 Regional, 1.1 km
        line(vec![(0, 100_000), (0, 101_000)], main(b"main")),         // way/2 Intercity, 11 m
        line(vec![(0, 101_000), (0, 200_000)], main(b"branch")),       // way/3 Regional
        line(vec![(0, 101_000), (5_000, 101_000)], vec![(b"service" as &[u8], b"siding")]), // way/4 siding at way/2's far end
    ];
    let mut feats = Vec::new();
    let mut spans = Vec::new();
    for i in 0..ways.len() {
        let l = format!("{{\"type\":\"Feature\",\"properties\":{{\"osm_id\":\"way/{}\"}},\"geometry\":null}}\n", i + 1);
        spans.push((feats.len(), feats.len() + l.len(), feats.len() + l.find("},\"geometry\"").unwrap()));
        feats.extend_from_slice(l.as_bytes());
    }
    let tracks: Vec<TrackIn> = ways
        .iter()
        .enumerate()
        .map(|(i, (ll, kv))| {
            let mut t: Vec<(&[u8], &[u8])> = vec![(b"railway", b"rail")];
            t.extend(kv.iter().copied());
            TrackIn { start: spans[i].0, end: spans[i].1, pos: spans[i].2, props: FeatProps { tags: List(t), osm_id: format!("way/{}", i + 1) }, routes: Vec::new(), first: ll[0], ll: ll.clone() }
        })
        .collect();
    let out = classify("t", &cfg_json(json!({})), &[], &feats, &tracks, &FxHashSet::default(), &[]);
    let typed: Vec<Value> = String::from_utf8(out.typed).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let ty = |i: usize| (typed[i]["properties"]["type"].as_str().unwrap().to_string(), typed[i]["properties"]["type_source"].as_str().unwrap().to_string());
    assert_eq!(ty(1), ("regional".to_string(), "neighbour".to_string()), "{typed:?}");
    assert_eq!(ty(3).0, "yard");
}

/// a loop of Intercity track (pieces that only touch each other) joined to Regional track at its ends is part of the Regional line
#[test]
fn intercity_chain_between_regional_track_becomes_regional() {
    let main = |u: &'static [u8]| vec![(b"usage" as &[u8], u), (b"railway:traffic_mode", b"mixed")];
    let seg = |a: i32, b: i32| vec![(0, a), (0, b)];
    let ways = vec![
        (seg(0, 100_000), main(b"branch")),         // way/1 Regional
        (seg(100_000, 200_000), main(b"main")),     // way/2 Intercity
        (seg(200_000, 300_000), main(b"main")),     // way/3 Intercity: a chain of two
        (seg(300_000, 400_000), main(b"branch")),   // way/4 Regional
        (seg(1_000_000, 1_100_000), main(b"main")), // way/5 Intercity, touching nothing
    ];
    let mut feats = Vec::new();
    let mut spans = Vec::new();
    for i in 0..ways.len() {
        let l = format!("{{\"type\":\"Feature\",\"properties\":{{\"osm_id\":\"way/{}\"}},\"geometry\":null}}\n", i + 1);
        spans.push((feats.len(), feats.len() + l.len(), feats.len() + l.find("},\"geometry\"").unwrap()));
        feats.extend_from_slice(l.as_bytes());
    }
    let tracks: Vec<TrackIn> = ways
        .iter()
        .enumerate()
        .map(|(i, (ll, kv))| {
            let mut t: Vec<(&[u8], &[u8])> = vec![(b"railway", b"rail")];
            t.extend(kv.iter().copied());
            TrackIn { start: spans[i].0, end: spans[i].1, pos: spans[i].2, props: FeatProps { tags: List(t), osm_id: format!("way/{}", i + 1) }, routes: Vec::new(), first: ll[0], ll: ll.clone() }
        })
        .collect();
    let out = classify("t", &cfg_json(json!({})), &[], &feats, &tracks, &FxHashSet::default(), &[]);
    let typed: Vec<Value> = String::from_utf8(out.typed).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let ty = |i: usize| typed[i]["properties"]["type"].as_str().unwrap().to_string();
    assert_eq!((ty(1), ty(2)), ("regional".to_string(), "regional".to_string()), "{typed:?}");
    assert_eq!(ty(4), "intercity", "an isolated piece is not part of a line: {typed:?}");
}

/// a high-speed service type applies only to track that is high-speed itself; the rest of such a route is Intercity
#[test]
fn high_speed_route_type_needs_high_speed_track() {
    let rows = [RelRow {
        id: "relation/9".into(),
        route: true,
        master: false,
        tags: Owned(vec![("route".into(), "train".into()), ("service".into(), "high_speed".into())]),
        rel_members: vec![],
    }];
    let ways: Vec<Vec<(&[u8], &[u8])>> = vec![
        vec![(b"railway", b"rail"), (b"maxspeed", b"300")],  // way/1: the LGV itself
        vec![(b"railway", b"rail"), (b"maxspeed", b"120")],  // way/2: conventional line the TGV also uses
        vec![(b"railway", b"rail"), (b"highspeed", b"yes")], // way/3: tagged high-speed
    ];
    let mut feats = Vec::new();
    let mut spans = Vec::new();
    for i in 0..ways.len() {
        let l = format!("{{\"type\":\"Feature\",\"properties\":{{\"osm_id\":\"way/{}\"}},\"geometry\":null}}\n", i + 1);
        spans.push((feats.len(), feats.len() + l.len(), feats.len() + l.find("},\"geometry\"").unwrap()));
        feats.extend_from_slice(l.as_bytes());
    }
    let tracks: Vec<TrackIn> = ways
        .iter()
        .enumerate()
        .map(|(i, t)| TrackIn {
            start: spans[i].0,
            end: spans[i].1,
            pos: spans[i].2,
            props: FeatProps { tags: List(t.clone()), osm_id: format!("way/{}", i + 1) },
            routes: vec!["relation/9".to_string()],
            first: (i as i32 * 1_000_000, 0),
            ll: vec![(i as i32 * 1_000_000, 0), (i as i32 * 1_000_000, 10_000)],
        })
        .collect();
    let out = classify("t", &cfg_json(json!({})), &rows, &feats, &tracks, &FxHashSet::default(), &[]);
    let typed: Vec<Value> = String::from_utf8(out.typed).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let ty = |i: usize| typed[i]["properties"]["type"].as_str().unwrap().to_string();
    assert_eq!((ty(0), ty(1), ty(2)), ("high_speed".to_string(), "intercity".to_string(), "high_speed".to_string()), "{typed:?}");
}
