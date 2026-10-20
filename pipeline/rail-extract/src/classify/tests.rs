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
    let out = classify("t", cfg, rows, &feats, &tracks, &[]);
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
    let rows = [RelRow { id: "relation/9".into(), route: true, master: false, tags: Owned(vec![("route".into(), "train".into())]), rel_members: vec![] }];
    let kinds_at = |cfg: &Cfg, pax: &[(i32, i32)]| -> Vec<(String, String)> {
        let out = classify("t", cfg, &rows, &feats, &tracks, pax);
        String::from_utf8(out.typed)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .map(|v| (v["properties"]["type"].as_str().unwrap().to_string(), v["properties"]["type_source"].as_str().unwrap().to_string()))
            .collect()
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
    let ways = [
        line(vec![(0, 0), (0, 100_000)], main(b"branch")),                                  // way/1 Regional, 1.1 km
        line(vec![(0, 100_000), (0, 101_000)], main(b"main")),                              // way/2 Intercity, 11 m
        line(vec![(0, 101_000), (0, 200_000)], main(b"branch")),                            // way/3 Regional
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
            TrackIn {
                start: spans[i].0,
                end: spans[i].1,
                pos: spans[i].2,
                props: FeatProps { tags: List(t), osm_id: format!("way/{}", i + 1) },
                routes: Vec::new(),
                first: ll[0],
                ll: ll.clone(),
            }
        })
        .collect();
    let out = classify("t", &cfg_json(json!({})), &[], &feats, &tracks, &[]);
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
    let ways = [
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
            TrackIn {
                start: spans[i].0,
                end: spans[i].1,
                pos: spans[i].2,
                props: FeatProps { tags: List(t), osm_id: format!("way/{}", i + 1) },
                routes: Vec::new(),
                first: ll[0],
                ll: ll.clone(),
            }
        })
        .collect();
    let out = classify("t", &cfg_json(json!({})), &[], &feats, &tracks, &[]);
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
    let out = classify("t", &cfg_json(json!({})), &rows, &feats, &tracks, &[]);
    let typed: Vec<Value> = String::from_utf8(out.typed).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let ty = |i: usize| typed[i]["properties"]["type"].as_str().unwrap().to_string();
    assert_eq!((ty(0), ty(1), ty(2)), ("high_speed".to_string(), "intercity".to_string(), "high_speed".to_string()), "{typed:?}");
}

/// the typed ways (tags, routes they list) of one classify call, each a separate 10 km piece; returns (types, lines.json)
fn classify_ways(ways: &[(Vec<(&[u8], &[u8])>, Vec<&str>)], rows: &[RelRow]) -> (Vec<String>, Vec<Value>) {
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
        .map(|(i, (t, r))| TrackIn {
            start: spans[i].0,
            end: spans[i].1,
            pos: spans[i].2,
            props: FeatProps { tags: List(t.clone()), osm_id: format!("way/{}", i + 1) },
            routes: r.iter().map(|x| x.to_string()).collect(),
            first: ((i % 600) as i32 * 100_000, (i / 600) as i32 * 1_000_000),
            ll: vec![((i % 600) as i32 * 100_000, (i / 600) as i32 * 1_000_000), ((i % 600) as i32 * 100_000, (i / 600) as i32 * 1_000_000 + 10_000)],
        })
        .collect();
    let out = classify("t", &cfg_json(json!({})), rows, &feats, &tracks, &[]);
    let typed: Vec<Value> = String::from_utf8(out.typed).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let lines: Vec<Value> = serde_json::from_slice(&out.lines).unwrap();
    (typed.iter().map(|f| f["properties"]["type"].as_str().unwrap().to_string()).collect(), lines)
}

