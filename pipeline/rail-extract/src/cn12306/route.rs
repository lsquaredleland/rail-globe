//! Routing one stop pair over the graph. Both stations are snapped onto the nearest points of the track (the edge is split there
//! for the search, by a small overlay: the graph itself is shared by all threads and never changes). A* finds the shortest path,
//! then up to three more by making the ways already used 3x dearer; the path is picked by the timetable (`pick`). Where the track has a hole,
//! a bounded search from both ends is joined across a gap of 10 km or less (`frontier_bridge`), and the join must pass two
//! audits: its track km within 1.5x the straight line, and a fastest scheduled time that does not imply more than 355 km/h.
//! A pair with no usable path is drawn as a chord (the straight line).

use super::graph::{cell, hav, Graph};
use rustc_hash::FxHashMap;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// the longest gap bridged between two pieces of track, km
const MAX_GAP_KM: f64 = 10.0;
/// a track path longer than this many times the straight line, that the schedule cannot follow, is a sign of a hole
const DETOUR_RATIO: f64 = 1.2;
/// a bridged path may be this much longer than the straight line, plus 1 km
const BRIDGE_RATIO: f64 = 1.5;
/// a bridged path may not need a faster train than this, km/h
const BRIDGE_MAX_KMH: f64 = 355.0;
/// speed over a gap, km/h
const GAP_KMH: f64 = 120.0;
/// a gap costs this many times a km of track, plus a flat `GAP_FLAT_KM`, so a path only crosses a hole it cannot go round
const GAP_COST_PER_KM: f64 = 10.0;
const GAP_FLAT_KM: f64 = 5.0;
/// stop and start allowance in the minimum run time, minutes
const START_MIN: f64 = 4.0;
/// a station further than this from any track is not routed, km
const SNAP_KM: f64 = 3.0;
/// a candidate within this much of the shortest fitting km may win on slack
const KM_TOLERANCE: f64 = 1.05;

/// one step of a path: (km, way), or None for a gap between two pieces of track
type Hop = Option<(f64, u32)>;

#[derive(Clone, Copy, PartialEq)]
struct F(f64);
impl Eq for F {}
impl PartialOrd for F {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for F {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&o.0)
    }
}

/// a station snapped onto an edge: `t` is the position along u -> v, 0 to 1
pub struct Snap {
    pub dist: f64,
    pub u: u32,
    pub v: u32,
    pub way: u32,
    pub t: f64,
    pub p: (f64, f64),
    pub seg: u32,
}

/// where on the segment a-b the point is nearest (flat approximation around the point): position 0 to 1, distance km, the point
pub fn snap_on_seg(lon: f64, lat: f64, a: (f64, f64), b: (f64, f64)) -> (f64, f64, (f64, f64)) {
    let (kx, ky) = (111.32 * lat.to_radians().cos(), 110.57);
    let (ex, ey) = ((b.0 - a.0) * kx, (b.1 - a.1) * ky);
    let l2 = ex * ex + ey * ey;
    let t = if l2 == 0.0 { 0.0 } else { ((((lon - a.0) * kx) * ex + ((lat - a.1) * ky) * ey) / l2).clamp(0.0, 1.0) };
    let p = (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1));
    (t, ((lon - p.0) * kx).hypot((lat - p.1) * ky), p)
}

/// the `k` nearest points on track within `r` km, one per way, nearest first
pub fn snaps(g: &Graph, lon: f64, lat: f64, k: usize, r: f64) -> Vec<Snap> {
    let (cx, cy) = cell(lon, lat);
    let mut best: FxHashMap<u32, Snap> = FxHashMap::default();
    // r km is at most 3 cells of 0.02 degrees in China
    for gx in cx - 3..=cx + 3 {
        for gy in cy - 3..=cy + 3 {
            for &si in g.seg_grid.get(&(gx, gy)).into_iter().flatten() {
                let s = &g.segs[si as usize];
                let (t, dist, p) = snap_on_seg(lon, lat, g.xy[s.a as usize], g.xy[s.b as usize]);
                if dist <= r && best.get(&s.way).is_none_or(|b| dist < b.dist || (dist == b.dist && si < b.seg)) {
                    best.insert(s.way, Snap { dist, u: s.a, v: s.b, way: s.way, t, p, seg: si });
                }
            }
        }
    }
    let mut v: Vec<Snap> = best.into_values().collect();
    v.sort_by(|a, b| a.dist.total_cmp(&b.dist).then(a.u.cmp(&b.u)).then(a.v.cmp(&b.v)).then(a.way.cmp(&b.way)));
    v.truncate(k);
    v
}

