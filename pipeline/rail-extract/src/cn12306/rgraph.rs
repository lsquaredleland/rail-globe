//! The graph of the corridor router (corridor-v3): the track of `graph.rs` as an adjacency list whose edges also carry a speed limit,
//! a high-speed flag, a service flag and the way they come from, so a path can be priced in time. Components that come within 250 m of
//! each other are stitched with one slow edge, where at least one of the two nodes is a dead end (a gap repair, flagged `F_STITCH`); a
//! dangling track end is where an unmapped gap starts (`degree` 1). Searches: A* from a
//! set of snapped nodes (`route`), Dijkstra with a caller's price and hops between parallel tracks of one named line (`route_custom`),
//! and the region within a track distance (`reach`).
//!
//! Every order here is fixed (node numbers, the order edges were read, ties broken by node number), so a search gives the same path on
//! any thread count.

use super::graph::{self, Graph, NO_LINE};
use rustc_hash::FxHashMap;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

/// (lon, lat) in degrees
pub type P = (f64, f64);

pub const F_HSR: u8 = 1;
pub const F_STITCH: u8 = 4;
pub const F_SERVICE: u8 = 8;
/// edge used by a per-pair (stage 1) path: on double track a search prefers it, so a re-routed train lands on the edges that are
/// already drawn for the line
pub const F_PROD: u8 = 16;
const PROD_DISCOUNT: f64 = 0.97;
/// an edge that is no way (a stitch)
pub const NO_WAY: u32 = u32::MAX;
/// components closer than this are stitched, km
const STITCH_KM: f64 = 0.25;

pub fn hav(a: P, b: P) -> f64 {
    graph::hav(a.0, a.1, b.0, b.1)
}

/// initial bearing in degrees from `a` to `b`
pub fn bearing(a: P, b: P) -> f64 {
    let (p1, p2) = (a.1.to_radians(), b.1.to_radians());
    let dl = (b.0 - a.0).to_radians();
    let y = dl.sin() * p2.cos();
    let x = p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

#[derive(Clone, Copy)]
pub struct Edge {
    pub to: u32,
    pub km: f32,
    pub vmax: u16,
    pub flags: u8,
    /// index of the way in the stage-1 graph, `NO_WAY` for a stitch
    pub way: u32,
}

pub struct RGraph {
    pub xy: Vec<P>,
    adj_start: Vec<u32>,
    adj: Vec<Edge>,
    /// distinct track neighbours before stitching; 1 marks a dangling track end, where unmapped gaps start
    pub degree: Vec<u8>,
    way_line: Vec<u32>,
    /// the names of the ways (`line`)
    pub lines: Vec<String>,
    pub n_stitches: usize,
}

impl RGraph {
    pub fn n_nodes(&self) -> usize {
        self.xy.len()
    }

    pub fn edges(&self, u: u32) -> &[Edge] {
        &self.adj[self.adj_start[u as usize] as usize..self.adj_start[u as usize + 1] as usize]
    }

    /// the first edge u -> v, if the two nodes are adjacent
    pub fn edge(&self, u: u32, v: u32) -> Option<&Edge> {
        self.edges(u).iter().find(|e| e.to == v)
    }

    /// the name of the edge's way (`NO_LINE` for none)
    pub fn line(&self, e: &Edge) -> u32 {
        if e.way == NO_WAY {
            NO_LINE
        } else {
            self.way_line[e.way as usize]
        }
    }

    /// flags every edge of the node paths as F_PROD; returns how many undirected edges were flagged
    pub fn mark_prod<'a>(&mut self, paths: impl Iterator<Item = &'a [u32]>) -> usize {
        let mut n = 0;
        for p in paths {
            for w in p.windows(2) {
                for (x, y) in [(w[0], w[1]), (w[1], w[0])] {
                    let (lo, hi) = (self.adj_start[x as usize] as usize, self.adj_start[x as usize + 1] as usize);
                    for e in self.adj[lo..hi].iter_mut() {
                        if e.to == y && e.flags & F_PROD == 0 {
                            e.flags |= F_PROD;
                            if x < y {
                                n += 1;
                            }
                        }
                    }
                }
            }
        }
        n
    }

    /// the ways a node path runs over, in order, each once in a row; a gap or a hop between parallel tracks has no way
    pub fn way_path(&self, nodes: &[u32]) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for w in nodes.windows(2) {
            if let Some(e) = self.edge(w[0], w[1]).filter(|e| e.way != NO_WAY) {
                if out.last() != Some(&e.way) {
                    out.push(e.way);
                }
            }
        }
        out
    }
}

