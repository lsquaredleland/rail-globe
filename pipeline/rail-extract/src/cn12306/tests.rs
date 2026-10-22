use super::graph::{hav, load, maxspeed_kmh};
use super::route::{pick, route, snap_on_seg, snaps, too_fast_pick, Net, Scratch, Status};
use super::stations::{pairs, place_stops, Row, Table, Train};
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

fn way(id: &str, extra: &str, coords: &[[f64; 2]]) -> String {
    format!(r#"{{"type":"Feature","properties":{{"railway":"rail","osm_id":"{id}"{extra}}},"geometry":{{"type":"LineString","coordinates":{coords:?}}}}}"#)
}

fn temp(name: &str, lines: &[String]) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("cn12306-test-{}-{name}.geojsonl", std::process::id()));
    std::fs::write(&p, lines.join("\n")).unwrap();
    p
}

#[test]
fn ways_join_within_two_metres_and_the_route_runs_between_snapped_points() {
    // way/2 starts one vertex unit (about 1 m) off the end of way/1; a way with no maxspeed takes its neighbour's speed (same name)
    let f = temp(
        "join",
        &[
            way("way/1", r#","maxspeed":"100","name":"X""#, &[[100.0, 30.0], [100.1, 30.0]]),
            way("way/2", r#","name":"X""#, &[[100.10001, 30.00001], [100.2, 30.0]]),
            way("way/3", r#","service":"siding""#, &[[100.2, 30.0], [100.2, 30.01]]),
        ],
    );
    let g = load(&f);
    std::fs::remove_file(&f).unwrap();
    assert_eq!(g.ways.len(), 3);
    assert_eq!(g.n_nodes(), 4, "the two ends within a unit are one node");
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
    assert!((c.km - hav(100.03, 30.0, 100.17, 30.0)).abs() < 0.01, "{} {:?} {:?}", c.km, c.geom, c.wk);
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
    let a = key(stretches(&[l0.clone(), l1.clone()], &ids));
    let b = key(stretches(&[l1.clone(), l0.clone()], &ids));
    assert_eq!(a, b);
    assert_eq!(a.len(), 4);
    assert!(a.iter().any(|(_, w)| w == &vec![1, 2]));
    // the same ways run the other way round, or twice, make the same stretches
    let c = key(stretches(&[l0.iter().rev().copied().collect(), l1.clone(), l1.clone()], &ids));
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
    let g = load(&f);
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
    assert_eq!(c.wk.iter().map(|w| w.0).collect::<Vec<_>>(), vec![1], "{:?} {}", c.geom, c.src);
}

#[test]
fn a_long_segment_is_found_from_a_station_far_from_its_vertices() {
    let lines = [way("way/1", "", &[[100.0, 30.0], [100.5, 30.0]])];
    let f = temp("seg", &lines);
    let g = load(&f);
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
    let train = Train { class: "G".into(), stops: vec![stop("A", "", "10:00"), stop("nowhere", "10:20", "10:22"), stop("C", "11:00", "")] };
    let (p, c) = pairs(&[train], &t);
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
    let t1 = Train { class: "G".into(), stops: vec![stop("A", "", "10:00"), stop("B", "10:30", "10:31"), stop("Z", "", "")] };
    let t2 = Train { class: "G".into(), stops: vec![stop("Y", "", "10:00"), stop("B", "10:30", "10:31")] };
    let (p, _) = pairs(&[t1, t2], &t);
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
