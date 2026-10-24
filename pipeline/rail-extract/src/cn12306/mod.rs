//! `rail-extract cn12306`: route every train of the China 12306 timetable over the world cut's China track (OSM `railway=rail` ways), so
//! a station can list the cities its trains go to and the paths they use. Offline and static: the timetable and the station table are
//! tracked inputs in pipeline/config/cn-12306/.
//!
//!   graph      graph.rs     every rail way of <data>/cn/features.geojsonl, ways joined only where they share an OSM node id (way_nodes.jsonl), a speed per way
//!   stations   stations.rs  timetable + station table -> the unique stop pairs with points and the fastest scheduled time
//!   route      route.rs     stage 1, one path per stop pair: snap both stations onto the track, A* with alternatives, pick by minimum
//!                           run time, bridge gaps of 10 km or less
//!   corridor   corridor.rs  stage 2, one path per train leg, after the corridor-v3 router of the user's china-rail project: candidates,
//!                           a learned run-time model (model.rs), demonstrated corridors, a per-train posterior, a switch away from the
//!                           stage-1 path only on decisive evidence, double track redrawn as one line, smallest-gap bridging
//!                           (graph and searches in rgraph.rs, the leave-one-out self-check in selfcheck.rs)
//!   stretch    stretch.rs   maximal runs of ways with the same set of trains, with a stable id
//!
//! Writes <out>/pairs.json, stretches.json, legs.jsonl, wrong_switches.jsonl and report.json. Nothing in them depends on thread count,
//! wall time or input order, so two runs give identical files.

mod corridor;
#[cfg(test)]
mod corridor_tests;
mod draw;
mod graph;
mod model;
pub(crate) mod records;
mod rgraph;
mod route;
mod selfcheck;
mod sidefiles;
mod stations;
mod stretch;
#[cfg(test)]
mod tests;

use rayon::prelude::*;
use route::{Routed, Scratch, Status};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

/// the failures of the stage (a missing input, a bad file) are values, not panics: the release profile aborts on a panic, and the build
/// must be able to skip the stage with a warning
pub(super) type R<T> = Result<T, String>;

/// an I/O result with the action and the path in its message
pub(super) fn io<T>(r: std::io::Result<T>, what: &str, p: &Path) -> R<T> {
    r.map_err(|e| format!("{what} {}: {e}", p.display()))
}

/// takes the services out of a country's side files (a skipped stage must not leave an old revision published)
pub fn remove_side_files(dir: &Path) {
    sidefiles::remove(dir);
}

/// `run`, with a failure returned as its message
pub fn run_checked(features: &Path, input: &Path, out: Option<&Path>, side: Option<Side>) -> Result<(), String> {
    run(features, input, out, side)
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn r6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

fn pct(sorted: &[f64], q: f64) -> f64 {
    sorted.get(((sorted.len() as f64 * q) as usize).min(sorted.len().saturating_sub(1))).copied().unwrap_or(0.0)
}

/// where the side files of the app come from and go to
pub struct Side<'a> {
    /// the cut and its station model: features.typed.geojsonl, stations.geojsonl, complexes.geojsonl
    pub cut: &'a Path,
    /// data/regions: the outlines of the cities
    pub regions: &'a Path,
    /// <out>/<slug>: shards.json is there, and the cn12306 folder goes in
    pub dir: &'a Path,
}

