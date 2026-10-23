//! The corridor router, ported from the user's corridor-v3 project (`corridor-router`, 2026-10-22): which corridor each train takes
//! between two stops, decided per train leg instead of once per stop pair.
//!
//! ```text
//! 1 candidates   per pair: the fastest path per class group, the shortest, up to three penalty-diversified alternatives, the stage-1
//!                path itself. "The same corridor" is geometric (within 150 m), so double track is one corridor
//! 2 calibration  how the scheduled minutes relate to the modelled run time (maxspeed, class cap, braking curve), learned per class from
//!                the legs whose corridor is not in doubt
//! 3 bridging     a pair whose stage-1 path crosses a gap, or that no candidate fits, gets the smallest plausible gap, started at a
//!                dangling track end
//! 4 services     the corridor that stopping trains of the same class group demonstrate between the same two stations
//! 5 posterior    per train, the legs are linked by a reversal penalty (a short dwell and a heading flip) and forward-backward gives
//!                each leg's probability per candidate
//! 6 decision     a leg leaves the stage-1 path only on decisive evidence (posterior >= 0.9 against <= 0.05 for the stage-1 path); a
//!                pair drawn as a chord gets a track path when one corridor is >= 0.8 likely
//! 7 redraw       a chosen corridor, and every stage-1 path, is redrawn within 200 m of itself on the track that carries the most
//!                legs, hopping (30 m) between parallel tracks of one named line, so double track is one line
//! ```
//!
//! The model is in model.rs, the graph and searches in rgraph.rs, the leave-one-out self-check in selfcheck.rs. Differences from the
//! original: the graph is the stage-1 graph of this build (cut ways, so every path is made of OSM way ids), the stations are the points
//! of the station table, the parameters are constants (no environment variables), and every iteration whose order could change a
//! result is ordered, so two runs give identical files on any thread count.

use super::model::{self, class_group, class_index, Calibration, Cand, LegObs, Shape, CLASS_NAMES, NCLASS};
use super::rgraph::{self, hav, Buffers, FineIndex, Metric, Penalty, RGraph, Snap, SnapIndex, P};
use super::route::{Routed, Status};
use super::selfcheck;
use super::stations::{self, PairIn, TrainRun};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Instant;

/// a candidate is no longer than this many times the straight line (plus 30 km) nor 1.6 times the shortest (plus 30 km)
const MAX_LEN_CHORD: f64 = 2.0;
const MAX_LEN_SHORTEST: f64 = 1.6;
/// a path is a new corridor when less than this share of it lies within 150 m of every corridor so far
const DISTINCT_OVERLAP: f64 = 0.8;
/// the stage-1 path is the same corridor as a candidate when both cover each other this much
const BASE_SAME_OVERLAP: f64 = 0.9;
const PENALTY_ROUNDS: usize = 3;
const PENALTY_MULT: f64 = 4.0;
/// the corridor a redraw stays within, km
const REDRAW_BUFFER_KM: f64 = 0.2;
/// rounds of the canonical redraw: each weights the tracks by the usage of the round before, so the busier of two parallel tracks wins
pub const REDRAW_ROUNDS: usize = 4;
/// legs whose feasible candidates predict within this ratio of each other have an undisputed corridor
const UNDISPUTED_SPREAD: f64 = 1.04;
/// bridging limits (as stage 1) and the gain in gap that replaces a stage-1 bridge
const BRIDGE_MAX_GAP_KM: f64 = 10.0;
const BRIDGE_RATIO: (f64, f64) = (0.95, 1.45);
const BRIDGE_MAX_KMH: f64 = 355.0;
const BRIDGE_GAP_GAIN_KM: f64 = 0.5;
/// a stop lies on a corridor when within this distance of its path, km; a service spans up to this many intermediate stops
pub const SVC_STOP_KM: f64 = 1.0;
const SVC_MAX_INTERMEDIATE: usize = 6;
/// with no dangling end to bridge from, a bridge may start at any node reached within this track distance of the station, km
const BRIDGE_FALLBACK_REACH_KM: f64 = 4.0;
/// a chord pair is also drawn on track when its gap is under 1 km and the posterior is at least this
const FILL_SMALL_GAP_POST: f64 = 0.35;

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// a train's leg between two placed stops
pub struct Leg {
    pub train: usize,
    /// index into the train's `legs`
    pub k: usize,
    pub pair: usize,
    /// the train runs the pair from its first name to its second
    pub forward: bool,
    pub class: usize,
    pub minutes: Option<f64>,
    /// minutes at the stop where the leg ends
    pub dwell_end: i64,
}