struct Virt {
    id: u32,
    seg: u32,
    p: (f64, f64),
    u: u32,
    v: u32,
    way: u32,
    du: f64,
    dv: f64,
}

/// the graph plus the stations snapped into the middle of an edge for one search
pub struct Net<'a> {
    pub g: &'a Graph,
    virt: Vec<Virt>,
}

impl<'a> Net<'a> {
    pub fn new(g: &'a Graph) -> Net<'a> {
        Net { g, virt: Vec::new() }
    }

    /// the node a snap stands for: an end of its edge, or a new node splitting the edge
    pub fn add(&mut self, sn: &Snap) -> u32 {
        if sn.t <= 1e-9 {
            return sn.u;
        }
        if sn.t >= 1.0 - 1e-9 {
            return sn.v;
        }
        let id = (self.g.n_nodes() + self.virt.len()) as u32;
        let (a, b) = (self.g.xy[sn.u as usize], self.g.xy[sn.v as usize]);
        self.virt.push(Virt { id, seg: sn.seg, p: sn.p, u: sn.u, v: sn.v, way: sn.way, du: hav(a.0, a.1, sn.p.0, sn.p.1), dv: hav(sn.p.0, sn.p.1, b.0, b.1) });
        id
    }

    pub fn xy(&self, n: u32) -> (f64, f64) {
        match (n as usize).checked_sub(self.g.n_nodes()) {
            Some(i) => self.virt[i].p,
            None => self.g.xy[n as usize],
        }
    }

    /// calls `f(to, km, way)` for every edge of `u`
    fn each(&self, u: u32, mut f: impl FnMut(u32, f64, u32)) {
        if let Some(i) = (u as usize).checked_sub(self.g.n_nodes()) {
            let v = &self.virt[i];
            f(v.u, v.du, v.way);
            f(v.v, v.dv, v.way);
            // two stations on the same segment: the track between them
            for o in self.virt.iter().filter(|o| o.seg == v.seg && o.id != v.id) {
                f(o.id, hav(v.p.0, v.p.1, o.p.0, o.p.1), v.way);
            }
            return;
        }
        for s in self.g.segs_at(u) {
            f(if s.a == u { s.b } else { s.a }, s.km, s.way);
        }
        for v in &self.virt {
            if v.u == u {
                f(v.id, v.du, v.way);
            }
            if v.v == u {
                f(v.id, v.dv, v.way);
            }
        }
    }

    /// the shortest edge from u to v
    fn edge(&self, u: u32, v: u32) -> Option<(f64, u32)> {
        let mut best: Option<(f64, u32)> = None;
        self.each(u, |x, d, w| {
            if x == v && best.is_none_or(|b| d < b.0) {
                best = Some((d, w));
            }
        });
        best
    }
}

/// the cost of a way: 1 per km, service track 3, and 3x more for every route round that used it
#[derive(Default, Clone)]
struct Pen(FxHashMap<u32, f64>);

impl Pen {
    fn mult(&self, g: &Graph, w: u32) -> f64 {
        self.0.get(&w).copied().unwrap_or(if g.ways[w as usize].service { 3.0 } else { 1.0 })
    }
}

/// per-thread arrays for the searches, stamped so that a search does not clear them
pub struct Scratch {
    stamp: u32,
    seen: Vec<u32>,
    done: Vec<u32>,
    cost: Vec<f64>,
    prev: Vec<u32>,
    /// the edge (km, way) a node was reached by
    prev_hop: Vec<(f64, u32)>,
}

impl Scratch {
    pub fn new(g: &Graph) -> Scratch {
        let n = g.n_nodes() + 4;
        Scratch { stamp: 0, seen: vec![0; n], done: vec![0; n], cost: vec![0.0; n], prev: vec![0; n], prev_hop: vec![(0.0, 0); n] }
    }
}

/// A* from s to t over `net`: (cost, nodes). None when nothing is cheaper than `bound`, or more than `cap` nodes were settled
fn astar(net: &Net, sc: &mut Scratch, s: u32, t: u32, bound: f64, pen: &Pen, cap: usize) -> Option<(f64, Vec<u32>, Vec<Hop>)> {
    let tp = net.xy(t);
    let h = |n: u32| {
        let p = net.xy(n);
        hav(p.0, p.1, tp.0, tp.1)
    };
    sc.stamp += 1;
    let st = sc.stamp;
    let mut heap = BinaryHeap::new();
    sc.cost[s as usize] = 0.0;
    sc.seen[s as usize] = st;
    heap.push(Reverse((F(h(s)), F(0.0), s)));
    let mut settled = 0usize;
    while let Some(Reverse((F(f), F(gc), u))) = heap.pop() {
        if sc.done[u as usize] == st {
            continue;
        }
        if f >= bound {
            return None;
        }
        if u == t {
            let (mut path, mut hops) = (vec![u], Vec::new());
            let mut x = u;
            while x != s {
                hops.push(Some(sc.prev_hop[x as usize]));
                x = sc.prev[x as usize];
                path.push(x);
            }
            path.reverse();
            hops.reverse();
            return Some((gc, path, hops));
        }
        sc.done[u as usize] = st;
        settled += 1;
        if settled > cap {
            return None;
        }
        net.each(u, |v, d, w| {
            let ng = gc + d * pen.mult(net.g, w);
            if sc.seen[v as usize] != st || ng < sc.cost[v as usize] {
                sc.seen[v as usize] = st;
                sc.cost[v as usize] = ng;
                sc.prev[v as usize] = u;
                sc.prev_hop[v as usize] = (d, w);
                heap.push(Reverse((F(ng + h(v)), F(ng), v)));
            }
        });
    }
    None
}

/// one way of getting from A to B
pub struct Cand {
    pub src: &'static str,
    /// the hops of the search path that cross a gap of more than 50 m instead of track
    pub jl: Vec<usize>,
    cells: FxHashMap<(i32, i32), f64>,
    pub km: f64,
    pub min_time: f64,
    /// ways on the path with their km, in the order the path meets them
    pub wk: Vec<(u32, f64)>,
    pub gap_km: f64,
    /// km on ways whose speed is a tag, inferred, or a default
    pub cov: [f64; 3],
    /// the graph nodes of the path (the stations' own points on an edge left out); two that are not joined by an edge are a gap
    pub nodes: Vec<u32>,
}

/// the path's km per 0.01 degree cell, to tell two paths on the same track from two corridors
fn cell_km(net: &Net, pn: &[u32]) -> FxHashMap<(i32, i32), f64> {
    let mut c: FxHashMap<(i32, i32), f64> = FxHashMap::default();
    for w in pn.windows(2) {
        if w[0] == w[1] {
            continue;
        }
        let (a, b) = (net.xy(w[0]), net.xy(w[1]));
        *c.entry((((a.0 + b.0) * 50.0) as i32, ((a.1 + b.1) * 50.0) as i32)).or_default() += hav(a.0, a.1, b.0, b.1);
    }
    c
}

fn overlap(a: &Cand, b: &Cand) -> f64 {
    let shared: f64 = b.cells.iter().map(|(w, k)| a.cells.get(w).copied().unwrap_or(0.0).min(*k)).sum();
    shared / a.km.min(b.km).max(1e-9)
}

/// the candidate for a node path and the hops between its nodes: km, minimum run time from the ways' speeds, and the gaps
fn mk(net: &Net, pn: &[u32], hops: &[Hop], src: &'static str) -> Cand {
    let mut wk: Vec<(u32, f64)> = Vec::new();
    let mut at: FxHashMap<u32, usize> = FxHashMap::default();
    let (mut km, mut mt, mut gap_km) = (0.0, START_MIN, 0.0);
    let mut jl = Vec::new();
    for (i, h) in hops.iter().enumerate() {
        let d = match *h {
            Some((d, way)) => {
                match at.get(&way) {
                    Some(&j) => wk[j].1 += d,
                    None => {
                        at.insert(way, wk.len());
                        wk.push((way, d));
                    }
                }
                mt += d / net.g.ways[way as usize].speed * 60.0;
                d
            }
            None => {
                let (a, b) = (net.xy(pn[i]), net.xy(pn[i + 1]));
                let d = hav(a.0, a.1, b.0, b.1);
                gap_km += d;
                mt += d / GAP_KMH * 60.0;
                if d > 0.05 {
                    jl.push(i);
                }
                d
            }
        };
        km += d;
    }
    let mut cov = [0.0; 3];
    for &(w, k) in &wk {
        cov[net.g.ways[w as usize].speed_src as usize] += k;
    }
    let nodes = pn.iter().copied().filter(|&n| (n as usize) < net.g.n_nodes()).collect();
    Cand { src, jl, cells: cell_km(net, pn), km, min_time: mt, wk, gap_km, cov, nodes }
}

/// the hop from `a` to `b` when two pieces of track are joined: the edge if there is one, else a gap
fn join(net: &Net, a: u32, b: u32) -> Hop {
    net.edge(a, b)
}

/// the nodes within `limit` (cost) of s, in the order they were first reached, with cost and the node before
fn dijkstra(net: &Net, s: u32, limit: f64) -> (Vec<u32>, FxHashMap<u32, f64>, FxHashMap<u32, (u32, f64, u32)>) {
    let mut cost: FxHashMap<u32, f64> = FxHashMap::default();
    let mut prev: FxHashMap<u32, (u32, f64, u32)> = FxHashMap::default();
    let mut order = vec![s];
    cost.insert(s, 0.0);
    let mut heap = BinaryHeap::new();
    heap.push(Reverse((F(0.0), s)));
    let mut done: rustc_hash::FxHashSet<u32> = Default::default();
    let pen = Pen::default();
    while let Some(Reverse((F(c), u))) = heap.pop() {
        if done.len() >= 700_000 {
            break;
        }
        if !done.insert(u) {
            continue;
        }
        net.each(u, |v, d, w| {
            let nc = c + d * pen.mult(net.g, w);
            if nc <= limit && cost.get(&v).is_none_or(|&o| nc < o) {
                if !cost.contains_key(&v) {
                    order.push(v);
                }
                cost.insert(v, nc);
                prev.insert(v, (u, d, w));
                heap.push(Reverse((F(nc), v)));
            }
        });
    }
    (order, cost, prev)
}

/// no short path: grow the cheap region around both ends, then join them across the smallest gap of `MAX_GAP_KM` or less
fn frontier_bridge(net: &Net, s: u32, t: u32, chord: f64, fs: Option<f64>, max_km: f64) -> Option<(Vec<u32>, Vec<Hop>)> {
    // the audits the joined path must pass anyway, so the cheapest join is not one that fails them, and the search needs go no further
    let km_cap = bridge_km_cap(chord, fs).min(max_km);
    let limit = km_cap;
    let (order_a, ca, pa) = dijkstra(net, s, limit);
    let (order_b, cb, pb) = dijkstra(net, t, limit);
    let mut by_cell: FxHashMap<(i32, i32), Vec<u32>> = FxHashMap::default();
    for &b in &order_b {
        let p = net.xy(b);
        by_cell.entry(cell(p.0, p.1)).or_default().push(b);
    }
    // 10 km is at most 8 cells of 0.02 degrees anywhere in China
    const R: i32 = 9;
    let mut best: Option<(f64, u32, u32)> = None;
    for &a in &order_a {
        let pa_ = net.xy(a);
        let (cx, cy) = cell(pa_.0, pa_.1);
        for gx in cx - R..=cx + R {
            for gy in cy - R..=cy + R {
                for &b in by_cell.get(&(gx, gy)).into_iter().flatten() {
                    let pb_ = net.xy(b);
                    let d = hav(pa_.0, pa_.1, pb_.0, pb_.1);
                    if d > MAX_GAP_KM {
                        continue;
                    }
                    if ca[&a] + d + cb[&b] > km_cap {
                        continue;
                    }
                    let tot = ca[&a] + gap_cost(d) + cb[&b];
                    if best.is_none_or(|x| tot < x.0) {
                        best = Some((tot, a, b));
                    }
                }
            }
        }
    }
    let (_, a, b) = best?;
    let (mut nodes, mut hops) = (vec![a], Vec::new());
    while *nodes.last().unwrap() != s {
        let (p, d, w) = pa[nodes.last().unwrap()];
        hops.push(Some((d, w)));
        nodes.push(p);
    }
    nodes.reverse();
    hops.reverse();
    if a != b {
        hops.push(join(net, a, b));
        nodes.push(b);
    }
    while *nodes.last().unwrap() != t {
        let (p, d, w) = pb[nodes.last().unwrap()];
        hops.push(Some((d, w)));
        nodes.push(p);
    }
    Some((nodes, hops))
}

/// the longest a bridged path may be: 1.5x the straight line plus 1 km, and short enough that the fastest time does not imply over 355 km/h
fn bridge_km_cap(chord: f64, fs: Option<f64>) -> f64 {
    let cap = BRIDGE_RATIO * chord + 1.0;
    fs.map_or(cap, |f| cap.min(BRIDGE_MAX_KMH * f / 60.0))
}

/// what crossing a gap of `d` km costs in the searches
fn gap_cost(d: f64) -> f64 {
    d * GAP_COST_PER_KM + GAP_FLAT_KM
}

/// the nearest points of two pieces of track that are not connected (a different component), within `MAX_GAP_KM`, in the box around A and B
fn bridge(g: &Graph, ua: u32, ub: u32, a: (f64, f64), b: (f64, f64)) -> Option<(f64, u32, u32)> {
    let (ca, cb) = (g.comp[ua as usize], g.comp[ub as usize]);
    if ca == cb || ca < 0 || cb < 0 {
        return None;
    }
    let (lo0, lo1) = (a.0.min(b.0) - 0.1, a.0.max(b.0) + 0.1);
    let (la0, la1) = (a.1.min(b.1) - 0.1, a.1.max(b.1) + 0.1);
    let mut side_a: Vec<u32> = Vec::new();
    let mut side_b: FxHashMap<(i32, i32), Vec<u32>> = FxHashMap::default();
    for gx in (lo0 * 50.0) as i32..=(lo1 * 50.0) as i32 {
        for gy in (la0 * 50.0) as i32..=(la1 * 50.0) as i32 {
            for &n in g.grid.get(&(gx, gy)).into_iter().flatten() {
                let c = g.comp[n as usize];
                if c == ca {
                    side_a.push(n);
                } else if c == cb {
                    side_b.entry((gx, gy)).or_default().push(n);
                }
            }
        }
    }
    let mut best: Option<(f64, u32, u32)> = None;
    for &x in &side_a {
        let p = g.xy[x as usize];
        let (cx, cy) = cell(p.0, p.1);
        for gx in cx - 9..=cx + 9 {
            for gy in cy - 9..=cy + 9 {
                for &y in side_b.get(&(gx, gy)).into_iter().flatten() {
                    let q = g.xy[y as usize];
                    let d = hav(p.0, p.1, q.0, q.1);
                    if d <= MAX_GAP_KM && best.is_none_or(|bb| d < bb.0) {
                        best = Some((d, x, y));
                    }
                }
            }
        }
    }
    best
}

/// which candidate the timetable picks: among those whose minimum run time fits the fastest scheduled time (`fs`, minutes), the ones
/// within `KM_TOLERANCE` of the shortest, and of those the one with the least slack. Without a time the shortest; when nothing fits
/// the one with the least minimum time (flag `too_fast`). Input is (km, minimum minutes).
pub fn pick(cands: &[(f64, f64)], fs: Option<f64>) -> (Option<usize>, &'static str) {
    if cands.is_empty() {
        return (None, "no_candidate");
    }
    let first_min = |it: &mut dyn Iterator<Item = usize>, key: &dyn Fn(usize) -> (f64, f64)| -> usize {
        it.fold(None::<(usize, (f64, f64))>, |b, i| match b {
            Some((_, kb)) if kb <= key(i) => b,
            _ => Some((i, key(i))),
        })
        .unwrap()
        .0
    };
    let Some(fs) = fs else {
        return (Some(first_min(&mut (0..cands.len()), &|i| (cands[i].0, 0.0))), "no_time");
    };
    let fit: Vec<usize> = (0..cands.len()).filter(|&i| cands[i].1 <= fs).collect();
    if fit.is_empty() {
        return (Some(first_min(&mut (0..cands.len()), &|i| (cands[i].1, 0.0))), "too_fast");
    }
    let shortest = fit.iter().map(|&i| cands[i].0).fold(f64::INFINITY, f64::min);
    let slack = |i: usize| ((fs - cands[i].1) * 1e6).round() / 1e6;
    let i = first_min(&mut fit.iter().copied().filter(|&i| cands[i].0 <= KM_TOLERANCE * shortest), &|i| (slack(i), cands[i].0));
    (Some(i), "")
}

/// nothing fits the fastest time: the shortest candidate, unless a longer one is at least 25% faster in minimum time and has at least
/// 90% of its km on tagged maxspeed (the shortest may only look slow because its speeds are guesses). Input (km, minutes, tagged share).
pub fn too_fast_pick(c: &[(f64, f64, f64)]) -> usize {
    let s = (0..c.len()).min_by(|&a, &b| c[a].0.total_cmp(&c[b].0)).unwrap();
    (0..c.len()).filter(|&i| c[i].0 > c[s].0 && c[i].1 <= 0.75 * c[s].1 && c[i].2 >= 0.9).min_by(|&a, &b| c[a].1.total_cmp(&c[b].1)).unwrap_or(s)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Ok,
    Bridged,
    Chord,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Bridged => "bridged",
            Status::Chord => "chord",
        }
    }
}

