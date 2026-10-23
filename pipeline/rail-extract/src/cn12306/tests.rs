use super::draw::simplify;
use super::graph::{hav, load, maxspeed_kmh};
use super::records::{build, city_en, city_zh, clean_en, AppStations, Cities};
use super::route::{pick, route, snap_on_seg, snaps, too_fast_pick, Net, Scratch, Status};
use super::stations::{pairs, place_stops, Row, Table, Train};
use super::stations::{RunStop, TrainRun};
use super::stretch::{stretch_id, stretches};

#[test]
fn maxspeed_reads_the_highest_number() {
    assert_eq!(maxspeed_kmh("120"), Some(120.0));
    assert_eq!(maxspeed_kmh("50;80"), Some(80.0));
    assert_eq!(maxspeed_kmh(" 100.5 km/h"), Some(100.5));
    assert!((maxspeed_kmh("30 mph").unwrap() - 48.27).abs() < 1e-9);
    assert_eq!(maxspeed_kmh("none"), None);
    assert_eq!(maxspeed_kmh("signals;60"), Some(60.0));
    assert_eq!(maxspeed_kmh("0"), None);
    assert_eq!(maxspeed_kmh("401"), None);
    assert_eq!(maxspeed_kmh(".5"), None);
    assert_eq!(maxspeed_kmh("1.2.3"), None);
}

#[test]
fn a_point_snaps_onto_the_middle_of_an_edge_or_its_end() {
    let (a, b) = ((100.0, 30.0), (100.02, 30.0));
    let (t, d, p) = snap_on_seg(100.005, 30.001, a, b);
    assert!((t - 0.25).abs() < 1e-9 && (p.0 - 100.005).abs() < 1e-9 && p.1 == 30.0);
    assert!((d - 0.1106).abs() < 1e-3, "{d}"); // 0.001 degrees of latitude
                                               // beyond the end the edge's end is the nearest point
    let (t, _, p) = snap_on_seg(100.03, 30.0, a, b);
    assert_eq!((t, p), (1.0, b));
    // a zero-length edge
    assert_eq!(snap_on_seg(100.0, 30.001, a, a).0, 0.0);
}