fn route(id: &str, tags: &[(&str, &str)]) -> RelRow {
    let mut t: Vec<(String, String)> = vec![("route".into(), "train".into())];
    t.extend(tags.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    RelRow { id: id.into(), route: true, master: false, tags: Owned(t), rel_members: vec![] }
}

/// railway=narrow_gauge is typed from usage and routes like standard gauge: public use is passenger, industrial is freight, heritage is Other
#[test]
fn narrow_gauge_is_typed_from_usage_and_routes() {
    let ng = |extra: &[(&'static [u8], &'static [u8])]| {
        let mut t: Vec<(&[u8], &[u8])> = vec![(b"railway", b"narrow_gauge")];
        t.extend_from_slice(extra);
        (t, vec![])
    };
    let mut routed = ng(&[]);
    routed.1 = vec!["relation/1"];
    let ways = [
        ng(&[(b"usage", b"industrial")]),
        ng(&[(b"usage", b"tourism")]),
        ng(&[(b"usage", b"branch"), (b"railway:traffic_mode", b"passenger")]), // (a lone dead-end piece with no station would be judged freight)
        ng(&[(b"usage", b"main"), (b"railway:traffic_mode", b"passenger")]),
        ng(&[]),
        routed,
        ng(&[(b"usage", b"main"), (b"service", b"yard")]),
    ];
    let (types, _) = classify_ways(&ways, &[route("relation/1", &[("service", "regional")])]);
    assert_eq!(types, ["freight", "other", "regional", "intercity", "other", "regional", "yard"]);
}

/// service=international;long_distance: the most prominent class any value gives
#[test]
fn route_service_with_several_values() {
    let ty = |svc: &str| {
        let rows = [route("relation/1", &[("service", svc)])];
        let (_, lines) = classify_ways(&[(vec![(b"railway", b"rail")], vec!["relation/1"])], &rows);
        lines[0]["type"].as_str().unwrap().to_string()
    };
    assert_eq!(ty("international;long_distance"), "intercity");
    assert_eq!(ty("regional;tourism"), "regional");
    assert_eq!(ty("commuter; long_distance"), "intercity");
    assert_eq!(ty("international;national"), "intercity");
    assert_eq!(ty("tourism"), "other");
    assert_eq!(ty("car_shuttle"), "other");
    assert_eq!(ty("highspeed"), "high_speed");
}

/// a route relation that is not running yet is not a line, and its ways get no route type
#[test]
fn route_under_construction_is_not_service() {
    for name in ["Tren Alameda-Melipilla (en construcción)", "Sabarimala Line (under construction)", "Santiago-Batuco (proyectado)"] {
        assert!(route_under_construction(&route("relation/1", &[("name", name)]).tags), "{name}");
    }
    assert!(route_under_construction(&route("relation/1", &[("state", "construction")]).tags));
    assert!(!route_under_construction(&route("relation/1", &[("name", "Tren Santiago-Valparaíso")]).tags));
    let ways = [(vec![(&b"railway"[..], &b"rail"[..]), (b"usage", b"main"), (b"railway:traffic_mode", b"passenger")], vec!["relation/1"])];
    let (types, lines) = classify_ways(&ways, &[route("relation/1", &[("name", "Tren Alameda-Melipilla (en construcción)"), ("service", "regional")])]);
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(types, ["intercity"]); // its own tags, not the route's Regional
    let (_, lines) = classify_ways(&ways, &[route("relation/1", &[("name", "Tren Alameda-Melipilla")])]);
    assert_eq!(lines.len(), 1);
}

/// a route is High-speed by track only when most of its length is high-speed track
#[test]
fn route_is_high_speed_by_track_only_for_a_majority() {
    let hs: (Vec<(&[u8], &[u8])>, Vec<&str>) = (vec![(b"railway", b"rail"), (b"highspeed", b"yes")], vec!["relation/1"]);
    let plain: (Vec<(&[u8], &[u8])>, Vec<&str>) = (vec![(b"railway", b"rail")], vec!["relation/1"]);
    let line_type = |ways: &[(Vec<(&[u8], &[u8])>, Vec<&str>)]| classify_ways(ways, &[route("relation/1", &[])]).1[0]["type"].as_str().unwrap().to_string();
    assert_eq!(line_type(&[hs.clone(), plain.clone(), plain.clone()]), "regional"); // one third: a regional train that crosses a high-speed link
    assert_eq!(line_type(&[hs.clone(), hs.clone(), plain.clone()]), "high_speed");
    assert_eq!(line_type(std::slice::from_ref(&hs)), "high_speed");
    // a long route with a little high-speed track (GWR London - Penzance) is Intercity
    let mut long = vec![hs.clone()];
    long.extend(std::iter::repeat_n(plain.clone(), 1600)); // each piece is 0.11 km: 176 km, 0.11 km of it high-speed
    assert_eq!(line_type(&long), "intercity");
    // track under construction counts for neither the total nor the high-speed share
    let building: (Vec<(&[u8], &[u8])>, Vec<&str>) =
        (vec![(b"railway", b"construction"), (b"construction", b"rail"), (b"highspeed", b"yes")], vec!["relation/1"]);
    assert_eq!(line_type(&[plain.clone(), plain.clone(), building.clone(), building.clone(), building.clone()]), "regional");
}

/// open track kinds + construction=* are construction track (West Cairo monorail: railway=monorail + construction=yes); railway=rail needs a rail kind as the value and no running route
#[test]
fn construction_tag_on_an_open_railway_is_construction() {
    let ways: Vec<(&str, &str, &str)> = vec![
        ("monorail", "yes", ""),        // 0: construction, mode monorail
        ("rail", "rail", ""),           // 1: new build (operator, no ref): construction, mode rail
        ("rail", "yes", ""),            // 2: doubling or electrification on a running line: open
        ("rail", "rail", "relation/9"), // 3: on a running route: open
        ("rail", "", ""),               // 4: open
        ("rail", "rail", "ref"),        // 5: leftover on an operating line (has a ref): open
        ("tram", "yes", "relation/9"),  // 6: new-build mode but on a running route: open
    ];
    let mut feats = Vec::new();
    let mut spans = Vec::new();
    for (i, (rw, c, _)) in ways.iter().enumerate() {
        let con = if c.is_empty() { String::new() } else { format!("\"construction\":\"{c}\",") };
        let l = format!("{{\"type\":\"Feature\",\"properties\":{{\"railway\":\"{rw}\",{con}\"osm_id\":\"way/{}\"}},\"geometry\":null}}\n", i + 1);
        spans.push((feats.len(), feats.len() + l.len(), feats.len() + l.find("},\"geometry\"").unwrap()));
        feats.extend_from_slice(l.as_bytes());
    }
    let tracks: Vec<TrackIn> = ways
        .iter()
        .enumerate()
        .map(|(i, (rw, c, r))| {
            let mut t: Vec<(&[u8], &[u8])> = vec![(b"railway", rw.as_bytes()), (b"usage", b"main")];
            if *rw == "rail" && *r != "ref" {
                t.push((b"operator", b"TCDD"));
            }
            if *r == "ref" {
                t.push((b"ref", b"L1"));
            }
            if !c.is_empty() {
                t.push((b"construction", c.as_bytes()));
            }
            TrackIn {
                start: spans[i].0,
                end: spans[i].1,
                pos: spans[i].2,
                props: FeatProps { tags: List(t), osm_id: format!("way/{}", i + 1) },
                routes: if r.is_empty() || *r == "ref" { Vec::new() } else { vec![r.to_string()] },
                first: (i as i32 * 1_000_000, 0),
                ll: vec![(i as i32 * 1_000_000, 0), (i as i32 * 1_000_000, 10_000)],
            }
        })
        .collect();
    let rows = [route("relation/9", &[("service", "regional")])];
    let out = classify("t", &cfg_json(json!({})), &rows, &feats, &tracks, &[]);
    let typed: Vec<Value> = String::from_utf8(out.typed).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let rw = |i: usize| typed[i]["properties"]["railway"].as_str().unwrap().to_string();
    let con = |i: usize| typed[i]["properties"]["construction"].as_str().map(String::from);
    assert_eq!((rw(0), con(0)), ("construction".into(), Some("monorail".into())));
    assert_eq!(typed[0]["properties"]["construction_type"], "monorail");
    assert_eq!(typed[0]["properties"]["type"], "monorail");
    assert_eq!((rw(1), con(1)), ("construction".into(), Some("rail".into())));
    assert_eq!((rw(2), con(2)), ("rail".into(), Some("yes".into())));
    assert_eq!((rw(3), con(3)), ("rail".into(), Some("rail".into())));
    assert_eq!((rw(4), con(4)), ("rail".into(), None));
    assert_eq!((rw(5), con(5)), ("rail".into(), Some("rail".into())));
    assert_eq!((rw(6), con(6)), ("tram".into(), Some("yes".into())));
    // no construction key in the feature text: the mode is appended, not lost
    let t = String::from_utf8(as_construction(b"{\"type\":\"Feature\",\"properties\":{\"railway\":\"monorail\",\"osm_id\":\"way/1\"", "monorail")).unwrap();
    assert!(t.contains("\"railway\":\"construction\"") && t.ends_with(",\"construction\":\"monorail\""), "{t}");
    // the mode may sit under proposed=* on a railway=construction way
    let p = FeatProps { tags: List(vec![(b"railway", b"construction"), (b"proposed", b"monorail")]), osm_id: "way/1".into() };
    assert_eq!(own_tag_type(&p, &cfg_json(json!({}))).0, "monorail");
}
