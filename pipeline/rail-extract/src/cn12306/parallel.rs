//! Parallel ways: a routed leg's trains also run on the ways beside its path, the other track of a double line or the other line of a
//! corridor. A way counts when at least 70% of its length (or of the leg's, if shorter) lies within 70 m of the path and runs
//! within 20 degrees of the path's bearing there. Service track never counts.

use super::graph::{cell, Graph};
use rustc_hash::{FxHashMap, FxHashSet};

/// metres from the path
const BUF_M: f64 = 70.0;
/// degrees between the bearings
const ANGLE: f64 = 20.0;
/// share of the way (of the leg, if shorter) that must lie beside the path
const SHARE: f64 = 0.7;
/// metres per degree of latitude; a degree of longitude is this times the cosine of the leg's mean latitude
const KY: f64 = 110_570.0;
const KX0: f64 = 111_320.0;

/// the segment that has been looked at in this leg
pub struct ParScratch {
    stamp: u32,
    seen: Vec<u32>,
}

impl ParScratch {
    pub fn new(g: &Graph) -> ParScratch {
        ParScratch { stamp: 0, seen: vec![0; g.segs.len()] }
    }
}

fn m(p: (f64, f64), kx: f64) -> (f64, f64) {
    (p.0 * kx, p.1 * KY)
}

/// bearing of a-b in degrees, 0 to 180 (a line has no direction)
fn bearing(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).atan2(b.1 - a.1).to_degrees().rem_euclid(180.0)
}

/// points every 20 m along the path with the bearing of their segment, without the gaps
fn densify(geom: &[(f64, f64)], gaps: &[usize], kx: f64) -> Vec<(f64, f64, f64)> {
    let mut pts = Vec::new();
    for i in 0..geom.len().saturating_sub(1) {
        if gaps.contains(&i) {
            continue;
        }
        let (a, b) = (m(geom[i], kx), m(geom[i + 1], kx));
        let d = (b.0 - a.0).hypot(b.1 - a.1);
        if d == 0.0 {
            continue;
        }
        let br = bearing(a, b);
        let n = ((d / 20.0) as usize).max(1);
        for j in 0..=n {
            let f = j as f64 / n as f64;
            pts.push((a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f, br));
        }
    }
    pts
}

/// way indexes (ascending) of the ways beside a leg's path, the leg's own ways included
pub fn parallel_ways(g: &Graph, sc: &mut ParScratch, geom: &[(f64, f64)], gaps: &[usize], leg_km: f64) -> Vec<u32> {
    let kx = KX0 * (geom.iter().map(|p| p.1).sum::<f64>() / geom.len().max(1) as f64).to_radians().cos();
    let pts = densify(geom, gaps, kx);
    if pts.is_empty() {
        return Vec::new();
    }
    let mut hash: FxHashMap<(i32, i32), Vec<usize>> = FxHashMap::default();
    for (i, p) in pts.iter().enumerate() {
        hash.entry(((p.0 / 100.0).floor() as i32, (p.1 / 100.0).floor() as i32)).or_default().push(i);
    }
    // the bearing of the path point nearest to (x, y) within the buffer
    let near = |x: f64, y: f64| -> Option<f64> {
        let (cx, cy) = ((x / 100.0).floor() as i32, (y / 100.0).floor() as i32);
        let mut best: Option<(f64, f64)> = None;
        for gx in cx - 1..=cx + 1 {
            for gy in cy - 1..=cy + 1 {
                for &i in hash.get(&(gx, gy)).into_iter().flatten() {
                    let d = (pts[i].0 - x).hypot(pts[i].1 - y);
                    if d <= BUF_M && best.is_none_or(|b| d < b.0) {
                        best = Some((d, pts[i].2));
                    }
                }
            }
        }
        best.map(|b| b.1)
    };
    let mut cells: FxHashSet<(i32, i32)> = FxHashSet::default();
    for q in &pts {
        let (cx, cy) = cell(q.0 / kx, q.1 / KY);
        for gx in cx - 1..=cx + 1 {
            for gy in cy - 1..=cy + 1 {
                cells.insert((gx, gy));
            }
        }
    }
    let mut cells: Vec<(i32, i32)> = cells.into_iter().collect();
    cells.sort_unstable();
    sc.stamp += 1;
    let mut inside: FxHashMap<u32, f64> = FxHashMap::default();
    for c in cells {
        for &si in g.seg_grid.get(&c).into_iter().flatten() {
            let (idx, s) = (si as usize, &g.segs[si as usize]);
            if sc.seen[idx] == sc.stamp {
                continue;
            }
            sc.seen[idx] = sc.stamp;
            let (a, b) = (m(g.xy[s.a as usize], kx), m(g.xy[s.b as usize], kx));
            let len = (b.0 - a.0).hypot(b.1 - a.1);
            if len == 0.0 {
                continue;
            }
            let br = bearing(a, b);
            let n = ((len / 25.0) as usize).max(1);
            let beside = (0..=n).all(|j| {
                let f = j as f64 / n as f64;
                near(a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f).is_some_and(|pb| {
                    let d = (pb - br).abs();
                    d.min(180.0 - d) <= ANGLE
                })
            });
            if beside && !g.ways[s.way as usize].service {
                *inside.entry(s.way).or_default() += s.km;
            }
        }
    }
    let mut ways: Vec<u32> = inside.into_iter().filter(|&(w, l)| l >= SHARE * g.ways[w as usize].km.min(leg_km.max(1e-6))).map(|(w, _)| w).collect();
    ways.sort_unstable();
    ways
}