/// the speed limit used for pricing a way: the tagged maxspeed, else 250 on a high-speed line, else by `usage`; service track 30
fn way_vmax(w: &graph::Way) -> u16 {
    let v = if w.service {
        30.0
    } else if w.speed_src == 0 {
        w.speed.clamp(10.0, 350.0)
    } else if w.hsr {
        250.0
    } else {
        w.usage_kmh
    };
    v.round() as u16
}

/// Gap repair: for each pair of components the closest pair of nodes within `STITCH_KM`, one of them a dead end (a node with one track
/// neighbour), ordered by component pair. Ways that merely come near each other are not joined.
fn stitches(g: &Graph, degree: &[u8]) -> Vec<(f64, u32, u32)> {
    let cell = |p: P| ((p.0 * 50.0) as i32, (p.1 * 50.0) as i32);
    let mut grid: FxHashMap<(i32, i32), Vec<u32>> = FxHashMap::default();
    for (i, &c) in g.xy.iter().enumerate() {
        if g.comp[i] >= 0 {
            grid.entry(cell(c)).or_default().push(i as u32);
        }
    }
    let mut best: BTreeMap<(i32, i32), (f64, u32, u32)> = BTreeMap::new();
    for u in 0..g.xy.len() as u32 {
        let (cu, pt) = (g.comp[u as usize], g.xy[u as usize]);
        if cu < 0 {
            continue;
        }
        let (cx, cy) = cell(pt);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &v in grid.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                    let cv = g.comp[v as usize];
                    if cv == cu {
                        continue;
                    }
                    let d = hav(pt, g.xy[v as usize]);
                    if d <= STITCH_KM && (degree[u as usize] == 1 || degree[v as usize] == 1) {
                        let e = best.entry((cu.min(cv), cu.max(cv))).or_insert((f64::INFINITY, 0, 0));
                        if d < e.0 {
                            *e = (d, u, v);
                        }
                    }
                }
            }
        }
    }
    best.into_values().collect()
}

pub fn build(g: &Graph) -> RGraph {
    let n = g.xy.len();
    let vmax: Vec<u16> = g.ways.iter().map(way_vmax).collect();
    let flags: Vec<u8> = g.ways.iter().map(|w| if w.hsr { F_HSR } else { 0 } | if w.service { F_SERVICE } else { 0 }).collect();
    // the degree before the stitches
    let mut nb: Vec<Vec<u32>> = vec![Vec::new(); n];
    for s in g.segs.iter().filter(|s| s.a != s.b && s.km > 0.0) {
        nb[s.a as usize].push(s.b);
        nb[s.b as usize].push(s.a);
    }
    let degree: Vec<u8> = nb
        .iter_mut()
        .map(|v| {
            v.sort_unstable();
            v.dedup();
            v.len().min(255) as u8
        })
        .collect();
    drop(nb);
    let stitched = stitches(g, &degree);

    // every edge in the order it is read: the segments, then the stitches; called once to count and once to fill
    let each = |f: &mut dyn FnMut(u32, Edge)| {
        for s in g.segs.iter().filter(|s| s.a != s.b && s.km > 0.0) {
            let e = Edge { to: s.b, km: s.km as f32, vmax: vmax[s.way as usize], flags: flags[s.way as usize], way: s.way };
            f(s.a, e);
            f(s.b, Edge { to: s.a, ..e });
        }
        for &(d, u, v) in &stitched {
            let e = Edge { to: v, km: d as f32, vmax: 40, flags: F_STITCH, way: NO_WAY };
            f(u, e);
            f(v, Edge { to: u, ..e });
        }
    };
    let mut adj_start = vec![0u32; n + 1];
    each(&mut |u, _| adj_start[u as usize + 1] += 1);
    for i in 0..n {
        adj_start[i + 1] += adj_start[i];
    }
    let mut fill = adj_start.clone();
    let blank = Edge { to: 0, km: 0.0, vmax: 0, flags: 0, way: NO_WAY };
    let mut adj = vec![blank; adj_start[n] as usize];
    each(&mut |u, e| {
        adj[fill[u as usize] as usize] = e;
        fill[u as usize] += 1;
    });
    RGraph { xy: g.xy.clone(), adj_start, adj, degree, way_line: g.ways.iter().map(|w| w.line).collect(), lines: g.lines.clone(), n_stitches: stitched.len() }
}