pub struct PairInfo {
    pub pa: P,
    pub pb: P,
    pub chord: f64,
    pub bridged: bool,
    pub class_mask: u32,
    pub dominant: usize,
    pub min_minutes: Option<f64>,
}

/// the candidate corridors of one pair
fn gen_candidates(g: &RGraph, fine: &FineIndex, buf: &mut Buffers, p: &PairInfo, sa: &Snap, sb: &Snap, base: Option<&[u32]>) -> Vec<Cand> {
    let mut cands: Vec<Cand> = Vec::new();
    let mut sets: Vec<Shape> = Vec::new();
    let no_pen = Penalty::default();
    let dist_cap = (MAX_LEN_CHORD * p.chord + 40.0).max(p.chord * 2.5 + 5.0);
    let time_cap = dist_cap / 40.0;
    let accept = |c: Cand, cands: &mut Vec<Cand>, sets: &mut Vec<Shape>| {
        if c.km > dist_cap {
            return;
        }
        let kmin = cands.iter().map(|k| k.km).fold(c.km, f64::min);
        if c.km > MAX_LEN_SHORTEST * kmin + 30.0 {
            return;
        }
        // distinct: a real share of the new path runs away from every corridor so far
        let sh = Shape::of(g, &c);
        if sets.iter().all(|s| sh.covered_by(s) < DISTINCT_OVERLAP) {
            cands.push(c);
            sets.push(sh);
        }
    };
    // the fastest per class group present on the pair, then the shortest
    let mut metrics: Vec<(String, Metric)> = Vec::new();
    let mut seen_caps: FxHashSet<(u64, u64)> = FxHashSet::default();
    for (c, name) in CLASS_NAMES.iter().enumerate() {
        if p.class_mask & (1 << c) == 0 {
            continue;
        }
        let m = model::class_metric(c);
        if let Metric::Time { cap, hsr_mult } = m {
            if seen_caps.insert((cap.to_bits(), hsr_mult.to_bits())) {
                metrics.push((format!("fast-{name}"), m));
            }
        }
    }
    metrics.push(("shortest".into(), Metric::Distance));
    for (name, m) in &metrics {
        let cap = if matches!(m, Metric::Distance) { dist_cap } else { time_cap };
        if let Some((_, path)) = rgraph::route(g, buf, sa, sb, p.pb, *m, &no_pen, cap) {
            accept(Cand::measure(g, path, name), &mut cands, &mut sets);
        }
    }
    // penalty-diversified alternatives under the dominant class's metric
    let dm = model::class_metric(p.dominant);
    let mut pen = Penalty::default();
    for r in 0..PENALTY_ROUNDS {
        if cands.is_empty() {
            break;
        }
        // every edge within 150 m of a corridor found costs more (both tracks of a double line, station loops), not just its own
        for s in &sets {
            for &q in s.samples() {
                for u in fine.near(g, q, 0.15) {
                    for e in g.edges(u) {
                        pen.insert((u.min(e.to), u.max(e.to)), PENALTY_MULT);
                    }
                }
            }
        }
        let before = cands.len();
        if let Some((_, path)) = rgraph::route(g, buf, sa, sb, p.pb, dm, &pen, time_cap * 2.0) {
            accept(Cand::measure(g, path, &format!("alt-{}", r + 1)), &mut cands, &mut sets);
        }
        if cands.len() == before {
            break;
        }
    }
    // the stage-1 path
    if let Some(nodes) = base {
        let mut nodes: Vec<u32> = nodes.to_vec();
        nodes.dedup();
        if nodes.len() >= 2 {
            let mut bc = Cand::measure(g, nodes, "production");
            bc.is_base = true;
            let sh = Shape::of(g, &bc);
            let same = sets.iter().position(|s| bc.gap_km == 0.0 && sh.covered_by(s) >= BASE_SAME_OVERLAP && s.covered_by(&sh) >= BASE_SAME_OVERLAP);
            match same {
                Some(i) => {
                    // the stage-1 geometry stands for that corridor
                    cands[i] = bc;
                    sets[i] = sh;
                }
                None => {
                    cands.push(bc);
                    sets.push(sh);
                }
            }
        }
    }
    cands
}

