//! Multipolygon assembly from member ways (replaces libosmium's area assembler for our areas).
//!
//! Like libosmium: roles are ignored, the geometry decides. Segments are collected from all member ways, duplicate segments are
//! removed in pairs, segments are joined into rings at shared locations (a ring that touches itself is split into two), rings are
//! nested by containment (even depth outer, odd depth inner), outer rings are counter-clockwise and inner rings clockwise.

use crate::cache::MISSING;
use rustc_hash::FxHashMap;

/// (lat, lon) in 1e-7 degrees
pub type P = (i32, i32);
/// a polygon: outer ring, then its inner rings; every ring closed (first point repeated last)
pub type Poly = Vec<Vec<P>>;

#[derive(Debug, PartialEq, Eq)]
pub enum Fail {
    /// a member node has no location: libosmium throws invalid_location and the area is skipped silently
    Location,
    /// the rings do not close or are degenerate: counted as dropped:bad_multipolygon
    Geometry,
}

/// `strict`: reject crossing or overlapping segments, as libosmium does (feature areas). Region outlines are built with
/// `strict` false, since simplified boundaries can cross by a few metres and the point-in-polygon test does not mind.
pub fn assemble(ways: &[&[P]], strict: bool) -> Result<Vec<Poly>, Fail> {
    assemble_gaps(ways, strict, 0.0).map(|r| r.0)
}

/// For region outlines: like `assemble`, but ring ends that do not meet are joined to the nearest other loose end within
/// `max_gap_deg` (a broken or simplified border). Returns the polygons and the length of every gap closed, in degrees.
pub fn assemble_outline(ways: &[&[P]], max_gap_deg: f64) -> Result<(Vec<Poly>, Vec<f64>), Fail> {
    assemble_gaps(ways, false, max_gap_deg)
}