/// how a search prices an edge
#[derive(Clone, Copy)]
pub enum Metric {
    /// hours at min(edge limit, `cap`) km/h; edges of dedicated high-speed lines (highspeed=yes, limit >= 250) cost `hsr_mult` times as much
    /// (conventional trains cannot use them)
    Time { cap: f64, hsr_mult: f64 },
    /// plain track km
    Distance,
}

impl Metric {
    pub fn cost(&self, e: &Edge) -> f64 {
        let c = self.raw_cost(e);
        if e.flags & F_PROD != 0 {
            c * PROD_DISCOUNT
        } else {
            c
        }
    }

    fn raw_cost(&self, e: &Edge) -> f64 {
        let km = e.km as f64;
        match *self {
            Metric::Distance => km,
            Metric::Time { cap, hsr_mult } => {
                let t = km / (e.vmax as f64).min(cap);
                if e.flags & F_HSR != 0 && e.vmax >= 250 {
                    t * hsr_mult
                } else {
                    t
                }
            }
        }
    }

    /// an admissible lower bound for `km` of straight line
    fn bound(&self, km: f64) -> f64 {
        let b = match *self {
            Metric::Distance => km,
            Metric::Time { .. } => km / 350.0,
        };
        b * PROD_DISCOUNT
    }

    /// the cost of reaching the track from a station point `km` away
    pub fn snap_cost(&self, km: f64) -> f64 {
        match *self {
            Metric::Distance => km * 2.0,
            Metric::Time { .. } => km / 30.0,
        }
    }
}

/// per-thread search state, reused across searches (stamped, never cleared)
pub struct Buffers {
    g: Vec<f64>,
    stamp: Vec<u32>,
    parent: Vec<u32>,
    run: u32,
}

impl Buffers {
    pub fn new(n: usize) -> Self {
        Buffers { g: vec![0.0; n], stamp: vec![0; n], parent: vec![0; n], run: 0 }
    }

    fn next_run(&mut self) -> u32 {
        self.run = self.run.wrapping_add(1);
        if self.run == 0 {
            self.stamp.iter_mut().for_each(|s| *s = 0);
            self.run = 1;
        }
        self.run
    }
}

/// a station's attachment: the track nodes near its point with their distance (km), nearest first
pub type Snap = Vec<(u32, f64)>;

fn grid_of(g: &RGraph, scale: f64) -> FxHashMap<(i32, i32), Vec<u32>> {
    let mut grid: FxHashMap<(i32, i32), Vec<u32>> = FxHashMap::default();
    for (i, c) in g.xy.iter().enumerate() {
        grid.entry(((c.0 * scale) as i32, (c.1 * scale) as i32)).or_default().push(i as u32);
    }
    grid
}

pub struct SnapIndex {
    grid: FxHashMap<(i32, i32), Vec<u32>>,
}

impl SnapIndex {
    pub fn new(g: &RGraph) -> Self {
        SnapIndex { grid: grid_of(g, 50.0) }
    }

    /// the track nodes within 1.5 km of `pt` (up to 48, nearest first); if none, the nearest node within 6 km
    pub fn snap(&self, g: &RGraph, pt: P) -> Snap {
        let (cx, cy) = ((pt.0 * 50.0) as i32, (pt.1 * 50.0) as i32);
        let mut near: Vec<(u32, f64)> = Vec::new();
        for dx in -4..=4 {
            for dy in -4..=4 {
                for &v in self.grid.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                    near.push((v, hav(pt, g.xy[v as usize])));
                }
            }
        }
        near.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        let close: Snap = near.iter().copied().filter(|x| x.1 <= 1.5).take(48).collect();
        if !close.is_empty() {
            return close;
        }
        if let Some(&x) = near.first().filter(|x| x.1 <= 6.0) {
            return vec![x];
        }
        // a wider ring for a station further than the grid reaches
        let mut best: Option<(u32, f64)> = None;
        for dx in -16..=16 {
            for dy in -16..=16 {
                for &v in self.grid.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                    let d = hav(pt, g.xy[v as usize]);
                    if d <= 6.0 && best.is_none_or(|b| d < b.1) {
                        best = Some((v, d));
                    }
                }
            }
        }
        best.into_iter().collect()
    }
}

