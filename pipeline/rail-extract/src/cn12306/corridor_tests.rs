//! Tests of the corridor router: its graph and searches, the candidates, the smallest-gap bridge, and the orders that decide ties (a
//! build must give the same files on any thread count).

use super::corridor::{bridge_for_test, candidates_for_test, decide, service_counts};
use super::graph::{self};
use super::model::class_index;
use super::model::{Cand, Shape, NCLASS};
use super::rgraph::hav;
use super::rgraph::{self, bearing, Buffers, Metric, Penalty, RGraph, Snap, SnapIndex, F_STITCH};
use super::stations::{pair_key, RunStop, TrainRun};
use super::tests::{temp, way};

fn net(name: &str, lines: &[String]) -> RGraph {
    let f = temp(name, lines);
    let g = graph::load(&f).unwrap();
    std::fs::remove_file(&f).unwrap();
    rgraph::build(&g)
}

fn node_at(g: &RGraph, lon: f64, lat: f64) -> u32 {
    (0..g.n_nodes() as u32)
        .find(|&n| (g.xy[n as usize].0 - lon).abs() < 1e-6 && (g.xy[n as usize].1 - lat).abs() < 1e-6)
        .unwrap_or_else(|| panic!("no node at {lon} {lat}"))
}

#[test]
fn bearings_are_compass_directions() {
    assert!((bearing((0.0, 0.0), (0.0, 1.0)) - 0.0).abs() < 1e-6, "north");
    assert!((bearing((0.0, 0.0), (1.0, 0.0)) - 90.0).abs() < 1e-6, "east");
    assert!((bearing((0.0, 0.0), (0.0, -1.0)) - 180.0).abs() < 1e-6, "south");
}