/// The smallest-gap bridge between the two stations' track: the connection starts at a dangling track end on one side, and the bridged
/// length must pass the stage-1 audits. Among plausible connections the smallest gap wins (ties: the shorter total).
fn min_gap_bridge(g: &RGraph, p: &PairInfo, sa: &Snap, sb: &Snap) -> Option<Cand> {
    let radius = (p.chord * 1.2 + 15.0).min(130.0);
    let ta = rgraph::reach(g, sa, radius);
    let tb = rgraph::reach(g, sb, radius);
    let (na, nb) = (rgraph::reached(&ta), rgraph::reached(&tb));
    let grid_of = |nodes: &[u32]| {
        let mut grid: FxHashMap<(i32, i32), Vec<u32>> = FxHashMap::default();
        for &v in nodes {
            let c = g.xy[v as usize];
            grid.entry(((c.0 * 20.0) as i32, (c.1 * 20.0) as i32)).or_default().push(v);
        }
        grid
    };
    let (ga, gb) = (grid_of(&na), grid_of(&nb));
    let speed_ok = |total: f64| match p.min_minutes {
        Some(m) if m > 0.0 => total / (m / 60.0) <= BRIDGE_MAX_KMH,
        _ => true,
    };
    let mut best: Option<(f64, f64, u32, u32)> = None; // (gap, total, a-node, b-node)
                                                       // from a dangling end; when no connection starts there, from any node reached within 4 km of its station
    let scan = |nodes: &[u32],
                from: &FxHashMap<u32, (f64, u32)>,
                other: &FxHashMap<u32, (f64, u32)>,
                grid: &FxHashMap<(i32, i32), Vec<u32>>,
                a_side: bool,
                dangling_only: bool,
                best: &mut Option<(f64, f64, u32, u32)>| {
        for &u in nodes {
            let Some(&(du, _)) = from.get(&u) else { continue };
            if dangling_only && g.degree[u as usize] != 1 {
                continue;
            }
            if !dangling_only && du > BRIDGE_FALLBACK_REACH_KM {
                continue;
            }
            let cu = g.xy[u as usize];
            let (cx, cy) = ((cu.0 * 20.0) as i32, (cu.1 * 20.0) as i32);
            for dx in -2..=2 {
                for dy in -2..=2 {
                    for &v in grid.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                        let gap = hav(cu, g.xy[v as usize]);
                        if gap > BRIDGE_MAX_GAP_KM || gap < 0.001 {
                            continue;
                        }
                        let Some(&(dv, _)) = other.get(&v) else { continue };
                        let total = du + gap + dv;
                        let ratio = total / p.chord;
                        if ratio < BRIDGE_RATIO.0 || ratio > BRIDGE_RATIO.1 || !speed_ok(total) {
                            continue;
                        }
                        let better = match *best {
                            None => true,
                            Some((bg, bt, _, _)) => gap < bg - 0.05 || ((gap - bg).abs() <= 0.05 && total < bt),
                        };
                        if better {
                            *best = Some(if a_side { (gap, total, u, v) } else { (gap, total, v, u) });
                        }
                    }
                }
            }
        }
    };
    scan(&na, &ta, &tb, &gb, true, true, &mut best);
    scan(&nb, &tb, &ta, &ga, false, true, &mut best);
    if best.is_none() {
        scan(&na, &ta, &tb, &gb, true, false, &mut best);
        scan(&nb, &tb, &ta, &ga, false, false, &mut best);
    }
    let (_, _, u, v) = best?;
    let mut nodes = rgraph::reach_path(&ta, u);
    let mut tail = rgraph::reach_path(&tb, v);
    tail.reverse();
    nodes.extend(tail);
    Some(Cand::measure(g, nodes, "min-gap-bridge"))
}

/// the smallest distance (km) from a point to a candidate's nodes
pub fn dist_to_path(g: &RGraph, k: &Cand, pt: P) -> f64 {
    k.nodes.iter().map(|&n| hav(pt, g.xy[n as usize])).fold(f64::INFINITY, f64::min)
}

type Usage = FxHashMap<(u32, u32), u32>;

/// Redraws a corridor for the map: between the same end nodes (per piece, for a bridged path), stay within 200 m of the corridor and
/// follow the track that carries the most legs, so a re-routed train joins the line already drawn instead of making a parallel one on
/// the other track of a double line.
fn redraw(g: &RGraph, fine: &FineIndex, buf: &mut Buffers, k: &Cand, usage: &Usage) -> Cand {
    let shape = Shape::of(g, k);
    let mut allowed: FxHashSet<(u32, u32)> = FxHashSet::default();
    for &p in shape.samples() {
        for u in fine.near(g, p, REDRAW_BUFFER_KM) {
            for e in g.edges(u) {
                allowed.insert((u.min(e.to), u.max(e.to)));
            }
        }
    }
    // every edge of the path itself too (the samples are 0.3 km apart)
    for w in k.nodes.windows(2) {
        allowed.insert((w[0].min(w[1]), w[0].max(w[1])));
    }
    let cost = |u: u32, e: &rgraph::Edge| -> f64 {
        let key = (u.min(e.to), u.max(e.to));
        let base = e.km as f64;
        if !allowed.contains(&key) {
            return base * 50.0;
        }
        match usage.get(&key) {
            Some(&n) => base / (1.0 + n as f64).sqrt(),
            None => base,
        }
    };
    let mut cuts: Vec<usize> = vec![0];
    for &i in &k.gaps_at {
        cuts.push(i);
        cuts.push(i + 1);
    }
    cuts.push(k.nodes.len() - 1);
    let mut nodes: Vec<u32> = Vec::new();
    for seg in cuts.chunks(2) {
        let (a, b) = (k.nodes[seg[0]], k.nodes[seg[1]]);
        let part = if a == b {
            vec![a]
        } else {
            rgraph::route_custom(g, buf, a, b, cost, Some(fine), k.km * 60.0).unwrap_or_else(|| k.nodes[seg[0]..=seg[1]].to_vec())
        };
        nodes.extend(part);
    }
    let mut out = Cand::measure(g, nodes, &k.source);
    out.is_base = k.is_base;
    out
}