/// a fine grid (about 0.28 km) over all nodes, for "everything near this corridor"
pub struct FineIndex {
    grid: FxHashMap<(i32, i32), Vec<u32>>,
}

const FINE_CELL: f64 = 400.0;

impl FineIndex {
    pub fn new(g: &RGraph) -> Self {
        FineIndex { grid: grid_of(g, FINE_CELL) }
    }

    /// the nodes within `km` (at most about 0.28) of `p`
    pub fn near<'a>(&'a self, g: &'a RGraph, p: P, km: f64) -> impl Iterator<Item = u32> + 'a {
        let (cx, cy) = ((p.0 * FINE_CELL) as i32, (p.1 * FINE_CELL) as i32);
        (-1..=1)
            .flat_map(move |dx| (-1..=1).map(move |dy| (cx + dx, cy + dy)))
            .filter_map(|c| self.grid.get(&c))
            .flatten()
            .copied()
            .filter(move |&v| hav(p, g.xy[v as usize]) <= km)
    }
}

/// penalty per undirected edge
pub type Penalty = FxHashMap<(u32, u32), f64>;

/// multi-source, multi-target A* from the `from` snap set to the `to` snap set (each node entered and left at its snap cost), edges priced
/// by `metric` and multiplied by `penalty` where listed. Gives up once the bound exceeds `max_cost`. Returns (cost, node path).
#[allow(clippy::too_many_arguments)]
pub fn route(g: &RGraph, buf: &mut Buffers, from: &Snap, to: &Snap, to_pt: P, metric: Metric, penalty: &Penalty, max_cost: f64) -> Option<(f64, Vec<u32>)> {
    let run = buf.next_run();
    let radius = to.iter().map(|x| x.1).fold(0.0, f64::max);
    let targets: FxHashMap<u32, f64> = to.iter().map(|&(v, d)| (v, metric.snap_cost(d))).collect();
    let h = |v: u32| metric.bound((hav(g.xy[v as usize], to_pt) - radius).max(0.0));
    let mut heap: BinaryHeap<Reverse<(u64, u32)>> = BinaryHeap::new();
    for &(s, d) in from {
        let c = metric.snap_cost(d);
        if buf.stamp[s as usize] != run || c < buf.g[s as usize] {
            buf.stamp[s as usize] = run;
            buf.g[s as usize] = c;
            buf.parent[s as usize] = s;
            heap.push(Reverse(((c + h(s)).to_bits(), s)));
        }
    }
    let mut best: Option<(f64, u32)> = None;
    while let Some(Reverse((fb, u))) = heap.pop() {
        let f = f64::from_bits(fb);
        if f > max_cost || best.is_some_and(|b| f >= b.0) {
            break;
        }
        let gu = buf.g[u as usize];
        if f > gu + h(u) + 1e-9 {
            continue; // stale entry
        }
        if let Some(tc) = targets.get(&u) {
            let total = gu + tc;
            if best.is_none_or(|b| total < b.0) {
                best = Some((total, u));
            }
        }
        for e in g.edges(u) {
            let mut c = metric.cost(e);
            if let Some(p) = penalty.get(&(u.min(e.to), u.max(e.to))) {
                c *= p;
            }
            let ng = gu + c;
            let v = e.to as usize;
            if buf.stamp[v] != run || ng < buf.g[v] {
                buf.stamp[v] = run;
                buf.g[v] = ng;
                buf.parent[v] = u;
                heap.push(Reverse(((ng + h(e.to)).to_bits(), e.to)));
            }
        }
    }
    let (cost, end) = best?;
    let mut path = vec![end];
    let mut cur = end;
    while buf.parent[cur as usize] != cur {
        cur = buf.parent[cur as usize];
        path.push(cur);
    }
    path.reverse();
    Some((cost, path))
}

/// a redraw may step to a node within `HOP_KM` on a way of the same named line (the other track of a double line, mapped as separate ways
/// that meet only at crossovers) at the hop length plus `HOP_COST`; distinct lines (a high-speed line beside a conventional one) never merge
pub const HOP_KM: f64 = 0.03;
const HOP_COST: f64 = 0.002;