fn assemble_gaps(ways: &[&[P]], strict: bool, max_gap_deg: f64) -> Result<(Vec<Poly>, Vec<f64>), Fail> {
    // ---- segments, normalised so (a, b) and (b, a) compare equal
    let mut segs: Vec<(P, P)> = Vec::new();
    for w in ways {
        let mut prev: Option<P> = None;
        for &p in w.iter() {
            if p.0 == MISSING {
                return Err(Fail::Location);
            }
            if let Some(q) = prev {
                if q != p {
                    segs.push(if q < p { (q, p) } else { (p, q) });
                }
            }
            prev = Some(p);
        }
    }
    segs.sort_unstable();
    // remove duplicates in pairs: an even run vanishes, an odd run leaves one
    let mut uniq: Vec<(P, P)> = Vec::with_capacity(segs.len());
    let mut i = 0;
    while i < segs.len() {
        let mut j = i + 1;
        while j < segs.len() && segs[j] == segs[i] {
            j += 1;
        }
        if (j - i) % 2 == 1 {
            uniq.push(segs[i]);
        }
        i = j;
    }
    if uniq.len() < 3 {
        return Err(Fail::Geometry);
    }
    // like libosmium, any two segments that cross or overlap make the area invalid
    if strict && intersects(&uniq) {
        return Err(Fail::Geometry);
    }

    // ---- adjacency: point -> segments; every point needs an even degree for the rings to close
    let mut adj: FxHashMap<P, Vec<u32>> = FxHashMap::default();
    for (k, &(a, b)) in uniq.iter().enumerate() {
        adj.entry(a).or_default().push(k as u32);
        adj.entry(b).or_default().push(k as u32);
    }
    let mut gaps = Vec::new();
    let odd: Vec<P> = adj.iter().filter(|(_, v)| v.len() % 2 == 1).map(|(p, _)| *p).collect();
    if !odd.is_empty() {
        if max_gap_deg <= 0.0 {
            return Err(Fail::Geometry);
        }
        // pair loose ends, nearest pairs first, and join each pair with a segment
        let dist = |a: P, b: P| -> f64 {
            let k = (a.0 as f64 * 1e-7).to_radians().cos();
            (((a.0 - b.0) as f64 * 1e-7).powi(2) + ((a.1 - b.1) as f64 * 1e-7 * k).powi(2)).sqrt()
        };
        let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
        for i in 0..odd.len() {
            for j in i + 1..odd.len() {
                let d = dist(odd[i], odd[j]);
                if d <= max_gap_deg {
                    pairs.push((d, i, j));
                }
            }
        }
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut done = vec![false; odd.len()];
        for (d, i, j) in pairs {
            if done[i] || done[j] {
                continue;
            }
            done[i] = true;
            done[j] = true;
            let (a, b) = (odd[i], odd[j]);
            let k = uniq.len() as u32;
            uniq.push(if a < b { (a, b) } else { (b, a) });
            adj.get_mut(&a).unwrap().push(k);
            adj.get_mut(&b).unwrap().push(k);
            gaps.push(d);
        }
        if done.iter().any(|d| !d) {
            return Err(Fail::Geometry);
        }
    }

    // ---- trace rings; a walk that comes back to a point already on the path closes a ring there. Where rings touch (a point
    // with more than two segments) the walk takes the sharpest turn the same way every time, so rings touch but never cross.
    let mut used = vec![false; uniq.len()];
    let mut rings: Vec<Vec<P>> = Vec::new();
    let take = |prev: Option<P>, p: P, used: &mut Vec<bool>, adj: &FxHashMap<P, Vec<u32>>| -> Option<P> {
        let other = |k: u32| {
            let (a, b) = uniq[k as usize];
            if a == p {
                b
            } else {
                a
            }
        };
        let mut best: Option<(f64, u32)> = None;
        for &k in &adj[&p] {
            if used[k as usize] {
                continue;
            }
            let Some(q) = prev else {
                best = Some((0.0, k));
                break;
            };
            // angle from the way back (p -> prev) counter-clockwise to the way out (p -> next), in (0, 2 pi]
            let n = other(k);
            let back = ((q.0 - p.0) as f64).atan2((q.1 - p.1) as f64);
            let out = ((n.0 - p.0) as f64).atan2((n.1 - p.1) as f64);
            let mut a = out - back;
            while a <= 0.0 {
                a += std::f64::consts::TAU;
            }
            if best.map_or(true, |(b, _)| a < b) {
                best = Some((a, k));
            }
        }
        let (_, k) = best?;
        used[k as usize] = true;
        Some(other(k))
    };
    for s in 0..uniq.len() {
        if used[s] {
            continue;
        }
        let start = uniq[s].0;
        let mut path: Vec<P> = vec![start];
        let mut pos: FxHashMap<P, usize> = FxHashMap::default();
        pos.insert(start, 0);
        let mut cur = start;
        loop {
            let prev = if path.len() >= 2 { Some(path[path.len() - 2]) } else { None };
            let Some(next) = take(prev, cur, &mut used, &adj) else {
                if path.len() == 1 {
                    break;
                }
                return Err(Fail::Geometry);
            };
            if let Some(&k) = pos.get(&next) {
                let mut ring: Vec<P> = path[k..].to_vec();
                ring.push(next);
                for p in &path[k + 1..] {
                    pos.remove(p);
                }
                path.truncate(k + 1);
                rings.push(ring);
            } else {
                pos.insert(next, path.len());
                path.push(next);
            }
            cur = next;
        }
    }
    rings.retain(|r| r.len() >= 4);
    if rings.is_empty() {
        return Err(Fail::Geometry);
    }

    // ---- nesting by containment, largest rings first
    let area2 = |r: &[P]| -> i128 {
        // twice the signed area in (lon, lat): > 0 counter-clockwise
        r.windows(2).map(|w| w[0].1 as i128 * w[1].0 as i128 - w[1].1 as i128 * w[0].0 as i128).sum()
    };
    let areas: Vec<i128> = rings.iter().map(|r| area2(r)).collect();
    let mut order: Vec<usize> = (0..rings.len()).collect();
    order.sort_by(|&a, &b| areas[b].abs().cmp(&areas[a].abs()).then(a.cmp(&b)));
    let mut depth = vec![0usize; rings.len()];
    let mut parent = vec![usize::MAX; rings.len()];
    for (oi, &r) in order.iter().enumerate() {
        for &q in order[..oi].iter().rev() {
            if ring_contains_ring(&rings[q], &rings[r]) {
                depth[r] = depth[q] + 1;
                parent[r] = q;
                break;
            }
        }
    }
    let mut polys: Vec<Poly> = Vec::new();
    let mut poly_of = vec![usize::MAX; rings.len()];
    for &r in &order {
        let mut ring = rings[r].clone();
        let ccw = areas[r] > 0;
        if depth[r] % 2 == 0 {
            if !ccw {
                ring.reverse();
            }
            poly_of[r] = polys.len();
            polys.push(vec![ring]);
        } else {
            if ccw {
                ring.reverse();
            }
            let p = poly_of[parent[r]];
            polys[p].push(ring);
        }
    }
    Ok((polys, gaps))
}

