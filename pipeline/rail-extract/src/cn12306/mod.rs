//! `rail-extract cn12306`: route every consecutive stop pair of the China 12306 timetable over the world cut's China track (OSM
//! `railway=rail` ways), so a station can list the cities its trains go to and the paths they use. Offline and static: the timetable
//! and the station table are tracked inputs in pipeline/config/cn-12306/.
//!
//!   graph      graph.rs     every rail way of <data>/cn/features.geojsonl, ways joined within 2 m, a speed per way
//!   stations   stations.rs  timetable + station table -> the unique stop pairs with points and the fastest scheduled time
//!   route      route.rs     snap both stations onto the track, A* with alternatives, pick by minimum run time, bridge gaps of 10 km or less
//!   parallel   parallel.rs  ways within 70 m and 20 degrees of a path count for the leg too
//!   stretch    stretch.rs   maximal runs of ways with the same legs, with a stable id
//!
//! Writes <out>/pairs.json, stretches.json and report.json. Nothing in them depends on thread count, wall time or input order, so
//! two runs give identical files.

mod graph;
mod parallel;
mod route;
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

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn r6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

fn pct(sorted: &[f64], q: f64) -> f64 {
    sorted.get(((sorted.len() as f64 * q) as usize).min(sorted.len().saturating_sub(1))).copied().unwrap_or(0.0)
}

pub fn run(features: &Path, input: &Path, out: &Path) {
    let t0 = Instant::now();
    let table = stations::load_table(&input.join("stations.tsv"));
    let trains = stations::load_timetable(&input.join("timetable.json"));
    let (pairs, counts) = stations::pairs(&trains, &table);
    eprintln!(
        "[{:6.2}s] timetable: {} trains, {} stops ({} without a place), {} unique stop pairs",
        t0.elapsed().as_secs_f64(),
        counts.trains,
        counts.stops,
        counts.unplaced_stops,
        pairs.len()
    );

    let g = graph::load(features);
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

    // longest pairs first, so the last jobs are short; the results go back to their place in `pairs`
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
    let (pairs, routed): (Vec<_>, Vec<_>) = pairs
        .into_iter()
        .zip(routed)
        .filter(|(p, r)| {
            let drop = p.joined && r.pick.is_none();
            if drop {
                dropped.push(format!("{}|{}", p.a, p.b));
            }
            !drop
        })
        .unzip();
    eprintln!("[{:6.2}s] routed", t0.elapsed().as_secs_f64());

    // legs: the pairs with a path
    let legs: Vec<usize> = (0..pairs.len()).filter(|&i| routed[i].pick.is_some()).collect();
    let beside: Vec<Vec<u32>> = legs
        .par_iter()
        .map_init(
            || parallel::ParScratch::new(&g),
            |sc, &i| {
                let c = routed[i].pick.as_ref().unwrap();
                parallel::parallel_ways(&g, sc, &c.geom, &c.jl, c.km)
            },
        )
        .collect();
    let paths: Vec<Vec<u32>> = legs.iter().map(|&i| routed[i].pick.as_ref().unwrap().wk.iter().map(|w| w.0).collect()).collect();
    let way_ids: Vec<&str> = g.ways.iter().map(|w| w.id.as_str()).collect();
    let mut st = stretch::stretches(&paths, &way_ids);
    stretch::attach_parallel(&mut st, &paths, &beside, |w| g.ways[w as usize].mid);
    eprintln!("[{:6.2}s] {} stretches", t0.elapsed().as_secs_f64(), st.len());

    std::fs::create_dir_all(out).unwrap_or_else(|e| crate::fail(format!("create {}: {e}", out.display())));
    let ids = |ws: &[u32]| -> Vec<&str> { ws.iter().map(|&w| way_ids[w as usize]).collect() };
    let leg_pos: BTreeMap<usize, usize> = legs.iter().enumerate().map(|(l, &i)| (i, l)).collect();
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
            m.insert("ways".into(), json!(ids(&paths[leg_pos[&i]])));
            m.insert("parallel".into(), json!(ids(&beside[leg_pos[&i]])));
        }
        text.push_str(&o.to_string());
        text.push_str(if i + 1 < pairs.len() { ",\n" } else { "\n" });
    }
    text.push_str("]\n");
    write(out, "pairs.json", &text);

    let mut text = String::from("[\n");
    for (i, s) in st.iter().enumerate() {
        let km: f64 = s.ways.iter().map(|&w| g.ways[w as usize].km).sum();
        let o = json!({ "id": format!("{:016x}", s.id), "km": r3(km), "ways": ids(&s.ways), "parallel": ids(&s.parallel), "legs": s.legs.iter().map(|&l| legs[l as usize]).collect::<Vec<_>>() });
        text.push_str(&o.to_string());
        text.push_str(if i + 1 < st.len() { ",\n" } else { "\n" });
    }
    text.push_str("]\n");
    write(out, "stretches.json", &text);

    // report
    let count = |f: &dyn Fn(&Routed) -> bool| routed.iter().filter(|r| f(r)).count();
    let mut flags: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &routed {
        *flags.entry(if r.flag.is_empty() { "none" } else { r.flag }).or_default() += 1;
    }
    let mut kms: Vec<f64> = st.iter().map(|s| s.ways.iter().map(|&w| g.ways[w as usize].km).sum()).collect();
    kms.sort_by(|a, b| a.total_cmp(b));
    let own: f64 = kms.iter().sum();
    let par: f64 = st.iter().flat_map(|s| &s.parallel).map(|&w| g.ways[w as usize].km).sum();
    let mut classes: BTreeMap<&str, usize> = BTreeMap::new();
    for t in &trains {
        *classes.entry(t.class.as_str()).or_default() += 1;
    }
    let report = json!({
        "timetable": { "trains": counts.trains, "classes": classes, "stops": counts.stops, "stops_without_place": counts.unplaced_stops, "pairs_over_unplaced_stops": counts.spanning_pairs, "pairs": pairs.len(), "dropped_joined_pairs": dropped },
        "graph": {
            "ways": g.ways.len(), "service_ways": g.ways.iter().filter(|w| w.service).count(), "nodes": g.n_nodes(), "edges": g.segs.len(), "components": n_comp,
        },
        "routing": {
            "ok": count(&|r| r.status == Status::Ok), "bridged": count(&|r| r.status == Status::Bridged), "chord": count(&|r| r.status == Status::Chord),
            "flags": flags, "bridge_searches_for_a_hole": count(&|r| r.searched == 1), "bridge_searches_for_a_detour": count(&|r| r.searched == 2),
        },
        "stretches": {
            "count": st.len(), "median_km": r3(pct(&kms, 0.5)), "p95_km": r3(pct(&kms, 0.95)), "max_km": r3(kms.last().copied().unwrap_or(0.0)),
            "path_ways": st.iter().map(|s| s.ways.len()).sum::<usize>(), "parallel_ways": st.iter().map(|s| s.parallel.len()).sum::<usize>(),
            "unique_km": r3(own), "parallel_km": r3(par),
        },
    });
    write(out, "report.json", &(serde_json::to_string_pretty(&report).unwrap() + "\n"));
    eprintln!("[{:6.2}s] done -> {}", t0.elapsed().as_secs_f64(), out.display());
}

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap_or_else(|e| crate::fail(format!("write {}: {e}", dir.join(name).display())));
}