fn lines_at(g: &RGraph, v: u32) -> impl Iterator<Item = u32> + '_ {
    g.edges(v).iter().map(|e| g.line(e)).filter(|l| *l != NO_LINE)
}

fn same_line(g: &RGraph, u: u32, v: u32) -> bool {
    lines_at(g, u).any(|a| lines_at(g, v).any(|b| a == b))
}

/// Dijkstra from `src` to `dst` with the caller's edge price (to redraw a corridor on the most used track). With `hop`, parallel tracks of one
/// line are joined by virtual hops (see `HOP_KM`); a hop shows in the path as two nodes that are not adjacent.
pub fn route_custom(
    g: &RGraph,
    buf: &mut Buffers,
    src: u32,
    dst: u32,
    cost: impl Fn(u32, &Edge) -> f64,
    hop: Option<&FineIndex>,
    max_cost: f64,
) -> Option<Vec<u32>> {
    let run = buf.next_run();
    buf.stamp[src as usize] = run;
    buf.g[src as usize] = 0.0;
    buf.parent[src as usize] = src;
    let mut heap: BinaryHeap<Reverse<(u64, u32)>> = BinaryHeap::new();
    heap.push(Reverse((0f64.to_bits(), src)));
    while let Some(Reverse((db, u))) = heap.pop() {
        let du = f64::from_bits(db);
        if du > buf.g[u as usize] + 1e-12 {
            continue;
        }
        if du > max_cost {
            return None;
        }
        if u == dst {
            let mut path = vec![dst];
            let mut cur = dst;
            while buf.parent[cur as usize] != cur {
                cur = buf.parent[cur as usize];
                path.push(cur);
            }
            path.reverse();
            return Some(path);
        }
        let mut relax = |v: u32, nd: f64, heap: &mut BinaryHeap<Reverse<(u64, u32)>>| {
            let vi = v as usize;
            if buf.stamp[vi] != run || nd < buf.g[vi] {
                buf.stamp[vi] = run;
                buf.g[vi] = nd;
                buf.parent[vi] = u;
                heap.push(Reverse((nd.to_bits(), v)));
            }
        };
        for e in g.edges(u) {
            relax(e.to, du + cost(u, e), &mut heap);
        }
        if let Some(fine) = hop {
            let pu = g.xy[u as usize];
            for v in fine.near(g, pu, HOP_KM) {
                if v != u && g.edge(u, v).is_none() && same_line(g, u, v) {
                    relax(v, du + hav(pu, g.xy[v as usize]) + HOP_COST, &mut heap);
                }
            }
        }
    }
    None
}

/// the nodes within a track distance `max_km` of a snap set, with their distance and the node before (ordered by node number
/// through `reached`)
pub fn reach(g: &RGraph, from: &Snap, max_km: f64) -> FxHashMap<u32, (f64, u32)> {
    let mut out: FxHashMap<u32, (f64, u32)> = FxHashMap::default();
    let mut heap: BinaryHeap<Reverse<(u64, u32)>> = BinaryHeap::new();
    for &(s, d) in from {
        if out.get(&s).is_none_or(|x| d < x.0) {
            out.insert(s, (d, s));
            heap.push(Reverse((d.to_bits(), s)));
        }
    }
    while let Some(Reverse((db, u))) = heap.pop() {
        let du = f64::from_bits(db);
        if du > out[&u].0 + 1e-12 {
            continue;
        }
        for e in g.edges(u) {
            let nd = du + e.km as f64;
            if nd <= max_km && out.get(&e.to).is_none_or(|x| nd < x.0) {
                out.insert(e.to, (nd, u));
                heap.push(Reverse((nd.to_bits(), e.to)));
            }
        }
    }
    out
}

/// the nodes of a `reach` in ascending order
pub fn reached(tree: &FxHashMap<u32, (f64, u32)>) -> Vec<u32> {
    let mut v: Vec<u32> = tree.keys().copied().collect();
    v.sort_unstable();
    v
}

/// the path from the source of a `reach` to `node`, the source first
pub fn reach_path(tree: &FxHashMap<u32, (f64, u32)>, node: u32) -> Vec<u32> {
    let mut path = vec![node];
    let mut cur = node;
    while let Some(&(_, p)) = tree.get(&cur) {
        if p == cur {
            break;
        }
        path.push(p);
        cur = p;
    }
    path.reverse();
    path
}