/// train-legs per undirected edge over weighted paths
fn edge_usage<'a>(paths: impl Iterator<Item = (&'a Cand, u32)>) -> Usage {
    let mut usage = Usage::default();
    for (k, n) in paths {
        for w in k.nodes.windows(2) {
            *usage.entry((w[0].min(w[1]), w[0].max(w[1]))).or_default() += n;
        }
    }
    usage
}

/// per pair: the count of compatible stopping services per candidate and the number of services, each by routing group
pub type SvcCounts = (Vec<[u32; model::NGROUP]>, [u32; model::NGROUP]);

/// For every train and every span of 2 to 7 consecutive stops whose ends are a pair, the candidates all of whose intermediate stops lie
/// on it: the corridor a stopping service demonstrates between the same two stations.
pub(super) fn service_counts(runs: &[TrainRun], keys: &FxHashMap<stations::Key, usize>, cands: &[Vec<Cand>], shapes: &[Vec<Shape>]) -> Vec<SvcCounts> {
    let spans: Vec<(usize, usize, Vec<bool>)> = runs
        .par_iter()
        .flat_map_iter(|t| {
            let gidx = class_group(class_index(&t.class));
            let mut out = Vec::new();
            for i in 0..t.stops.len() {
                let Some(pi_) = t.stops[i].pt else { continue };
                let hi = (i + SVC_MAX_INTERMEDIATE + 2).min(t.stops.len());
                for j in i + 2..hi {
                    let Some(pj_) = t.stops[j].pt else { continue };
                    if t.stops[i].name == t.stops[j].name {
                        continue;
                    }
                    let (key, _) = stations::pair_key(&t.stops[i].name, pi_, &t.stops[j].name, pj_);
                    let Some(&pi) = keys.get(&key) else { continue };
                    if cands[pi].is_empty() {
                        continue;
                    }
                    let pts: Option<Vec<P>> = t.stops[i + 1..j].iter().map(|s| s.pt).collect();
                    let Some(pts) = pts else { continue };
                    let compat: Vec<bool> = shapes[pi].iter().map(|s| pts.iter().all(|q| s.near(*q, SVC_STOP_KM))).collect();
                    if compat.iter().any(|x| *x) {
                        out.push((pi, gidx, compat));
                    }
                }
            }
            out.into_iter()
        })
        .collect();
    let mut svc: Vec<SvcCounts> = cands.iter().map(|cs| (vec![[0u32; model::NGROUP]; cs.len()], [0u32; model::NGROUP])).collect();
    for (pi, gidx, compat) in spans {
        svc[pi].1[gidx] += 1;
        for (k, ok) in compat.iter().enumerate() {
            if *ok {
                svc[pi].0[k][gidx] += 1;
            }
        }
    }
    svc
}

struct Decision {
    leg: usize,
    chosen: Option<usize>,
    base: Option<usize>,
    post: Vec<f64>,
    decision: &'static str,
}