#[test]
fn components_within_250_m_are_stitched_once_and_a_dangling_end_has_degree_one() {
    // 0.002 degrees of longitude at 30 N is 0.19 km, 0.0027 is 0.26 km
    let g = net(
        "stitch",
        &[
            way("way/1", r#","maxspeed":"100""#, &[[100.0, 30.0], [100.005, 30.0], [100.01, 30.0]]),
            way("way/2", "", &[[100.012, 30.0], [100.02, 30.0]]),
            way("way/3", "", &[[100.0227, 30.0], [100.03, 30.0]]),
        ],
    );
    assert_eq!(g.n_stitches, 1, "the third way is 0.26 km from the second");
    let (a, b) = (node_at(&g, 100.01, 30.0), node_at(&g, 100.012, 30.0));
    let e = g.edge(a, b).expect("the two near ends are joined");
    assert_eq!((e.flags & F_STITCH, e.vmax, e.way), (F_STITCH, 40, rgraph::NO_WAY));
    assert!((e.km as f64 - hav((100.01, 30.0), (100.012, 30.0))).abs() < 1e-4);
    // degrees are counted before the stitches
    assert_eq!(g.degree[a as usize], 1);
    assert_eq!(g.degree[node_at(&g, 100.005, 30.0) as usize], 2);
    // the stitched edge belongs to no named line
    assert_eq!(g.line(e), graph::NO_LINE);
}

#[test]
fn the_speed_of_a_way_is_its_tag_else_250_on_a_high_speed_line_else_by_usage_and_30_on_service_track() {
    let g = net(
        "speed",
        &[
            way("way/1", r#","maxspeed":"160""#, &[[100.0, 30.0], [100.01, 30.0]]),
            way("way/2", r#","highspeed":"yes""#, &[[100.01, 30.0], [100.02, 30.0]]),
            way("way/3", r#","usage":"branch""#, &[[100.02, 30.0], [100.03, 30.0]]),
            way("way/4", r#","usage":"main""#, &[[100.03, 30.0], [100.04, 30.0]]),
            way("way/5", "", &[[100.04, 30.0], [100.05, 30.0]]),
            way("way/6", r#","service":"yard","maxspeed":"100""#, &[[100.05, 30.0], [100.06, 30.0]]),
            way("way/7", r#","maxspeed":"500""#, &[[100.06, 30.0], [100.07, 30.0]]),
        ],
    );
    let v = |lon: f64| g.edge(node_at(&g, lon, 30.0), node_at(&g, lon + 0.01, 30.0)).unwrap().vmax;
    assert_eq!([v(100.0), v(100.01), v(100.02), v(100.03), v(100.04), v(100.05), v(100.06)], [160, 250, 80, 120, 100, 30, 100], "500 is no speed: the default");
    assert_eq!(g.edge(node_at(&g, 100.01, 30.0), node_at(&g, 100.02, 30.0)).unwrap().flags & rgraph::F_HSR, rgraph::F_HSR);
}

/// two ways of equal length between A and B, one north and one south of the straight line, the search is the same on a fresh buffer and on one
/// that has been used (the stamps), and on a repeat
#[test]
fn a_search_does_not_depend_on_the_state_of_its_buffer() {
    let g = net(
        "buffer",
        &[
            way("way/1", r#","maxspeed":"100""#, &[[100.0, 30.0], [100.05, 30.01], [100.1, 30.0]]),
            way("way/2", r#","maxspeed":"100""#, &[[100.0, 30.0], [100.05, 29.99], [100.1, 30.0]]),
            way("way/3", r#","maxspeed":"100""#, &[[100.1, 30.0], [100.2, 30.0]]),
        ],
    );
    let snapper = SnapIndex::new(&g);
    let (a, b): (Snap, Snap) = (snapper.snap(&g, (100.0, 30.0)), snapper.snap(&g, (100.2, 30.0)));
    let m = Metric::Time { cap: 300.0, hsr_mult: 1.0 };
    let pen = Penalty::default();
    let mut fresh = Buffers::new(g.n_nodes());
    let one = rgraph::route(&g, &mut fresh, &a, &b, (100.2, 30.0), m, &pen, 1e9).unwrap();
    let mut used = Buffers::new(g.n_nodes());
    for _ in 0..5 {
        let _ = rgraph::route(&g, &mut used, &b, &a, (100.0, 30.0), Metric::Distance, &pen, 1e9);
    }
    let two = rgraph::route(&g, &mut used, &a, &b, (100.2, 30.0), m, &pen, 1e9).unwrap();
    let three = rgraph::route(&g, &mut used, &a, &b, (100.2, 30.0), m, &pen, 1e9).unwrap();
    assert_eq!(one.1, two.1);
    assert_eq!(two.1, three.1);
    assert_eq!(one.0.to_bits(), two.0.to_bits());
    // a penalty on the path taken moves the search to the other one
    let mut pen = Penalty::default();
    for w in one.1.windows(2) {
        pen.insert((w[0].min(w[1]), w[0].max(w[1])), 4.0);
    }
    let other = rgraph::route(&g, &mut used, &a, &b, (100.2, 30.0), m, &pen, 1e9).unwrap();
    assert_ne!(other.1, one.1);
}

#[test]
fn a_reach_is_listed_in_ascending_node_order() {
    let g = net(
        "reach",
        &[way("way/1", "", &[[100.0, 30.0], [100.01, 30.0], [100.02, 30.0], [100.03, 30.0]]), way("way/2", "", &[[100.01, 30.0], [100.01, 30.01]])],
    );
    let start: Snap = vec![(node_at(&g, 100.0, 30.0), 0.0)];
    let tree = rgraph::reach(&g, &start, 50.0);
    let nodes = rgraph::reached(&tree);
    assert_eq!(nodes.len(), g.n_nodes());
    assert!(nodes.windows(2).all(|w| w[0] < w[1]));
    let far = node_at(&g, 100.03, 30.0);
    let path = rgraph::reach_path(&tree, far);
    assert_eq!((path[0], *path.last().unwrap(), path.len()), (node_at(&g, 100.0, 30.0), far, 4));
    // the radius is a track distance: 1.5 km reaches the first two vertices only
    assert_eq!(rgraph::reach(&g, &start, 1.5).len(), 2);
}

#[test]
fn the_fastest_corridor_for_a_class_and_the_shortest_are_two_candidates_and_a_conventional_train_is_kept_off_the_high_speed_line() {
    // a direct slow line and a longer high-speed detour 3.3 km north of it
    let lines = [
        way("way/1", r#","maxspeed":"60""#, &[[100.0, 30.0], [100.1, 30.0], [100.2, 30.0]]),
        way("way/2", r#","maxspeed":"300","highspeed":"yes""#, &[[100.0, 30.0], [100.0, 30.03], [100.1, 30.03], [100.2, 30.03], [100.2, 30.0]]),
    ];
    let g = net("cands", &lines);
    let (a, b) = ((100.0, 30.0), (100.2, 30.0));
    let gc = candidates_for_test(&g, a, b, 1 << class_index("G"), None);
    let src: Vec<&str> = gc.iter().map(|c| c.source.as_str()).collect();
    assert!(src.contains(&"fast-G") && src.contains(&"shortest"), "{src:?}");
    let (fast, short) = (gc.iter().find(|c| c.source == "fast-G").unwrap(), gc.iter().find(|c| c.source == "shortest").unwrap());
    assert!(fast.km > short.km && fast.hsr_km > 20.0 && short.hsr_km == 0.0);
    let gi = class_index("G");
    assert!(fast.predict(gi).unwrap() < short.predict(gi).unwrap() / 2.0, "the high-speed line is much faster for a G train");
    // a conventional train may not use the high-speed line
    assert!(fast.predict(class_index("Z")).is_none());
    let zc = candidates_for_test(&g, a, b, 1 << class_index("Z"), None);
    assert!(zc[0].hsr_km == 0.0);
    // the candidates come in the same order every time
    let again = candidates_for_test(&g, a, b, 1 << class_index("G"), None);
    assert_eq!(src, again.iter().map(|c| c.source.as_str()).collect::<Vec<_>>());
    assert_eq!(gc.iter().map(|c| c.nodes.clone()).collect::<Vec<_>>(), again.iter().map(|c| c.nodes.clone()).collect::<Vec<_>>());
    // the stage-1 path is a candidate of its own; the same corridor as one found replaces it and keeps its geometry
    let base = short.nodes.clone();
    let with = candidates_for_test(&g, a, b, 1 << class_index("G"), Some(&base));
    assert_eq!(with.iter().filter(|c| c.is_base).count(), 1);
    assert_eq!(with.len(), gc.len(), "the same corridor as `shortest`");
    assert_eq!(with.iter().find(|c| c.is_base).unwrap().nodes, base);
}

#[test]
fn the_smallest_gap_is_bridged_from_a_dangling_end() {
    // two pieces of track with a hole of 2.9 km between 100.1 and 100.13; a stub off the first piece ends nearer the second piece but
    // is not a dangling end of the main track (it joins at a vertex): its end is dangling too, and its gap is the larger one
    let g = net(
        "gap",
        &[
            way("way/1", "", &[[100.0, 30.0], [100.05, 30.0], [100.1, 30.0]]),
            way("way/2", "", &[[100.13, 30.0], [100.18, 30.0], [100.2, 30.0]]),
            way("way/3", "", &[[100.05, 30.0], [100.05, 30.02]]),
        ],
    );
    let (a, b) = ((100.02, 30.0), (100.18, 30.0));
    let c = bridge_for_test(&g, a, b, Some(20.0)).expect("bridged");
    assert_eq!(c.source, "min-gap-bridge");
    assert_eq!(c.gaps_at.len(), 1);
    assert!((c.gap_km - hav((100.1, 30.0), (100.13, 30.0))).abs() < 0.01, "{}", c.gap_km);
    // the same answer every time
    for _ in 0..5 {
        assert_eq!(bridge_for_test(&g, a, b, Some(20.0)).unwrap().nodes, c.nodes);
    }
    // a schedule that would need more than 355 km/h over the bridged path refuses it
    assert!(bridge_for_test(&g, a, b, Some(2.0)).is_none());
}

/// a double track mapped as two ways 11 m apart (not joined), five stations, eight stopping trains and two nonstop trains
fn synthetic(dir: &std::path::Path) {
    let mut f = Vec::new();
    for k in 0..4 {
        let (x0, x1) = (100.0 + 0.2 * k as f64, 100.2 + 0.2 * k as f64);
        f.push(way(&format!("way/{}", 10 + k), r#","name":"X线","maxspeed":"200""#, &[[x0, 30.0], [x1, 30.0]]));
        f.push(way(&format!("way/{}", 20 + k), r#","name":"X线","maxspeed":"200""#, &[[x0, 30.0001], [x1, 30.0001]]));
    }
    std::fs::create_dir_all(dir).unwrap();
    let p = super::tests::temp("synthetic-features", &f);
    std::fs::copy(&p, dir.join("features.geojsonl")).unwrap();
    std::fs::copy(p.with_file_name("way_nodes.jsonl"), dir.join("way_nodes.jsonl")).unwrap();
    let names = ["A", "B", "C", "D", "E"];
    let tsv: String = names.iter().enumerate().map(|(i, n)| format!("{n}\t{}\t30.00005\tlist\n", 100.0 + 0.2 * i as f64)).collect();
    std::fs::write(dir.join("stations.tsv"), tsv).unwrap();
    let hhmm = |m: i32| format!("{:02}:{:02}", m / 60, m % 60);
    let mut trains = Vec::new();
    for t in 0..8 {
        let t0 = 8 * 60 + 12 * t;
        let stops: Vec<serde_json::Value> = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let (arr, dep) = (t0 + 16 * i as i32, t0 + 16 * i as i32 + 1);
                serde_json::json!([n, if i == 0 { String::new() } else { hhmm(arr) }, if i == 4 { String::new() } else { hhmm(dep) }])
            })
            .collect();
        trains.push(serde_json::json!({ "code": format!("G{}", 100 + t), "class": "G", "stops": stops }));
    }
    for t in 0..2 {
        let t0 = 9 * 60 + 30 * t;
        trains.push(serde_json::json!({ "code": format!("G{}", 200 + t), "class": "G", "stops": [["A", "", hhmm(t0)], ["E", hhmm(t0 + 36), ""]] }));
    }
    std::fs::write(dir.join("timetable.json"), serde_json::to_string(&trains).unwrap()).unwrap();
}

fn run_with_threads(dir: &std::path::Path, out: &std::path::Path, threads: usize) {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
        .install(|| super::run_checked(&dir.join("features.geojsonl"), dir, Some(out), None).unwrap());
}

#[test]
fn double_track_is_one_line_and_the_files_are_the_same_on_any_thread_count() {
    let base = std::env::temp_dir().join(format!("cn12306-corridor-{}", std::process::id()));
    let (inp, o1, o4) = (base.join("in"), base.join("o1"), base.join("o4"));
    synthetic(&inp);
    run_with_threads(&inp, &o1, 1);
    run_with_threads(&inp, &o4, 4);
    for f in ["pairs.json", "stretches.json", "legs.jsonl", "wrong_switches.jsonl", "report.json"] {
        assert_eq!(std::fs::read(o1.join(f)).unwrap(), std::fs::read(o4.join(f)).unwrap(), "{f} differs between 1 and 4 threads");
    }
    // every leg of the 8 x 4 + 2 has a path, and the ten trains share one track: one stretch over the four ways, not a thick and a thin line
    let legs: Vec<serde_json::Value> = std::fs::read_to_string(o1.join("legs.jsonl")).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(legs.len(), 34);
    assert!(legs.iter().all(|l| l["ways"].as_array().is_some_and(|w| !w.is_empty())), "{legs:?}");
    let st: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(o1.join("stretches.json")).unwrap()).unwrap();
    let st = st.as_array().unwrap();
    assert_eq!(st.len(), 1, "{st:?}");
    assert_eq!((st[0]["ways"].as_array().unwrap().len(), st[0]["trains"].as_array().unwrap().len()), (4, 10));
    let r: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(o1.join("report.json")).unwrap()).unwrap();
    let d = &r["corridor"]["decisions"];
    assert_eq!(d.as_object().unwrap().values().map(|v| v.as_u64().unwrap()).sum::<u64>(), 34, "{d}");
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn the_other_track_of_the_same_named_line_lights_with_the_stretch_and_nothing_else_does() {
    use super::stretch::Stretch;
    let f = temp(
        "twins",
        &[
            way("way/1", r#","name":"X线""#, &[[100.0, 30.0], [100.01, 30.0], [100.02, 30.0]]),
            way("way/2", r#","name":"X线""#, &[[100.0, 30.0001], [100.01, 30.0001], [100.02, 30.0001]]),
            way("way/3", r#","name":"Y线""#, &[[100.0, 30.0001], [100.01, 30.00012], [100.02, 30.0001]]),
            way("way/4", r#","name":"X线""#, &[[100.0, 30.005], [100.01, 30.005], [100.02, 30.005]]),
            way("way/5", r#","name":"X线","service":"siding""#, &[[100.0, 30.00008], [100.01, 30.00008]]),
        ],
    );
    let g = graph::load(&f).unwrap();
    std::fs::remove_file(&f).unwrap();
    let st = [Stretch { id: 1, ways: vec![0], trains: vec![0] }];
    assert_eq!(super::sidefiles::twins(&g, &st), vec![vec![1]], "way/2 only: not the other line, not 550 m away, not service track");
}

fn cand(gap_km: f64, is_base: bool, source: &str) -> Cand {
    Cand {
        nodes: Vec::new(),
        km: 100.0,
        gap_km,
        gaps_at: Vec::new(),
        hsr_km: 0.0,
        run_min: [60.0; NCLASS],
        source: source.to_string(),
        is_base,
        head_start: 0.0,
        head_end: 0.0,
        lines: Vec::new(),
    }
}

const OK: f64 = -1.0;
const NOT_FEASIBLE: f64 = -30.0;

#[test]
fn a_leg_keeps_the_stage_1_path_unless_the_evidence_is_decisive() {
    let pc = [cand(0.0, true, "production"), cand(0.0, false, "fast-G")];
    // the stage-1 path is the likeliest
    assert_eq!(decide(&pc, &[0.8, 0.2], &[OK, OK]), (Some(0), Some(0), "production"));
    // another corridor at 0.95 against 0.02: a switch
    assert_eq!(decide(&pc, &[0.02, 0.95], &[OK, OK]), (Some(1), Some(0), "switched"));
    // 0.89 is not enough, and neither is a stage-1 path above 0.05
    assert_eq!(decide(&pc, &[0.03, 0.89], &[OK, OK]), (Some(0), Some(0), "ambiguous-keep"));
    assert_eq!(decide(&pc, &[0.2, 0.8], &[OK, OK]), (Some(0), Some(0), "ambiguous-keep"));
    // nothing fits the schedule: the stage-1 path stays
    assert_eq!(decide(&pc, &[0.02, 0.95], &[NOT_FEASIBLE, NOT_FEASIBLE]), (Some(0), Some(0), "infeasible-keep"));
    assert_eq!(decide(&[], &[], &[]), (None, None, "no-candidates"));
}

#[test]
fn a_bridged_stage_1_path_is_replaced_by_the_smallest_gap_that_fits() {
    // the stage-1 path crosses 5 km of nothing; a candidate with no gap and one with 1 km
    let pc = [cand(5.0, true, "production"), cand(1.0, false, "min-gap-bridge"), cand(0.0, false, "shortest")];
    assert_eq!(decide(&pc, &[0.6, 0.1, 0.3], &[OK, OK, OK]), (Some(2), Some(0), "bridge-fix"), "no gap first");
    // the gap-free one does not fit the schedule: the 1 km bridge (4 km smaller) does
    assert_eq!(decide(&pc, &[0.6, 0.1, 0.3], &[OK, OK, NOT_FEASIBLE]), (Some(1), Some(0), "bridge-fix"));
    // a gain under half a km is no reason to leave
    let pc = [cand(5.0, true, "production"), cand(4.8, false, "min-gap-bridge")];
    assert_eq!(decide(&pc, &[0.6, 0.4], &[OK, OK]), (Some(0), Some(0), "production"));
    // a decisive switch comes before a bridge-fix
    let pc = [cand(5.0, true, "production"), cand(0.0, false, "shortest")];
    assert_eq!(decide(&pc, &[0.01, 0.97], &[OK, OK]), (Some(1), Some(0), "switched"));
}

#[test]
fn a_chord_pair_is_drawn_on_track_when_one_corridor_is_likely_enough() {
    let pc = [cand(3.0, false, "min-gap-bridge"), cand(0.0, false, "shortest")];
    assert_eq!(decide(&pc, &[0.1, 0.9], &[OK, OK]), (Some(1), None, "filled"));
    // under 1 km of gap, 0.35 is enough
    assert_eq!(decide(&pc, &[0.3, 0.7], &[OK, OK]), (Some(1), None, "filled"));
    let pc = [cand(0.0, false, "shortest"), cand(3.0, false, "min-gap-bridge")];
    // a gap of 3 km needs 0.8
    assert_eq!(decide(&pc, &[0.3, 0.7], &[OK, OK]), (None, None, "chord"));
    assert_eq!(decide(&pc, &[0.19, 0.81], &[OK, OK]), (Some(1), None, "filled"));
    // not feasible: a chord
    assert_eq!(decide(&pc, &[0.9, 0.1], &[NOT_FEASIBLE, OK]), (None, None, "chord"));
    assert_eq!(decide(&pc, &[0.9, 0.1], &[OK, OK]), (Some(0), None, "filled"));
}

#[test]
fn a_stopping_train_votes_for_the_corridor_that_passes_its_intermediate_stop() {
    // a direct slow line and a high-speed detour 3.3 km north of it; the train stops at C on the detour
    let g = net(
        "svc",
        &[
            way("way/1", r#","maxspeed":"60""#, &[[100.0, 30.0], [100.1, 30.0], [100.2, 30.0]]),
            way("way/2", r#","maxspeed":"300","highspeed":"yes""#, &[[100.0, 30.0], [100.0, 30.03], [100.1, 30.03], [100.2, 30.03], [100.2, 30.0]]),
        ],
    );
    let (a, b, c) = ((100.0, 30.0), (100.2, 30.0), (100.1, 30.03));
    let cands = vec![candidates_for_test(&g, a, b, 1 << class_index("G"), None)];
    assert!(cands[0].len() >= 2);
    let shapes: Vec<Vec<Shape>> = vec![cands[0].iter().map(|k| Shape::of(&g, k)).collect()];
    let (key, _) = pair_key("A", a, "B", b);
    let keys = [(key, 0usize)].into_iter().collect();
    let stop = |n: &str, p: (f64, f64), t: i32| RunStop { name: n.to_string(), pt: Some(p), times: Some((t, t)) };
    let runs = [
        TrainRun {
            code: "G1".into(),
            class: "G".into(),
            stops: vec![stop("A", a, 600), stop("C", c, 620), stop("B", b, 640)],
            legs: vec![(0, 1, 0), (1, 2, 0)],
        },
        // a Z train through the same stops votes in its own routing group
        TrainRun { code: "Z1".into(), class: "Z".into(), stops: vec![stop("A", a, 700), stop("C", c, 740), stop("B", b, 780)], legs: vec![] },
    ];
    let svc = service_counts(&runs, &keys, &cands, &shapes);
    let fast = cands[0].iter().position(|k| k.source == "fast-G").unwrap();
    let short = cands[0].iter().position(|k| k.source == "shortest").unwrap();
    let group_g = super::model::class_group(class_index("G"));
    let group_z = super::model::class_group(class_index("Z"));
    assert_eq!((svc[0].1[group_g], svc[0].1[group_z]), (1, 1), "one span for each group");
    assert_eq!((svc[0].0[fast][group_g], svc[0].0[short][group_g]), (1, 0), "only the detour passes C");
}