pub struct Routed {
    pub status: Status,
    pub flag: &'static str,
    pub chord_km: f64,
    /// candidates left after the audits
    pub cands: usize,
    pub slack: Option<f64>,
    /// 0: no bridge search, 1: searched for a hole (no usable track path), 2: searched because the track path is a detour the schedule cannot follow
    pub searched: u8,
    pub pick: Option<Cand>,
}

/// route one pair; `fs` is the fastest scheduled time in minutes
pub fn route(g: &Graph, sc: &mut Scratch, a: (f64, f64), b: (f64, f64), fs: Option<f64>) -> Routed {
    let chord = hav(a.0, a.1, b.0, b.1);
    let none = |flag| Routed { status: Status::Chord, flag, chord_km: chord, cands: 0, slack: None, searched: 0, pick: None };
    let (sa, sb) = (snaps(g, a.0, a.1, 4, SNAP_KM), snaps(g, b.0, b.1, 4, SNAP_KM));
    if sa.is_empty() || sb.is_empty() {
        return none("no_snap");
    }
    let cap = (6000.0 * chord).clamp(3e5, 4e6) as usize;
    let mut combos: Vec<(f64, &Snap, &Snap)> = sa.iter().flat_map(|x| sb.iter().map(move |y| (x.dist + y.dist, x, y))).collect();
    combos.sort_by(|p, q| p.0.total_cmp(&q.0));
    combos.truncate(3);

    let max_km = (3.0 * chord).max(chord + 20.0);
    let mut pen = Pen::default();
    let mut cands: Vec<Cand> = Vec::new();
    let add = |cands: &mut Vec<Cand>, c: Cand| {
        if !cands.iter().any(|o| overlap(o, &c) >= 0.8 && (o.km - c.km).abs() < 0.05 * o.km + 1.0) {
            cands.push(c);
        }
    };
    const ROUND: [&str; 4] = ["astar0", "astar1", "astar2", "astar3"];
    for (rnd, &src) in ROUND.iter().enumerate() {
        let mut best: Option<(f64, Cand)> = None;
        for &(_, x, y) in &combos {
            let mut net = Net::new(g);
            let (s, t) = (net.add(x), net.add(y));
            if s == t {
                continue;
            }
            let bound = best.as_ref().map_or(max_km * 4.0, |b| b.0);
            let mut rr = astar(&net, sc, s, t, bound, &pen, cap);
            if rr.is_none() && rnd == 0 && best.is_none() {
                if let Some((d, ga, gb)) = bridge(g, x.u, y.u, a, b) {
                    if let (Some(p1), Some(p2)) = (astar(&net, sc, s, ga, 1e9, &pen, cap), astar(&net, sc, gb, t, 1e9, &pen, cap)) {
                        let hop = join(&net, ga, gb);
                        let cost = p1.0 + p2.0 + if hop.is_some() { d } else { gap_cost(d) };
                        rr = Some((cost, [p1.1, p2.1].concat(), [p1.2, vec![hop], p2.2].concat()));
                    }
                }
            }
            if let Some((cost, nodes, hops)) = rr {
                if best.as_ref().is_none_or(|b| cost < b.0) {
                    best = Some((cost, mk(&net, &nodes, &hops, src)));
                }
            }
        }
        let Some((_, c)) = best else { break };
        for &(w, _) in &c.wk {
            let m = pen.mult(g, w);
            pen.0.insert(w, m * 3.0);
        }
        if c.km <= max_km {
            add(&mut cands, c);
        }
    }
    // bridge a gap only when track alone gives no answer: no path within the ratio audit that the fastest time allows (or, when the
    // two ends are one piece of track, no path within the ratio at all: then the time is the guess that is wrong, not the track)
    let ratio_cap = BRIDGE_RATIO * chord + 1.0;
    let one_piece = g.comp[combos[0].1.u as usize] == g.comp[combos[0].2.u as usize];
    let usable = cands.iter().any(|c| c.jl.is_empty() && c.km <= ratio_cap && (one_piece || c.km <= bridge_km_cap(chord, fs)));
    // a hole also shows as a detour the schedule cannot follow: no track path fits the fastest time and the best one is over 1.2x the
    // straight line; a bridge then has to be at most 0.9x that path
    let best_track = cands.iter().filter(|c| c.jl.is_empty()).map(|c| c.km).fold(f64::INFINITY, f64::min);
    let too_fast = fs.is_some_and(|f| !cands.is_empty() && !cands.iter().any(|c| c.jl.is_empty() && c.min_time <= f));
    let detour = usable && too_fast && best_track > DETOUR_RATIO * chord;
    let mut searched = 0;
    if (!usable || detour) && chord > 0.3 {
        searched = if usable { 2 } else { 1 };
        let mut net = Net::new(g);
        let (s, t) = (net.add(combos[0].1), net.add(combos[0].2));
        if s != t {
            let cap = if detour { 0.9 * best_track } else { f64::INFINITY };
            if let Some((nodes, hops)) = frontier_bridge(&net, s, t, chord, fs, cap) {
                let c = mk(&net, &nodes, &hops, "bridged");
                if c.km <= max_km {
                    add(&mut cands, c);
                }
            }
        }
    }
    // a path across a gap passes both audits; the schedule cannot be faster than the path allows
    cands.retain(|c| c.jl.is_empty() || !(c.km > BRIDGE_RATIO * chord + 1.0 || fs.is_some_and(|f| c.km / (f / 60.0) > BRIDGE_MAX_KMH)));
    let (mut i, flag) = pick(&cands.iter().map(|c| (c.km, c.min_time)).collect::<Vec<_>>(), fs);
    let n = cands.len();
    if flag == "too_fast" {
        i = Some(too_fast_pick(&cands.iter().map(|c| (c.km, c.min_time, c.cov[0] / c.km.max(1e-9))).collect::<Vec<_>>()));
    }
    match i {
        None => Routed { cands: n, searched, ..none(flag) },
        Some(i) => {
            let mut c = cands.swap_remove(i);
            c.cells = FxHashMap::default();
            let status = if c.jl.is_empty() { Status::Ok } else { Status::Bridged };
            Routed { status, flag, chord_km: chord, cands: n, slack: fs.map(|f| f - c.min_time), searched, pick: Some(c) }
        }
    }
}