/// What to draw for one leg: (the candidate chosen, the stage-1 candidate, the decision). `post` is the posterior and `emit` the log
/// evidence of every candidate (a candidate is feasible above -29).
pub(super) fn decide(pc: &[Cand], post: &[f64], emit: &[f64]) -> (Option<usize>, Option<usize>, &'static str) {
    let params = model::params();
    let best = (0..post.len()).max_by(|&a, &b| post[a].total_cmp(&post[b]));
    let base = pc.iter().position(|c| c.is_base);
    let feasible = |i: usize| emit[i] > -29.0;
    let any_feasible = (0..pc.len()).any(feasible);
    // a bridged stage-1 path is replaced by a feasible candidate with a gap smaller by half a km or none at all: the smallest
    // gap first, then the higher posterior
    let bridge_fix = base.and_then(|bi| {
        let bg = pc[bi].gap_km;
        if bg <= 0.0 {
            return None;
        }
        (0..pc.len())
            .filter(|&i| feasible(i) && (pc[i].gap_km < bg - BRIDGE_GAP_GAIN_KM || pc[i].gap_km == 0.0))
            .min_by(|&a, &b| pc[a].gap_km.total_cmp(&pc[b].gap_km).then_with(|| post[b].total_cmp(&post[a])))
    });
    let (chosen, decision) = match (best, base) {
        (None, _) => (None, "no-candidates"),
        (Some(_), _) if !any_feasible => (base, "infeasible-keep"),
        (Some(b), Some(bi)) => {
            if b != bi && post[b] >= params.switch_post && post[bi] <= params.base_max {
                (Some(b), "switched")
            } else if let Some(fx) = bridge_fix {
                (Some(fx), "bridge-fix")
            } else if b != bi {
                (Some(bi), "ambiguous-keep")
            } else {
                (Some(bi), "production")
            }
        }
        (Some(b), None) => {
            if feasible(b) && (post[b] >= params.fill_post || (pc[b].gap_km < 1.0 && post[b] >= FILL_SMALL_GAP_POST)) {
                (Some(b), "filled")
            } else {
                (None, "chord")
            }
        }
    };
    (chosen, base, decision)
}

pub struct Out {
    /// per train, per leg of its `legs`: the ways of the leg's path in the order it runs them, None for a chord, a dropped pair or no
    /// candidate
    pub paths: Vec<Vec<Option<Vec<u32>>>>,
    /// one JSON object per leg: the decision and the path
    pub legs_text: String,
    /// the self-check's wrong switches, one JSON object per line
    pub wrong_switches: String,
    pub report: Value,
}

pub struct Inputs<'a> {
    pub g: &'a RGraph,
    pub pairs: &'a [PairIn],
    pub routed: &'a [Routed],
    pub runs: &'a [TrainRun],
    /// pair index of the timetable -> index in `pairs` (None: dropped)
    pub remap: &'a [Option<usize>],
    /// the OSM id of every way of the stage-1 graph
    pub way_ids: &'a [&'a str],
}