/// libosmium SegmentList::find_intersections + calculate_intersection: do any two (distinct) segments cross in a point that is
/// not a shared end point, or overlap along a line? Points are (x = lon, y = lat), segments ordered by location (x, then y).
fn intersects(segs: &[(P, P)]) -> bool {
    type V = (i64, i64);
    let v = |p: P| -> V { (p.1 as i64, p.0 as i64) };
    let mut s: Vec<(V, V)> = segs.iter().map(|&(a, b)| {
        let (a, b) = (v(a), v(b));
        if a <= b { (a, b) } else { (b, a) }
    }).collect();
    s.sort_unstable();
    let cross = |a: V, b: V| -> i128 { a.0 as i128 * b.1 as i128 - a.1 as i128 * b.0 as i128 };
    let sub = |a: V, b: V| -> V { (a.0 - b.0, a.1 - b.1) };
    let hit = |(p0, p1): (V, V), (q0, q1): (V, V)| -> bool {
        if (p0 == q0 && p1 == q1) || (p0 == q1 && p1 == q0) {
            return false;
        }
        let pd = sub(p1, p0);
        let d = cross(pd, sub(q1, q0));
        if d != 0 {
            if p0 == q0 || p0 == q1 || p1 == q0 || p1 == q1 {
                return false; // touching at an end point
            }
            let na = (q1.0 - q0.0) as i128 * (p0.1 - q0.1) as i128 - (q1.1 - q0.1) as i128 * (p0.0 - q0.0) as i128;
            let nb = (p1.0 - p0.0) as i128 * (p0.1 - q0.1) as i128 - (p1.1 - p0.1) as i128 * (p0.0 - q0.0) as i128;
            return (d > 0 && na >= 0 && na <= d && nb >= 0 && nb <= d) || (d < 0 && na <= 0 && na >= d && nb <= 0 && nb >= d);
        }
        if cross(pd, sub(q0, p0)) == 0 {
            // collinear: overlapping unless they only meet end to end
            let mut sl = [(p0, 0), (p1, 0), (q0, 1), (q1, 1)];
            sl.sort_by(|a, b| a.0.cmp(&b.0));
            if sl[1].0 == sl[2].0 {
                return false;
            }
            return sl[0].1 != sl[1].1;
        }
        false
    };
    for i in 0..s.len().saturating_sub(1) {
        for j in i + 1..s.len() {
            if s[j].0 .0 > s[i].1 .0 {
                break; // outside the x range of segment i (the list is sorted by first point)
            }
            let y_overlap = s[i].0 .1.min(s[i].1 .1) <= s[j].0 .1.max(s[j].1 .1) && s[j].0 .1.min(s[j].1 .1) <= s[i].0 .1.max(s[i].1 .1);
            if y_overlap && hit(s[i], s[j]) {
                return true;
            }
        }
    }
    false
}

/// Is ring `inner` inside ring `outer`? Decided by the first vertex of `inner` that is not on `outer`'s boundary.
fn ring_contains_ring(outer: &[P], inner: &[P]) -> bool {
    let (mut a0, mut o0, mut a1, mut o1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for p in outer {
        a0 = a0.min(p.0);
        o0 = o0.min(p.1);
        a1 = a1.max(p.0);
        o1 = o1.max(p.1);
    }
    for p in inner {
        if p.0 < a0 || p.0 > a1 || p.1 < o0 || p.1 > o1 {
            return false;
        }
    }
    for &p in inner {
        if let Some(r) = point_in_ring(outer, p) {
            return r;
        }
    }
    false
}

/// even-odd test; None when the point lies on the ring
pub fn point_in_ring(r: &[P], p: P) -> Option<bool> {
    let (px, py) = (p.1 as i64, p.0 as i64);
    let mut inside = false;
    for w in r.windows(2) {
        let (x1, y1, x2, y2) = (w[0].1 as i64, w[0].0 as i64, w[1].1 as i64, w[1].0 as i64);
        // on the segment?
        let cross = (x2 - x1) as i128 * (py - y1) as i128 - (y2 - y1) as i128 * (px - x1) as i128;
        if cross == 0 && px >= x1.min(x2) && px <= x1.max(x2) && py >= y1.min(y2) && py <= y1.max(y2) {
            return None;
        }
        if (y1 > py) != (y2 > py) {
            let lhs = (x2 - x1) as i128 * (py - y1) as i128;
            let rhs = (px - x1) as i128 * (y2 - y1) as i128;
            if if y2 > y1 { lhs > rhs } else { lhs < rhs } {
                inside = !inside;
            }
        }
    }
    Some(inside)
}