pub(super) fn way(id: &str, extra: &str, coords: &[[f64; 2]]) -> String {
    format!(r#"{{"type":"Feature","properties":{{"railway":"rail","osm_id":"{id}"{extra}}},"geometry":{{"type":"LineString","coordinates":{coords:?}}}}}"#)
}

/// writes the features as a cut would, with way_nodes.jsonl beside them: a node id for every distinct vertex, so ways share a node where they
/// share a vertex
pub(super) fn temp(name: &str, lines: &[String]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cn12306-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut ids: std::collections::BTreeMap<String, i64> = Default::default();
    let mut nodes: Vec<u8> = Vec::new();
    for (w, l) in lines.iter().enumerate() {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        let refs: Vec<i64> = v["geometry"]["coordinates"]
            .as_array()
            .map(|cs| {
                cs.iter()
                    .map(|c| {
                        let k = c.to_string();
                        let n = ids.len() as i64 + 1000;
                        *ids.entry(k).or_insert(n)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let id = v["properties"]["osm_id"].as_str().and_then(|x| x.strip_prefix("way/")).and_then(|x| x.parse().ok()).unwrap_or(w as i64);
        crate::cut::encode_way_nodes(&mut nodes, id, &refs);
    }
    std::fs::write(dir.join("way_nodes.jsonl"), nodes).unwrap();
    let p = dir.join("features.geojsonl");
    std::fs::write(&p, lines.join("\n")).unwrap();
    p
}

#[test]
fn ways_share_a_node_and_the_route_runs_between_snapped_points() {
    // way/2 starts at the last node of way/1; a way with no maxspeed takes its neighbour's speed (same name)
    let f = temp(
        "join",
        &[
            way("way/1", r#","maxspeed":"100","name":"X""#, &[[100.0, 30.0], [100.1, 30.0]]),
            way("way/2", r#","name":"X""#, &[[100.1, 30.0], [100.2, 30.0]]),
            way("way/3", r#","service":"siding""#, &[[100.2, 30.0], [100.2, 30.01]]),
        ],
    );
    let g = load(&f).unwrap();
    std::fs::remove_file(&f).unwrap();
    assert_eq!(g.ways.len(), 3);
    assert_eq!(g.n_nodes(), 4, "the shared node is one node");
    assert_eq!(g.ways[1].speed, 100.0, "inferred from the neighbour of the same name");
    assert_eq!(g.ways[2].speed, 40.0);
    assert!(g.comp.iter().all(|&c| c == 0));

    // stations beside each way, far from the other: the edge is split there, so the path is the distance between the two points
    let (a, b) = ((100.03, 30.0003), (100.17, 30.0002));
    let sn = snaps(&g, a.0, a.1, 4, 3.0);
    assert_eq!(sn[0].way, 0);
    assert!(sn[0].t > 0.0 && sn[0].t < 1.0);
    let mut net = Net::new(&g);
    assert!(net.add(&sn[0]) as usize >= g.n_nodes(), "a point inside an edge is a new node");
    let mut sc = Scratch::new(&g);
    let r = route(&g, &mut sc, a, b, Some(30.0));
    assert_eq!(r.status, Status::Ok);
    let c = r.pick.unwrap();
    assert!((c.km - hav(100.03, 30.0, 100.17, 30.0)).abs() < 0.01, "{} {:?}", c.km, c.wk);
    assert_eq!(c.wk.iter().map(|w| w.0).collect::<Vec<_>>(), vec![0, 1]);
    // the minimum time: 4 minutes plus the km at each way's speed
    assert!((c.min_time - (4.0 + c.km / 100.0 * 60.0)).abs() < 0.01);
}

#[test]
fn the_fastest_time_picks_the_corridor_that_fits_with_the_least_slack() {
    // (km, minimum minutes)
    let c = [(100.0, 50.0), (104.0, 55.0), (130.0, 60.0)];
    assert_eq!(pick(&c, Some(60.0)), (Some(1), ""), "within 1.05x of the shortest fit and the least slack wins");
    assert_eq!(pick(&c, Some(50.0)), (Some(0), ""), "only the first fits");
    // 106 km is more than 1.05x of 100 km: its tight slack does not count
    assert_eq!(pick(&[(100.0, 50.0), (106.0, 59.0)], Some(60.0)), (Some(0), ""));
    // the shortest does not fit; the shortest that fits is the base of the guard
    assert_eq!(pick(&[(90.0, 70.0), (100.0, 55.0), (104.0, 59.0), (106.0, 59.5)], Some(60.0)), (Some(2), ""));
    // nothing fits: the least minimum time, flagged
    assert_eq!(pick(&c, Some(40.0)), (Some(0), "too_fast"));
    assert_eq!(pick(&[(100.0, 50.0), (110.0, 45.0)], Some(40.0)), (Some(1), "too_fast"));
    // no time at all: the shortest
    assert_eq!(pick(&[(110.0, 50.0), (100.0, 55.0)], None), (Some(1), "no_time"));
    assert_eq!(pick(&[], Some(10.0)), (None, "no_candidate"));
}

#[test]
fn stretch_ids_are_the_sha1_of_the_sorted_way_ids() {
    let sha = |s: &str| super::stretch::stretch_id(&mut [s]);
    assert_eq!(sha("abc"), 0xa999_3e36_4706_816a, "SHA-1 test vector");
    assert_eq!(stretch_id(&mut ["way/2", "way/10", "way/1"]), stretch_id(&mut ["way/1", "way/10", "way/2"]));
}

#[test]
fn stretches_do_not_depend_on_the_order_of_the_legs() {
    let ids: Vec<String> = (0..6).map(|i| format!("way/{}", 100 + i)).collect();
    let ids: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    // leg 0 runs over ways 0 to 3, leg 1 shares 1 and 2 and then goes on over 4: ways 1 and 2 are one stretch, 0 and 3 and 4 are alone
    let (l0, l1) = (vec![0, 1, 2, 3], vec![1, 2, 4]);
    let key = |st: Vec<super::stretch::Stretch>| st.into_iter().map(|s| (s.id, s.ways)).collect::<Vec<_>>();
    let a = key(stretches(&[l0.clone(), l1.clone()], &[0, 1], &ids).unwrap());
    let b = key(stretches(&[l1.clone(), l0.clone()], &[1, 0], &ids).unwrap());
    assert_eq!(a, b);
    assert_eq!(a.len(), 4);
    assert!(a.iter().any(|(_, w)| w == &vec![1, 2]));
    // the same ways run the other way round, or twice, make the same stretches
    let c = key(stretches(&[l0.iter().rev().copied().collect(), l1.clone(), l1.clone()], &[0, 1, 1], &ids).unwrap());
    assert_eq!(c.iter().filter(|(_, w)| w.len() == 2).count(), 1);
    assert_eq!(c, a);
    assert_eq!(c.len(), 4, "a duplicate leg adds legs to a way but changes no run");
}

fn table(rows: &[(&str, &[(f64, f64)], bool, bool)]) -> Table {
    rows.iter()
        .map(|(n, pts, trusted, candidate)| (n.to_string(), pts.iter().map(|&p| Row { pt: Some(p), trusted: *trusted, candidate: *candidate }).collect()))
        .collect()
}

#[test]
fn a_name_with_two_places_takes_the_one_nearest_its_neighbours_and_a_misplaced_stop_is_dropped() {
    let t = table(&[
        ("A", &[(104.0, 30.0)], true, false),
        ("B", &[(120.0, 40.0), (104.5, 30.2)], false, true),
        ("C", &[(105.0, 30.5)], true, false),
        ("far", &[(80.0, 45.0)], false, false),
    ]);
    let p = place_stops(&t, &["A", "B", "C"]);
    assert_eq!(p[1], Some((104.5, 30.2)));
    // a stop that does not sit between its neighbours is dropped, an unknown name has no place
    let p = place_stops(&t, &["A", "B", "far", "C", "nowhere"]);
    assert_eq!(p, vec![Some((104.0, 30.0)), Some((104.5, 30.2)), None, Some((105.0, 30.5)), None]);
}

#[test]
fn when_nothing_fits_the_shortest_wins_unless_a_much_faster_tagged_one_exists() {
    // (km, minutes, tagged share)
    assert_eq!(too_fast_pick(&[(100.0, 40.0, 0.5), (150.0, 30.0, 0.95)]), 1, "25% faster and tagged");
    assert_eq!(too_fast_pick(&[(100.0, 40.0, 0.5), (150.0, 30.0, 0.8)]), 0, "not tagged enough");
    assert_eq!(too_fast_pick(&[(100.0, 40.0, 0.5), (150.0, 32.0, 0.95)]), 0, "only 20% faster");
    assert_eq!(too_fast_pick(&[(150.0, 30.0, 0.95), (100.0, 40.0, 0.5)]), 0, "input order does not matter");
}

fn route_over(name: &str, lines: &[String], a: (f64, f64), b: (f64, f64), fs: Option<f64>) -> super::route::Routed {
    let f = temp(name, lines);
    let g = load(&f).unwrap();
    std::fs::remove_file(&f).unwrap();
    let mut sc = Scratch::new(&g);
    route(&g, &mut sc, a, b, fs)
}

#[test]
fn a_gap_is_crossed_only_when_the_track_gives_no_path_within_the_audits() {
    // the straight line has a 0.15 km hole between 100.1 and 100.1015; a loop 0.005 degrees north goes round it
    let line = [way("way/1", "", &[[100.0, 30.0], [100.09, 30.0], [100.1, 30.0]]), way("way/2", "", &[[100.1015, 30.0], [100.11, 30.0], [100.2, 30.0]])];
    let loop_ = way("way/3", "", &[[100.09, 30.0], [100.09, 30.005], [100.11, 30.005], [100.11, 30.0]]);
    let (a, b) = ((100.03, 30.0), (100.17, 30.0));
    // with the loop the ends are one piece of track and the loop is within 1.5x the straight line: even a time the loop cannot meet
    // (14 km in half a minute) is not a reason to bridge
    let with_loop = [line[0].clone(), line[1].clone(), loop_];
    for (name, fs) in [("gap-loop", 100.0), ("gap-loop-fast", 0.5)] {
        let r = route_over(name, &with_loop, a, b, Some(fs));
        assert_eq!(r.status, Status::Ok, "{name}");
        assert!(r.pick.unwrap().km > 14.0);
    }
    // without it there is no path: the hole is crossed
    let r = route_over("gap-cross", &line, a, b, Some(100.0));
    assert_eq!(r.status, Status::Bridged);
    let c = r.pick.unwrap();
    assert!(c.km < 14.0 && (c.gap_km - 0.145).abs() < 0.03, "{} {}", c.km, c.gap_km);
}

#[test]
fn two_stations_on_one_long_segment_are_joined_directly() {
    let lines = [way("way/1", "", &[[100.0, 30.0], [100.2, 30.0]])];
    let r = route_over("same-seg", &lines, (100.05, 30.0005), (100.12, 30.0005), Some(60.0));
    let c = r.pick.unwrap();
    assert!((c.km - hav(100.05, 30.0, 100.12, 30.0)).abs() < 0.01, "{}", c.km);
}

#[test]
fn the_way_credited_is_the_one_the_search_used() {
    // two ways between the same two nodes: the service one costs 3x, so the path uses the other, listed second
    let lines = [way("way/1", r#","service":"siding""#, &[[100.0, 30.0], [100.05, 30.0]]), way("way/2", "", &[[100.0, 30.0], [100.05, 30.0]])];
    let r = route_over("credit", &lines, (100.0, 30.0), (100.05, 30.0), Some(60.0));
    let c = r.pick.unwrap();
    assert_eq!(c.wk.iter().map(|w| w.0).collect::<Vec<_>>(), vec![1], "{}", c.src);
}

#[test]
fn a_long_segment_is_found_from_a_station_far_from_its_vertices() {
    let lines = [way("way/1", "", &[[100.0, 30.0], [100.5, 30.0]])];
    let f = temp("seg", &lines);
    let g = load(&f).unwrap();
    std::fs::remove_file(&f).unwrap();
    // 25 km from both vertices, 0.5 km from the line
    let s = snaps(&g, 100.25, 30.0045, 4, 3.0);
    assert_eq!(s.len(), 1);
    assert!((s[0].dist - 0.5).abs() < 0.01);
}

#[test]
fn a_stop_without_a_place_is_passed_over_and_the_times_add_up() {
    let t = table(&[("A", &[(104.0, 30.0)], false, false), ("C", &[(105.0, 30.5)], false, false)]);
    let stop = |n: &str, a: &str, d: &str| (n.to_string(), a.to_string(), d.to_string());
    let train = Train { code: "G1".into(), class: "G".into(), stops: vec![stop("A", "", "10:00"), stop("nowhere", "10:20", "10:22"), stop("C", "11:00", "")] };
    let (p, _, c) = pairs(&[train], &t);
    assert_eq!((p.len(), c.unplaced_stops, c.spanning_pairs), (1, 1, 1), "A-C over the unplaced stop");
    assert_eq!((p[0].a.as_str(), p[0].b.as_str(), p[0].fs), ("A", "C", Some(60.0)));
}

#[test]
fn two_places_of_one_name_are_two_pairs_and_a_stop_without_times_adds_none() {
    let t = table(&[
        ("A", &[(104.0, 30.0)], false, false),
        ("B", &[(104.5, 30.2), (110.0, 35.0)], false, true),
        ("Z", &[(104.6, 30.3)], false, false),
        ("Y", &[(110.1, 35.1)], false, false),
    ]);
    let stop = |n: &str, a: &str, d: &str| (n.to_string(), a.to_string(), d.to_string());
    let t1 = Train { code: "G1".into(), class: "G".into(), stops: vec![stop("A", "", "10:00"), stop("B", "10:30", "10:31"), stop("Z", "", "")] };
    let t2 = Train { code: "G1".into(), class: "G".into(), stops: vec![stop("Y", "", "10:00"), stop("B", "10:30", "10:31")] };
    let (p, _, _) = pairs(&[t1, t2], &t);
    let names: Vec<_> = p.iter().map(|x| (x.a.as_str(), x.b.as_str(), x.fs)).collect();
    assert_eq!(names, vec![("A", "B", Some(30.0)), ("B", "Y", Some(30.0)), ("B", "Z", None)]);
    assert!(p[0].pb.0 < 105.0 && p[1].pa.0 > 109.0, "each B keeps its own place");
}

#[test]
fn a_detour_the_schedule_cannot_follow_is_bridged_when_the_bridge_is_much_shorter() {
    // the same hole, but the loop round it is 0.02 degrees north: 1.33x the straight line
    let lines = [
        way("way/1", "", &[[100.0, 30.0], [100.09, 30.0], [100.1, 30.0]]),
        way("way/2", "", &[[100.1015, 30.0], [100.11, 30.0], [100.2, 30.0]]),
        way("way/3", "", &[[100.09, 30.0], [100.09, 30.02], [100.11, 30.02], [100.11, 30.0]]),
    ];
    let (a, b) = ((100.03, 30.0), (100.17, 30.0));
    let r = route_over("detour-slow", &lines, a, b, Some(100.0));
    assert_eq!(r.status, Status::Ok, "the loop fits a slow train");
    let r = route_over("detour-fast", &lines, a, b, Some(10.0));
    assert_eq!((r.status, r.searched), (Status::Bridged, 2));
}

#[test]
fn english_station_and_city_names_are_cleaned() {
    assert_eq!(clean_en("成都东", "Chengdudong"), "Chengdu East");
    assert_eq!(clean_en("北京", "Beijing Railway Station"), "Beijing");
    assert_eq!(clean_en("上海虹桥", "Shanghai-Hongqiao (China Railway)"), "Shanghai Hongqiao");
    assert_eq!(clean_en("广州南", "Guangzhou South Railway Station"), "Guangzhou South");
    assert_eq!(clean_en("西安北", "Xianbei"), "Xian North");
    assert_eq!(city_zh("成都市"), "成都");
    assert_eq!(city_zh("阿坝藏族羌族自治州"), "阿坝");
    assert_eq!(city_zh("伊犁哈萨克自治州 ئىلى"), "伊犁");
    assert_eq!(city_zh("海东市"), "海东");
    assert_eq!(city_en("Chengdu City", "cn-chengdu"), "Chengdu");
    assert_eq!(city_en("", "cn-ba-yan-nur"), "Ba Yan Nur");
}

#[test]
fn stretch_lines_are_simplified_with_shared_segments_once_and_chains_joined() {
    // two lines over the same ground: one chain, the straight middle vertex dropped
    let a = [(100.0, 30.0), (100.5, 30.0001), (101.0, 30.0)];
    let b = [(100.0, 30.0), (101.0, 30.0)];
    let s = simplify(&[&a, &b], 0.006, 3);
    assert_eq!(s, vec![vec![(100000, 30000), (101000, 30000)]]);
    // a Y: two chains meet at a junction and stay separate lines
    let c = [(100.0, 30.0), (100.5, 30.0), (101.0, 30.5)];
    let d = [(100.5, 30.0), (101.0, 29.5)];
    let s = simplify(&[&c, &d], 0.006, 3);
    assert_eq!(s.len(), 3);
}

fn app_dir(name: &str, stations: &[&str], complexes: &[&str]) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cn12306-app-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("stations.geojsonl"), stations.join("\n")).unwrap();
    std::fs::write(d.join("complexes.geojsonl"), complexes.join("\n")).unwrap();
    d
}

fn station(id: &str, complex: &str, name: &str, mode: &str, lon: f64, lat: f64) -> String {
    format!(
        r#"{{"type":"Feature","properties":{{"id":"{id}","complex":"{complex}","name":"{name}","modes":["{mode}"]}},"geometry":{{"type":"Point","coordinates":[{lon},{lat}]}}}}"#
    )
}

fn run_of(stops: &[(&str, f64, f64, i32)]) -> TrainRun {
    let stops: Vec<RunStop> = stops.iter().map(|&(n, lo, la, t)| RunStop { name: n.to_string(), pt: Some((lo, la)), times: Some((t, t)) }).collect();
    TrainRun { code: "G1".into(), class: "G".into(), legs: (1..stops.len()).map(|j| (j - 1, j, 0)).collect(), stops }
}

#[test]
fn ids_never_point_away_from_a_station_that_has_its_own_record() {
    // 甲 and 乙 are two train stations of one complex; a subway station of it has no trains
    let dir = app_dir(
        "ids",
        &[
            &station("node/1", "relation/9", "甲", "train", 104.0, 30.0),
            &station("node/2", "relation/9", "乙 ئ", "train", 104.05, 30.0),
            &station("node/3", "relation/9", "甲地铁", "subway", 104.0, 30.001),
            &station("node/4", "relation/8", "丁", "train", 110.0, 31.0),
            &station("node/5", "relation/8", "戊", "train", 110.02, 31.0),
        ],
        &[
            r#"{"type":"Feature","properties":{"id":"relation/9","station_ids":["node/1","node/2","node/3"]},"geometry":{"type":"Point","coordinates":[104,30]}}"#,
            r#"{"type":"Feature","properties":{"id":"relation/8","station_ids":["node/4","node/5"]},"geometry":{"type":"Point","coordinates":[110,31]}}"#,
        ],
    );
    let app = AppStations::load(&dir).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let cities = Cities::rectangles(&[("一", "One", [103.0, 29.0, 105.0, 31.0]), ("二", "Two", [109.0, 30.0, 111.0, 32.0])]);
    // 甲 - 乙 - 丁 - 戊: the last two are in the other city
    let runs = [run_of(&[("甲", 104.0, 30.0, 600), ("乙", 104.05, 30.0, 610), ("丁", 110.0, 31.0, 700), ("戊", 110.02, 31.0, 710)])];
    let out = build(&runs, &[vec![]], &app, &cities, &Default::default(), &Default::default());
    let keys: Vec<&str> = out.records.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(keys, vec!["node/1", "node/2", "node/4", "node/5"]);
    for k in &keys {
        assert!(!out.ids.contains_key(*k), "{k} has a record of its own");
    }
    assert_eq!(out.ids.get("node/3").map(String::as_str), Some("node/1"), "a station without trains opens the complex's record");
    assert_eq!(out.ids.get("relation/9").map(String::as_str), Some("node/1"));
    // from 甲 the other city counts once and both of its stops count in the breakdown
    let rec: serde_json::Value = serde_json::from_str(&out.records[0].1).unwrap();
    let city = &rec["dest"][0];
    assert_eq!((city[0].as_str(), city[2].as_u64()), (Some("二"), Some(1)));
    assert_eq!(city[4].as_array().unwrap().len(), 2);
}

#[test]
fn a_skipped_stage_leaves_nothing_behind() {
    let r = super::run_checked(std::path::Path::new("/nonexistent/features"), std::path::Path::new("/nonexistent/inputs"), None, None);
    assert!(r.unwrap_err().contains("stations.tsv"));
    let d = std::env::temp_dir().join(format!("cn12306-skip-{}", std::process::id()));
    std::fs::create_dir_all(d.join("cn12306/abcd1234")).unwrap();
    std::fs::write(d.join("shards.json"), r#"{"stations":8,"cn12306":{"rev":"abcd1234","count":1024}}"#).unwrap();
    super::remove_side_files(&d);
    assert!(!d.join("cn12306").exists());
    assert_eq!(std::fs::read_to_string(d.join("shards.json")).unwrap(), r#"{"stations":8}"#);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_stretch_is_the_type_with_most_km_and_yard_or_freight_only_when_nothing_else_is_there() {
    use super::draw::{stretch_type, WayGeo};
    let w = |ty: &str, km: f64| WayGeo { ty: ty.to_string(), coords: vec![], km };
    assert_eq!(stretch_type([w("yard", 5.0), w("freight", 3.0), w("intercity", 1.0)].iter()), "intercity");
    assert_eq!(stretch_type([w("high_speed", 10.0), w("intercity", 12.0), w("high_speed", 4.0)].iter()), "high_speed");
    assert_eq!(stretch_type([w("yard", 2.0), w("freight", 3.0)].iter()), "freight");
    assert_eq!(stretch_type([].iter()), "other");
}

#[test]
fn the_apostrophe_of_xian_stays() {
    assert_eq!(clean_en("西安北", "Xi'anbei"), "Xi'an North");
    assert_eq!(clean_en("西安北", "Xi’anbei"), "Xi’an North");
}

#[test]
fn stretches_are_sets_of_trains_not_of_legs() {
    let ids: Vec<String> = (0..3).map(|i| format!("way/{}", 100 + i)).collect();
    let ids: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    // train 0 stops after way 1 (two legs, the second one starting on way 1 again), train 1 runs through: all three ways carry the same two trains
    let st = stretches(&[vec![0, 1], vec![1, 2], vec![0, 1, 2]], &[0, 0, 1], &ids).unwrap();
    assert_eq!(st.len(), 1);
    assert_eq!((st[0].ways.clone(), st[0].trains.clone()), (vec![0, 1, 2], vec![0, 1]));
}

#[test]
fn a_pair_key_is_the_names_in_order_and_says_whether_the_stops_run_that_way() {
    use super::stations::pair_key;
    let (k1, f1) = pair_key("A", (104.0, 30.0), "B", (105.0, 31.0));
    let (k2, f2) = pair_key("B", (105.0, 31.0), "A", (104.0, 30.0));
    assert_eq!(k1, k2);
    assert_eq!((f1, f2), (true, false));
    assert_eq!(k1, ("A".to_string(), "B".to_string(), [104_000_000, 30_000_000, 105_000_000, 31_000_000]));
    // two places of one name are two keys
    assert_ne!(pair_key("A", (104.0, 30.0), "B", (105.0, 31.0)).0, pair_key("A", (104.0, 30.0), "B", (110.0, 35.0)).0);
}

#[test]
fn ways_that_do_not_share_a_node_are_not_joined_however_near_they_come() {
    // way/2 starts 1 m from the end of way/1 (a different node), way/3 crosses the end of way/1 at the same place with its own node ids
    let f = temp("nojoin", &[way("way/1", "", &[[100.0, 30.0], [100.1, 30.0]]), way("way/2", "", &[[100.10001, 30.00001], [100.2, 30.0]])]);
    let g = load(&f).unwrap();
    std::fs::remove_file(&f).unwrap();
    assert_eq!((g.n_nodes(), g.comp.iter().copied().max()), (4, Some(1)), "two components: the 1 m gap is left to the router's stitch");
}

#[test]
fn a_missing_node_file_is_a_clear_failure() {
    let d = std::env::temp_dir().join(format!("cn12306-test-{}-nonodes", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("features.geojsonl"), way("way/1", "", &[[100.0, 30.0], [100.1, 30.0]])).unwrap();
    let r = load(&d.join("features.geojsonl"));
    std::fs::remove_dir_all(&d).unwrap();
    let e = r.err().expect("fails");
    assert!(e.contains("way_nodes.jsonl"), "{e}");
}

#[test]
fn a_duplicate_node_at_a_way_end_on_the_same_level_is_joined_and_listed_nothing_else_is() {
    // A: a way ending at (100.1, 30) with node ids of its own; B starts at the same point with another id (a mapping error): joined.
    // C and D cross in the middle at (100.3, 30.0): mid-way, not joined. E ends at a point where F, a bridge, ends: another level.
    let lines = vec![
        way("way/1", "", &[[100.0, 30.0], [100.1, 30.0]]),
        way("way/2", "", &[[100.1, 30.0], [100.2, 30.0]]),
        way("way/3", "", &[[100.25, 30.0], [100.3, 30.0], [100.35, 30.0]]),
        way("way/4", "", &[[100.3, 29.95], [100.3, 30.0], [100.3, 30.05]]),
        way("way/5", "", &[[100.5, 30.0], [100.6, 30.0]]),
        way("way/6", r#","bridge":"yes""#, &[[100.6, 30.0], [100.7, 30.0]]),
    ];
    let f = temp("dupnodes", &lines);
    // `temp` gives one node id to one point; ways 2, 4 and 6 get ids of their own (the first id moves, so every later one moves with it)
    let dir = f.parent().unwrap();
    let nodes = std::fs::read_to_string(dir.join("way_nodes.jsonl")).unwrap();
    let mut out = String::new();
    for l in nodes.lines() {
        let mut v: Vec<i64> = serde_json::from_str(l).unwrap();
        if [2, 4, 6].contains(&v[0]) {
            v[1] += 50_000;
        }
        out.push_str(&format!("{v:?}\n"));
    }
    std::fs::write(dir.join("way_nodes.jsonl"), out).unwrap();
    let g = load(&f).unwrap();
    std::fs::remove_file(&f).unwrap();
    assert_eq!(g.dup_joins.len(), 1, "{:?}", g.dup_joins);
    assert_eq!((g.dup_joins[0].0.as_str(), g.dup_joins[0].1.as_str()), ("way/1", "way/2"));
}