pub fn run(inp: &Inputs, t0: &Instant) -> Out {
    let (g, pairs_in, routed, runs) = (inp.g, inp.pairs, inp.routed, inp.runs);
    let mark = |what: String| eprintln!("[{:6.2}s]   corridor: {what}", t0.elapsed().as_secs_f64());
    let snapper = SnapIndex::new(g);
    let fine = FineIndex::new(g);
    mark(format!("graph: {} nodes, {} stitches", g.n_nodes(), g.n_stitches));

    // ---- legs ----
    let mut legs: Vec<Leg> = Vec::new();
    for (ti, t) in runs.iter().enumerate() {
        let c = class_index(&t.class);
        for (k, &(i, j, pair)) in t.legs.iter().enumerate() {
            let Some(pair) = inp.remap[pair] else { continue };
            let minutes = t.stops[i].times.zip(t.stops[j].times).map(|(a, b)| (b.0 - a.1) as f64).filter(|m| *m > 0.0);
            let dwell_end = t.stops[j].times.map_or(0, |(a, d)| (d - a).rem_euclid(1440) as i64);
            legs.push(Leg { train: ti, k, pair, forward: t.stops[i].name < t.stops[j].name, class: c, minutes, dwell_end });
        }
    }
    let mut pair_legs: Vec<Vec<usize>> = vec![Vec::new(); pairs_in.len()];
    for (i, l) in legs.iter().enumerate() {
        pair_legs[l.pair].push(i);
    }
    let pairs: Vec<PairInfo> = (0..pairs_in.len())
        .map(|pi| {
            let (pa, pb) = (pairs_in[pi].pa, pairs_in[pi].pb);
            let mut mask = 0u32;
            let mut count = [0usize; NCLASS];
            let mut min_m: Option<f64> = None;
            for &li in &pair_legs[pi] {
                mask |= 1 << legs[li].class;
                count[legs[li].class] += 1;
                if let Some(m) = legs[li].minutes {
                    min_m = Some(min_m.map_or(m, |x: f64| x.min(m)));
                }
            }
            let dominant = (0..NCLASS).max_by_key(|&c| count[c]).unwrap_or(0);
            PairInfo { pa, pb, chord: hav(pa, pb).max(0.5), bridged: routed[pi].status == Status::Bridged, class_mask: mask, dominant, min_minutes: min_m }
        })
        .collect();
    let keys: FxHashMap<stations::Key, usize> = pairs_in.iter().enumerate().map(|(i, p)| (stations::pair_key(&p.a, p.pa, &p.b, p.pb).0, i)).collect();
    mark(format!("legs: {} legs on {} pairs", legs.len(), pairs.len()));

    // ---- candidates (parallel) ----
    let snaps: Vec<(Snap, Snap)> = pairs.par_iter().map(|p| (snapper.snap(g, p.pa), snapper.snap(g, p.pb))).collect();
    let mut cands: Vec<Vec<Cand>> = pairs
        .par_iter()
        .zip(snaps.par_iter())
        .enumerate()
        .map_init(
            || Buffers::new(g.n_nodes()),
            |buf, (pi, (p, (sa, sb)))| {
                if sa.is_empty() || sb.is_empty() {
                    return Vec::new();
                }
                gen_candidates(g, &fine, buf, p, sa, sb, routed[pi].pick.as_ref().map(|c| c.nodes.as_slice()))
            },
        )
        .collect();
    mark(format!("candidates: {} over {} pairs", cands.iter().map(|c| c.len()).sum::<usize>(), pairs.len()));

    // ---- calibration on the undisputed legs ----
    let calib_samples: Vec<(usize, f64)> = legs
        .iter()
        .filter_map(|l| {
            let m = l.minutes?;
            let preds: Vec<f64> = cands[l.pair].iter().filter(|k| k.gap_km == 0.0).filter_map(|k| k.predict(l.class)).collect();
            if preds.is_empty() {
                return None;
            }
            let lo = preds.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = preds.iter().copied().fold(0.0, f64::max);
            (hi / lo <= UNDISPUTED_SPREAD).then(|| (l.class, (m / lo).ln()))
        })
        .collect();
    let cal = Calibration::fit(&calib_samples);
    mark(format!("calibration: {} undisputed legs", calib_samples.len()));

    // ---- smallest-gap bridging: the pairs stage 1 bridged, and the pairs where some leg fits no candidate ----
    let needs_bridge: Vec<usize> = (0..pairs.len())
        .filter(|&pi| {
            if snaps[pi].0.is_empty() || snaps[pi].1.is_empty() {
                return false;
            }
            if pairs[pi].bridged || cands[pi].is_empty() {
                return true;
            }
            pair_legs[pi].iter().any(|&li| {
                let l = &legs[li];
                l.minutes.is_some_and(|m| cands[pi].iter().all(|k| k.predict(l.class).is_none_or(|p| !cal.is_feasible(l.class, (m / p).ln()))))
            })
        })
        .collect();
    let bridges: Vec<(usize, Option<Cand>)> = needs_bridge.par_iter().map(|&pi| (pi, min_gap_bridge(g, &pairs[pi], &snaps[pi].0, &snaps[pi].1))).collect();
    let mut n_bridged = 0;
    for (pi, b) in bridges {
        if let Some(b) = b {
            n_bridged += 1;
            cands[pi].push(b);
        }
    }
    mark(format!("bridging: {} pairs searched, {} bridged", needs_bridge.len(), n_bridged));

    // ---- demonstrated corridors ----
    let shapes: Vec<Vec<Shape>> = cands.par_iter().map(|cs| cs.iter().map(|k| Shape::of(g, k)).collect()).collect();
    let svc = service_counts(runs, &keys, &cands, &shapes);
    let n_svc: u32 = svc.iter().map(|x| x.1.iter().sum::<u32>()).sum();
    mark(format!("service evidence: {n_svc} stopping-service spans"));

    // ---- per-train inference (parallel over trains) ----
    let mut train_legs: Vec<Vec<usize>> = vec![Vec::new(); runs.len()];
    for (i, l) in legs.iter().enumerate() {
        train_legs[l.train].push(i);
    }
    let params = model::params();
    let decisions: Vec<Decision> = train_legs
        .par_iter()
        .flat_map_iter(|tl| {
            let mut out: Vec<Decision> = Vec::new();
            // chains of consecutive legs that have candidates
            let mut chain: Vec<usize> = Vec::new();
            let flush = |chain: &mut Vec<usize>, out: &mut Vec<Decision>| {
                if chain.is_empty() {
                    return;
                }
                let obs: Vec<LegObs> = chain
                    .iter()
                    .map(|&li| {
                        let l = &legs[li];
                        let pc = &cands[l.pair];
                        let gidx = class_group(l.class);
                        let compat: Vec<u32> = svc[l.pair].0.iter().map(|x| x[gidx]).collect();
                        let sp = model::service_logprior(&compat, svc[l.pair].1[gidx]);
                        let emit = match l.minutes {
                            Some(m) => model::emissions(&cal, pc, l.class, m),
                            None => {
                                let kmin = pc.iter().map(|k| k.km).fold(f64::INFINITY, f64::min);
                                pc.iter().map(|k| -params.length_prior * (k.km / kmin).ln()).collect()
                            }
                        };
                        let emit: Vec<f64> = emit.iter().zip(&sp).map(|(e, p)| e + p).collect();
                        LegObs { cands: pc, forward: l.forward, emit, dwell_end: l.dwell_end }
                    })
                    .collect();
                let posts = model::posteriors(&obs);
                for (k, &li) in chain.iter().enumerate() {
                    let pc = obs[k].cands;
                    let post = posts[k].clone();
                    let emit = obs[k].emit.clone();
                    let (chosen, base, decision) = decide(pc, &post, &emit);
                    out.push(Decision { leg: li, chosen, base, post, decision });
                }
                chain.clear();
            };
            for &li in tl {
                let has = !cands[legs[li].pair].is_empty();
                let contiguous = chain.last().is_none_or(|&prev| {
                    let (a, b) = (&legs[prev], &legs[li]);
                    runs[a.train].legs[a.k].1 == runs[b.train].legs[b.k].0
                });
                if !has || !contiguous {
                    flush(&mut chain, &mut out);
                }
                if has {
                    chain.push(li);
                } else {
                    out.push(Decision { leg: li, chosen: None, base: None, post: Vec::new(), decision: "no-candidates" });
                }
            }
            flush(&mut chain, &mut out);
            out.into_iter()
        })
        .collect();
    mark(format!("inference: {} legs", decisions.len()));

    // ---- double track as one corridor: every stage-1 path is redrawn within 200 m of itself onto the busiest track (rounds, so
    // "busiest" is judged on already merged usage), then every chosen non-stage-1 corridor the same way ----
    let weight = |pi: usize| pair_legs[pi].len() as u32;
    // The path drawn for a pair's production legs. corridor-v3 takes the shortest gap-free candidate there, for every pair; here only when
    // the stage-1 path crosses a gap, because stage 1 picks by the schedule and the shortest would move the nonstop Chengdu-Chongqing
    // trains off the Chengyu line (and break golden 1). Otherwise the stage-1 path stands.
    let canon_source = |pi: usize| -> Option<&Cand> {
        let base = cands[pi].iter().find(|c| c.is_base);
        match base {
            Some(b) if b.gap_km > 0.0 => cands[pi].iter().filter(|c| c.gap_km == 0.0).min_by(|a, b| a.km.total_cmp(&b.km)).or(base),
            _ => base,
        }
    };
    let mut usage = edge_usage((0..cands.len()).filter_map(|pi| canon_source(pi).map(|b| (b, weight(pi)))));
    let mut canon: Vec<Option<Cand>> = vec![None; cands.len()];
    for _round in 0..REDRAW_ROUNDS {
        canon = (0..cands.len())
            .into_par_iter()
            .map_init(|| Buffers::new(g.n_nodes()), |buf, pi| canon_source(pi).map(|b| redraw(g, &fine, buf, b, &usage)))
            .collect();
        usage = edge_usage(canon.iter().enumerate().filter_map(|(pi, c)| c.as_ref().map(|c| (c, weight(pi)))));
    }
    mark(format!("canonical redraw of {} pair paths", canon.iter().filter(|c| c.is_some()).count()));
    let mut todo: Vec<(usize, usize)> = decisions
        .iter()
        .filter(|d| matches!(d.decision, "switched" | "filled" | "bridge-fix"))
        .filter_map(|d| d.chosen.map(|k| (legs[d.leg].pair, k)))
        .collect();
    todo.sort_unstable();
    todo.dedup();
    let drawn: Vec<Cand> = todo.par_iter().map_init(|| Buffers::new(g.n_nodes()), |buf, &(pi, k)| redraw(g, &fine, buf, &cands[pi][k], &usage)).collect();
    let drawn_at: FxHashMap<(usize, usize), usize> = todo.iter().enumerate().map(|(i, &k)| (k, i)).collect();
    mark(format!("redraw of {} chosen corridors", drawn.len()));

    // ---- outputs: the ways of every leg ----
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut paths: Vec<Vec<Option<Vec<u32>>>> = runs.iter().map(|t| vec![None; t.legs.len()]).collect();
    let mut text = String::new();
    let mut trains_changed: FxHashSet<usize> = FxHashSet::default();
    let canon_ways: Vec<Option<Vec<u32>>> = canon.par_iter().map(|c| c.as_ref().map(|c| g.way_path(&c.nodes))).collect();
    let drawn_ways: Vec<Vec<u32>> = drawn.par_iter().map(|c| g.way_path(&c.nodes)).collect();
    for d in &decisions {
        *counts.entry(d.decision).or_default() += 1;
        let l = &legs[d.leg];
        let t = &runs[l.train];
        let (i, j, _) = t.legs[l.k];
        let pi = l.pair;
        let ways: Option<&Vec<u32>> = match (d.chosen, d.decision) {
            (Some(k), "switched" | "filled" | "bridge-fix") => {
                trains_changed.insert(l.train);
                drawn_at.get(&(pi, k)).map(|&i| &drawn_ways[i])
            }
            (Some(_), _) => canon_ways[pi].as_ref(),
            (None, _) => None,
        };
        if let Some(w) = ways {
            let mut w = w.clone();
            if !l.forward {
                w.reverse();
            }
            paths[l.train][l.k] = Some(w);
        }
        let kdesc = |c: Option<usize>| -> Value {
            match c {
                Some(c) => {
                    let k = &cands[pi][c];
                    json!({ "cand": c, "src": k.source, "km": r1(k.km), "pred_min": k.predict(l.class).map(r1), "post": d.post.get(c).map(|p| (p * 1000.0).round() / 1000.0), "lines": k.lines.iter().map(|x| &x.0).collect::<Vec<_>>() })
                }
                None => Value::Null,
            }
        };
        text.push_str(
            &json!({
                "code": t.code, "class": t.class, "from": t.stops[i].name, "to": t.stops[j].name, "minutes": l.minutes,
                "decision": d.decision, "chosen": kdesc(d.chosen), "production": kdesc(d.base),
                "ways": paths[l.train][l.k].as_ref().map(|w| w.iter().map(|&x| inp.way_ids[x as usize]).collect::<Vec<_>>()),
            })
            .to_string(),
        );
        text.push('\n');
    }
    mark("outputs".to_string());

    // ---- the self-check ----
    let ctx = selfcheck::Ctx { g, cands: &cands, shapes: &shapes, svc: &svc, cal: &cal, runs, keys: &keys };
    let (validation, wrong_switches) = selfcheck::validate(&ctx);
    mark(format!("self-check: {} tests", validation["tests"]));

    let report = json!({
        "legs": legs.len(),
        "pairs": pairs.len(),
        "candidates": cands.iter().map(|c| c.len()).sum::<usize>(),
        "pairs_without_candidates": cands.iter().filter(|c| c.is_empty()).count(),
        "stitches": g.n_stitches,
        "bridges": { "searched": needs_bridge.len(), "bridged": n_bridged },
        "service_spans": n_svc,
        "decisions": counts,
        "trains_with_changed_legs": trains_changed.len(),
        "calibration": (0..NCLASS).map(|c| json!({
            "class": CLASS_NAMES[c], "n": cal.n[c],
            "median_ratio": (cal.median[c].exp() * 1000.0).round() / 1000.0,
            "p5": (cal.q05[c].exp() * 1000.0).round() / 1000.0,
            "p95": (cal.q95[c].exp() * 1000.0).round() / 1000.0,
            "feasibility_floor": (cal.x_min[c].exp() * 1000.0).round() / 1000.0,
        })).collect::<Vec<_>>(),
        "self_check": validation,
    });
    Out { paths, legs_text: text, wrong_switches, report }
}

#[cfg(test)]
pub(super) fn bridge_for_test(g: &RGraph, a: P, b: P, min_minutes: Option<f64>) -> Option<Cand> {
    let p = PairInfo { pa: a, pb: b, chord: hav(a, b).max(0.5), bridged: true, class_mask: 1, dominant: 0, min_minutes };
    let snapper = SnapIndex::new(g);
    min_gap_bridge(g, &p, &snapper.snap(g, a), &snapper.snap(g, b))
}

#[cfg(test)]
pub(super) fn candidates_for_test(g: &RGraph, a: P, b: P, class_mask: u32, base: Option<&[u32]>) -> Vec<Cand> {
    let p = PairInfo {
        pa: a,
        pb: b,
        chord: hav(a, b).max(0.5),
        bridged: false,
        class_mask,
        dominant: (0..NCLASS).find(|c| class_mask & (1 << c) != 0).unwrap_or(0),
        min_minutes: None,
    };
    let (snapper, fine) = (SnapIndex::new(g), FineIndex::new(g));
    let mut buf = Buffers::new(g.n_nodes());
    gen_candidates(g, &fine, &mut buf, &p, &snapper.snap(g, a), &snapper.snap(g, b), base)
}