fn run(features: &Path, input: &Path, out: Option<&Path>, side: Option<Side>) -> R<()> {
    let t0 = Instant::now();
    let table = stations::load_table(&input.join("stations.tsv"))?;
    let trains = stations::load_timetable(&input.join("timetable.json"))?;
    let (pairs, runs, counts) = stations::pairs(&trains, &table);
    eprintln!(
        "[{:6.2}s] timetable: {} trains, {} stops ({} without a place), {} unique stop pairs",
        t0.elapsed().as_secs_f64(),
        counts.trains,
        counts.stops,
        counts.unplaced_stops,
        pairs.len()
    );

    let g = graph::load(features)?;
    let n_comp = g.comp.iter().copied().max().map_or(0, |m| m + 1);
    eprintln!(
        "[{:6.2}s] graph: {} ways ({} service), {} nodes, {} edges, {} components",
        t0.elapsed().as_secs_f64(),
        g.ways.len(),
        g.ways.iter().filter(|w| w.service).count(),
        g.n_nodes(),
        g.segs.len(),
        n_comp
    );

    // stage 1: longest pairs first, so the last jobs are short; the results go back to their place in `pairs`
    let mut order: Vec<usize> = (0..pairs.len()).collect();
    order.sort_by(|&i, &j| {
        let d = |p: &stations::PairIn| graph::hav(p.pa.0, p.pa.1, p.pb.0, p.pb.1);
        d(&pairs[j]).total_cmp(&d(&pairs[i])).then(i.cmp(&j))
    });
    let done: Vec<(usize, Routed)> =
        order.par_iter().map_init(|| Scratch::new(&g), |sc, &i| (i, route::route(&g, sc, pairs[i].pa, pairs[i].pb, pairs[i].fs))).collect();
    let mut routed: Vec<Option<Routed>> = (0..pairs.len()).map(|_| None).collect();
    for (i, r) in done {
        routed[i] = Some(r);
    }
    let routed: Vec<Routed> = routed.into_iter().map(|r| r.unwrap()).collect();
    // a pair joined over an unplaced stop that has no path is dropped: its ends are not known to be a hop apart
    let mut dropped: Vec<String> = Vec::new();
    let mut remap: Vec<Option<usize>> = Vec::with_capacity(pairs.len());
    let (pairs, routed): (Vec<_>, Vec<_>) = pairs
        .into_iter()
        .zip(routed)
        .filter(|(p, r)| {
            let drop = p.joined && r.pick.is_none();
            if drop {
                dropped.push(format!("{}|{}", p.a, p.b));
            }
            remap.push((!drop).then(|| remap.iter().flatten().count()));
            !drop
        })
        .unzip();
    eprintln!("[{:6.2}s] routed (stage 1)", t0.elapsed().as_secs_f64());

    // stage 2: the corridor of every train leg
    let mut rg = rgraph::build(&g);
    let flagged = rg.mark_prod(routed.iter().filter_map(|r| r.pick.as_ref().map(|c| c.nodes.as_slice())));
    eprintln!("[{:6.2}s] router graph: {} edges of stage-1 paths preferred on double track", t0.elapsed().as_secs_f64(), flagged);
    let way_ids: Vec<&str> = g.ways.iter().map(|w| w.id.as_str()).collect();
    let cv = corridor::run(&corridor::Inputs { g: &rg, pairs: &pairs, routed: &routed, runs: &runs, remap: &remap, way_ids: &way_ids }, &t0);
    drop(rg);

    // the paths in train order, so the legs of one train follow each other
    let (mut paths, mut owner): (Vec<Vec<u32>>, Vec<u32>) = (Vec::new(), Vec::new());
    for (ti, legs) in cv.paths.iter().enumerate() {
        for p in legs.iter().flatten().filter(|p| !p.is_empty()) {
            paths.push(p.clone());
            owner.push(ti as u32);
        }
    }
    let st = stretch::stretches(&paths, &owner, &way_ids)?;
    eprintln!("[{:6.2}s] {} stretches over {} leg paths", t0.elapsed().as_secs_f64(), st.len(), paths.len());

    let ids = |ws: &[u32]| -> Vec<&str> { ws.iter().map(|&w| way_ids[w as usize]).collect() };
    if let Some(out) = out {
        io(std::fs::create_dir_all(out), "create", out)?;
        let mut text = String::from("[\n");
        for (i, (p, r)) in pairs.iter().zip(&routed).enumerate() {
            let mut o = json!({ "a": p.a, "b": p.b, "status": r.status.name(), "flag": r.flag, "pa": [r6(p.pa.0), r6(p.pa.1)], "pb": [r6(p.pb.0), r6(p.pb.1)], "chord_km": r3(r.chord_km), "fs": p.fs });
            if let Some(c) = &r.pick {
                let m = o.as_object_mut().unwrap();
                m.insert("km".into(), json!(r3(c.km)));
                m.insert("min_time".into(), json!(r3(c.min_time)));
                m.insert("slack".into(), json!(r.slack.map(r3)));
                m.insert("src".into(), json!(c.src));
                m.insert("cands".into(), json!(r.cands));
                m.insert("gap_km".into(), json!(r3(c.gap_km)));
                m.insert("gaps".into(), json!(c.jl.len()));
                m.insert("ways".into(), json!(c.wk.iter().map(|w| way_ids[w.0 as usize]).collect::<Vec<_>>()));
            }
            text.push_str(&o.to_string());
            text.push_str(if i + 1 < pairs.len() { ",\n" } else { "\n" });
        }
        text.push_str("]\n");
        write(out, "pairs.json", &text)?;

        let mut text = String::from("[\n");
        for (i, s) in st.iter().enumerate() {
            let km: f64 = s.ways.iter().map(|&w| g.ways[w as usize].km).sum();
            let o = json!({ "id": format!("{:016x}", s.id), "km": r3(km), "ways": ids(&s.ways), "trains": s.trains });
            text.push_str(&o.to_string());
            text.push_str(if i + 1 < st.len() { ",\n" } else { "\n" });
        }
        text.push_str("]\n");
        write(out, "stretches.json", &text)?;
        write(out, "legs.jsonl", &cv.legs_text)?;
        write(out, "wrong_switches.jsonl", &cv.wrong_switches)?;
    }
    // report
    let count = |f: &dyn Fn(&Routed) -> bool| routed.iter().filter(|r| f(r)).count();
    let mut flags: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &routed {
        *flags.entry(if r.flag.is_empty() { "none" } else { r.flag }).or_default() += 1;
    }
    let mut kms: Vec<f64> = st.iter().map(|s| s.ways.iter().map(|&w| g.ways[w as usize].km).sum()).collect();
    kms.sort_by(|a, b| a.total_cmp(b));
    let own: f64 = kms.iter().sum();
    let mut classes: BTreeMap<&str, usize> = BTreeMap::new();
    for t in &trains {
        *classes.entry(t.class.as_str()).or_default() += 1;
    }
    let report = json!({
        "timetable": { "trains": counts.trains, "classes": classes, "stops": counts.stops, "stops_without_place": counts.unplaced_stops, "pairs_over_unplaced_stops": counts.spanning_pairs, "pairs": pairs.len(), "dropped_joined_pairs": dropped },
        "graph": {
            "ways_without_node_ids": g.ways_without_node_ids, "dup_node_joins": g.dup_joins.iter().map(|(a, b, p)| json!({ "ways": [a, b], "point": [p.0, p.1] })).collect::<Vec<_>>(), "ways": g.ways.len(), "service_ways": g.ways.iter().filter(|w| w.service).count(), "nodes": g.n_nodes(), "edges": g.segs.len(), "components": n_comp,
        },
        "routing": {
            "ok": count(&|r| r.status == Status::Ok), "bridged": count(&|r| r.status == Status::Bridged), "chord": count(&|r| r.status == Status::Chord),
            "flags": flags, "bridge_searches_for_a_hole": count(&|r| r.searched == 1), "bridge_searches_for_a_detour": count(&|r| r.searched == 2),
        },
        "corridor": cv.report,
        "stretches": {
            "count": st.len(), "median_km": r3(pct(&kms, 0.5)), "p95_km": r3(pct(&kms, 0.95)), "max_km": r3(kms.last().copied().unwrap_or(0.0)),
            "path_ways": st.iter().map(|s| s.ways.len()).sum::<usize>(), "unique_km": r3(own),
        },
    });
    if let Some(out) = out {
        write(out, "report.json", &(serde_json::to_string_pretty(&report).unwrap_or_default() + "\n"))?;
        eprintln!("[{:6.2}s] done -> {}", t0.elapsed().as_secs_f64(), out.display());
    }
    if let Some(side) = side {
        let english = stations::load_english(&input.join("stations.tsv"))?;
        let gazetteer = stations::load_gazetteer(input)?;
        let snapshot = stations::load_snapshot(input)?;
        let mut files = sidefiles::write(&side, &snapshot, &g, &st, &runs, &english, &gazetteer)?;
        // the report of the build: the records and the files, with the routing, the decisions and the self-check
        if let Some(m) = files.as_object_mut() {
            for k in ["timetable", "graph", "routing", "corridor", "stretches"] {
                m.insert(k.to_string(), report[k].clone());
            }
        }
        let rp = side.cut.join("cn12306_report.json");
        io(std::fs::write(&rp, serde_json::to_string_pretty(&files).unwrap_or_default() + "\n"), "write", &rp)?;
        eprintln!("[{:6.2}s] side files -> {}", t0.elapsed().as_secs_f64(), side.dir.join("cn12306").display());
    }
    Ok(())
}

fn write(dir: &Path, name: &str, text: &str) -> R<()> {
    io(std::fs::write(dir.join(name), text), "write", &dir.join(name))
}
