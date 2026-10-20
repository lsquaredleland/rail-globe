//! Passenger evidence for unrouted main-line track: passenger stations and routes beside it, freight by evidence (docs/13, docs/15).

use super::*;

/// Which `targets` have a passenger station (`pax`, points as (lat, lon) in 1e-7 degrees) within PAX_NEAR_M of one of their segments.
pub(super) fn near_station(tracks: &[TrackIn], targets: &[usize], pax: &[(i32, i32)]) -> FxHashSet<usize> {
    use rayon::prelude::*;
    let tree = rstar::RTree::bulk_load(pax.iter().filter(|p| p.0 != i32::MIN).map(|p| [p.1 as f64 / 1e7, p.0 as f64 / 1e7]).collect::<Vec<_>>());
    targets
        .par_iter()
        .filter(|&&i| {
            let pts: Vec<[f64; 2]> = tracks[i].ll.iter().filter(|p| p.0 != i32::MIN).map(|p| [p.1 as f64 / 1e7, p.0 as f64 / 1e7]).collect();
            pts.windows(2).any(|w| {
                let k = ((w[0][1] + w[1][1]) / 2.0).to_radians().cos().max(0.01);
                let (dy, dx) = (PAX_NEAR_M / 110_540.0, PAX_NEAR_M / (111_320.0 * k));
                let (lo, hi) = ([w[0][0].min(w[1][0]) - dx, w[0][1].min(w[1][1]) - dy], [w[0][0].max(w[1][0]) + dx, w[0][1].max(w[1][1]) + dy]);
                tree.locate_in_envelope(rstar::AABB::from_corners(lo, hi)).any(|s| {
                    let xy = |q: [f64; 2]| [(q[0] - s[0]) * 111_320.0 * k, (q[1] - s[1]) * 110_540.0];
                    let (a, b) = (xy(w[0]), xy(w[1]));
                    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                    let len2 = dx * dx + dy * dy;
                    let t = if len2 == 0.0 { 0.0 } else { ((-a[0] * dx - a[1] * dy) / len2).clamp(0.0, 1.0) };
                    (a[0] + t * dx).hypot(a[1] + t * dy) <= PAX_NEAR_M
                })
            })
        })
        .copied()
        .collect()
}

pub(super) const BESIDE_M: f64 = 25.0; // track this close to passenger-routed track for BESIDE_SHARE of its length is part of the same passenger corridor
pub(super) const BESIDE_SHARE: f64 = 0.5;

pub(super) struct RSeg {
    pub(super) a: [f64; 2],
    pub(super) b: [f64; 2],
}
impl rstar::RTreeObject for RSeg {
    type Envelope = rstar::AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        rstar::AABB::from_corners(self.a, self.b)
    }
}

/// Which `targets` (track indices) run beside track that is on a passenger route: second tracks, crossovers and connectors that no
/// route relation lists. A way is beside when at least BESIDE_SHARE of its length (sampled about every 20 m) lies within BESIDE_M of a
/// routed way of `routed` (indices). Distances are metres on a local plane.
pub(super) fn beside_routed(tracks: &[TrackIn], routed: &[usize], targets: &[usize]) -> FxHashSet<usize> {
    use rayon::prelude::*;
    let deg = |p: &(i32, i32)| [p.1 as f64 / 1e7, p.0 as f64 / 1e7]; // (lon, lat)
    let segs: Vec<RSeg> = routed
        .iter()
        .flat_map(|&i| {
            let pts: Vec<[f64; 2]> = tracks[i].ll.iter().filter(|p| p.0 != i32::MIN).map(deg).collect();
            pts.windows(2).map(|w| RSeg { a: w[0], b: w[1] }).collect::<Vec<_>>()
        })
        .collect();
    let tree = rstar::RTree::bulk_load(segs);
    let near = |p: [f64; 2]| -> bool {
        let k = p[1].to_radians().cos().max(0.01);
        let (dy, dx) = (BESIDE_M / 110_540.0, BESIDE_M / (111_320.0 * k));
        tree.locate_in_envelope_intersecting(rstar::AABB::from_corners([p[0] - dx, p[1] - dy], [p[0] + dx, p[1] + dy])).any(|s| {
            let xy = |q: [f64; 2]| [(q[0] - p[0]) * 111_320.0 * k, (q[1] - p[1]) * 110_540.0];
            let (a, b) = (xy(s.a), xy(s.b));
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len2 = dx * dx + dy * dy;
            let t = if len2 == 0.0 { 0.0 } else { ((-a[0] * dx - a[1] * dy) / len2).clamp(0.0, 1.0) };
            (a[0] + t * dx).hypot(a[1] + t * dy) <= BESIDE_M
        })
    };
    targets
        .par_iter()
        .filter(|&&i| {
            let pts: Vec<[f64; 2]> = tracks[i].ll.iter().filter(|p| p.0 != i32::MIN).map(deg).collect();
            let (mut n, mut hit) = (0.0f64, 0.0f64); // metres sampled, metres beside
            for w in pts.windows(2) {
                let k = ((w[0][1] + w[1][1]) / 2.0).to_radians().cos();
                let m = ((w[1][0] - w[0][0]) * 111_320.0 * k).hypot((w[1][1] - w[0][1]) * 110_540.0);
                let steps = ((m / 20.0).ceil() as u32).max(1);
                for s in 0..steps {
                    let f = (s as f64 + 0.5) / steps as f64;
                    n += m / steps as f64;
                    if near([w[0][0] + (w[1][0] - w[0][0]) * f, w[0][1] + (w[1][1] - w[0][1]) * f]) {
                        hit += m / steps as f64;
                    }
                }
            }
            n > 0.0 && hit >= BESIDE_SHARE * n
        })
        .copied()
        .collect()
}

pub(super) const PAX_NEAR_M: f64 = 200.0; // a passenger station this close to a track (to the line between its points, not just its points) serves it
pub(super) const SHORT_SPUR_KM: f64 = 3.0;
pub(super) const MAX_FREIGHT_BRANCH_KM: f64 = 15.0; // a longer branch with no station is as likely a passenger line whose stations are not mapped (or not near)
pub(super) const JUNCTION_CLEAR_M: f64 = 800.0; // a station this close to where the railway joins another track belongs to that track

/// (lon, lat) in degrees of a (lat, lon) point in 1e-7 degrees
fn deg(p: (i32, i32)) -> (f64, f64) {
    (p.1 as f64 / 1e7, p.0 as f64 / 1e7)
}

/// metres on a plane around `lat0`
fn xy(p: (f64, f64), lat0: f64) -> (f64, f64) {
    ((p.0) * 111_320.0 * lat0.to_radians().cos(), p.1 * 110_540.0)
}

/// the 0.01-degree grid cell of a (lon, lat) point
fn cell(p: (f64, f64)) -> (i32, i32) {
    ((p.1 / 0.01).floor() as i32, (p.0 / 0.01).floor() as i32)
}

fn dist_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let l0 = (a.1 + b.1) / 2.0;
    let (ax, ay) = xy(a, l0);
    let (bx, by) = xy(b, l0);
    ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt()
}

/// metres from point `p` to the segment a-b
fn seg_dist_m(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (px, py) = xy(p, p.1);
    let (ax, ay) = xy(a, p.1);
    let (bx, by) = xy(b, p.1);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0) };
    ((px - (ax + t * dx)).powi(2) + (py - (ay + t * dy)).powi(2)).sqrt()
}

fn len_km(ll: &[(i32, i32)]) -> f64 {
    let pts: Vec<(f64, f64)> = ll.iter().filter(|p| p.0 != i32::MIN).map(|&p| deg(p)).collect();
    crate::tiles::haversine_len(&pts)
}

/// Each station belongs to the track nearest to it (not a service track: yards and sidings crowd the stations), within PAX_NEAR_M: a
/// branch that runs beside a main line does not get the main line's stations. Returns track -> the stations that belong to it.
fn station_owners(tracks: &[TrackIn], stations: &[(f64, f64)]) -> FxHashMap<usize, Vec<usize>> {
    let mut grid: FxHashMap<(i32, i32), Vec<usize>> = FxHashMap::default();
    for (i, &p) in stations.iter().enumerate() {
        grid.entry(cell(p)).or_default().push(i);
    }
    let mut owner: Vec<Option<(f64, usize)>> = vec![None; stations.len()];
    for (ti, t) in tracks.iter().enumerate() {
        if !matches!(t.props.g("railway"), Some("rail" | "narrow_gauge")) || t.props.g("service").is_some_and(|v| SERVICE_TRACK.contains(&v)) {
            continue;
        }
        let pts: Vec<(f64, f64)> = t.ll.iter().filter(|p| p.0 != i32::MIN).map(|&p| deg(p)).collect();
        for w in pts.windows(2) {
            let n = ((dist_m(w[0], w[1]) / 400.0).ceil() as usize).max(1);
            let mut seen: Vec<(i32, i32)> = Vec::new();
            for k in 0..=n {
                let f = k as f64 / n as f64;
                let c = cell((w[0].0 + (w[1].0 - w[0].0) * f, w[0].1 + (w[1].1 - w[0].1) * f));
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let key = (c.0 + dy, c.1 + dx);
                        if seen.contains(&key) {
                            continue;
                        }
                        seen.push(key);
                        for &si in grid.get(&key).into_iter().flatten() {
                            let d = seg_dist_m(stations[si], w[0], w[1]);
                            if d <= PAX_NEAR_M && owner[si].is_none_or(|(bd, _)| d < bd) {
                                owner[si] = Some((d, ti));
                            }
                        }
                    }
                }
            }
        }
    }
    let mut owned: FxHashMap<usize, Vec<usize>> = FxHashMap::default();
    for (si, o) in owner.iter().enumerate() {
        if let Some((_, ti)) = o {
            owned.entry(*ti).or_default().push(si);
        }
    }
    owned
}

/// How many ways end at each point, over all track: an end no other way touches is a dead end.
fn end_counts(tracks: &[TrackIn]) -> FxHashMap<(i32, i32), u32> {
    let mut ends: FxHashMap<(i32, i32), u32> = FxHashMap::default();
    for t in tracks {
        if let (Some(&a), Some(&b)) = (t.ll.first(), t.ll.last()) {
            *ends.entry(a).or_default() += 1;
            *ends.entry(b).or_default() += 1;
        }
    }
    ends
}

/// The railways among the candidates: ways with the same name and ref; unnamed ways without a ref are joined end to end into one railway.
fn railway_groups(tracks: &[TrackIn], cand: &[bool]) -> FxHashMap<String, Vec<usize>> {
    let mut uf = UnionFind::new(tracks.len());
    let anon = |t: &TrackIn| t.props.g("name").is_none_or(str::is_empty) && t.props.g("ref").is_none_or(str::is_empty);
    let mut at: FxHashMap<(i32, i32), usize> = FxHashMap::default();
    for (i, t) in tracks.iter().enumerate() {
        if !cand[i] || !anon(t) {
            continue;
        }
        for e in [t.ll.first(), t.ll.last()].into_iter().flatten() {
            match at.get(e) {
                Some(&j) => {
                    uf.union(i, j);
                }
                None => {
                    at.insert(*e, i);
                }
            }
        }
    }
    let mut groups: FxHashMap<String, Vec<usize>> = FxHashMap::default();
    for (i, t) in tracks.iter().enumerate() {
        if !cand[i] {
            continue;
        }
        let key = if anon(t) { format!("a:{}", uf.find(i)) } else { format!("n:{}|{}", t.props.g("name").unwrap_or(""), t.props.g("ref").unwrap_or("")) };
        groups.entry(key).or_default().push(i);
    }
    groups
}

/// Why one railway (its ways) is freight, if it is: "no-station" or "short-spur".
fn judge_railway(
    tracks: &[TrackIn],
    members: &[usize],
    ends: &FxHashMap<(i32, i32), u32>,
    stations: &[(f64, f64)],
    owned: &FxHashMap<usize, Vec<usize>>,
) -> Option<&'static str> {
    // the ends of the railway as a whole: a dead end (a terminus), or a junction with another track (a station there belongs to the line
    // it joins: a freight branch starts in a passenger station's yard)
    let mut own_ends: FxHashMap<(i32, i32), u32> = FxHashMap::default();
    for &i in members {
        for e in [tracks[i].ll.first(), tracks[i].ll.last()].into_iter().flatten() {
            *own_ends.entry(*e).or_default() += 1;
        }
    }
    let chain_ends: Vec<((i32, i32), bool)> =
        own_ends.iter().filter(|(e, n)| e.0 != i32::MIN && **n == 1).map(|(e, _)| (*e, ends.get(e).copied().unwrap_or(0) > 1)).collect();
    let junctions: Vec<(f64, f64)> = chain_ends.iter().filter(|c| c.1).map(|c| deg(c.0)).collect();
    let group_stations: Vec<usize> = members.iter().flat_map(|i| owned.get(i).into_iter().flatten().copied()).collect();
    let served = group_stations.iter().any(|&si| junctions.iter().all(|&j| dist_m(stations[si], j) > JUNCTION_CLEAR_M));
    let dead_ends: Vec<(f64, f64)> = chain_ends.iter().filter(|c| !c.1).map(|c| deg(c.0)).collect();
    // Only a branch is judged: it has a free end (a dead end). A through piece with no station of its own, joined to track at both ends,
    // is most likely a stretch of a passenger line whose stations the neighbouring ways own.
    if dead_ends.is_empty() {
        None
    } else if !served && members.iter().map(|&i| len_km(&tracks[i].ll)).sum::<f64>() < MAX_FREIGHT_BRANCH_KM {
        Some("no-station")
    } else {
        // a short spur whose dead end has no station of its own
        let km: f64 = members.iter().map(|&i| len_km(&tracks[i].ll)).sum();
        let end_served = |d: (f64, f64)| group_stations.iter().any(|&si| dist_m(stations[si], d) <= PAX_NEAR_M * 3.0);
        (km < SHORT_SPUR_KM && dead_ends.iter().any(|&d| !end_served(d))).then_some("short-spur")
    }
}

/// Unrouted main-line and branch track that no passenger train can use, found from what is (not) beside it. Returns the track indices to
/// type as Freight, with the reason. A railway (its ways with the same name and ref; unnamed ways joined end to end) is freight when
/// (1) it is a branch (it has a dead end) that no passenger station serves, or (2) it is a short spur (under SHORT_SPUR_KM) with a dead end that has no passenger station
/// beside it. A station serves the railway of the track nearest to it (within PAX_NEAR_M), so a branch beside a main line does not get the
/// main line's stations. Only ways that would otherwise
/// be Intercity or Regional from their own tags, with no passenger route and no passenger traffic tag, are looked at.
pub(super) fn freight_by_evidence(tracks: &[TrackIn], cand: &[bool], pax: &[(i32, i32)]) -> FxHashMap<usize, &'static str> {
    let stations: Vec<(f64, f64)> = pax.iter().filter(|p| p.0 != i32::MIN).map(|&p| deg(p)).collect();
    let owned = station_owners(tracks, &stations);
    let ends = end_counts(tracks);
    let mut out = FxHashMap::default();
    for members in railway_groups(tracks, cand).values() {
        if let Some(r) = judge_railway(tracks, members, &ends, &stations, &owned) {
            for &i in members {
                out.insert(i, r);
            }
        }
    }
    out
}

/// A named railway that is freight nearly everywhere (OSM tags `railway:traffic_mode=freight` on most of its ways, not all) is freight
/// all the way: its untagged, unrouted main-line ways would otherwise show as Intercity fragments of a hidden line (Haoji railway).
/// Name -> (freight ways, ways) among the rail ways with no passenger route.
pub(super) fn freight_ways_by_name(tracks: &[TrackIn], cfg: &Cfg, route_lines: &RouteLines) -> std::collections::HashMap<String, (u32, u32)> {
    let mut by_name: std::collections::HashMap<String, (u32, u32)> = std::collections::HashMap::new();
    for tr in tracks {
        let p = &tr.props;
        if !matches!(p.g("railway"), Some("rail" | "narrow_gauge")) || tr.routes.iter().any(|r| route_lines.type_of(r).is_some()) {
            continue;
        }
        let Some(name) = p.g("name").filter(|n| !n.is_empty()) else { continue };
        let e = by_name.entry(name.to_string()).or_default();
        e.1 += 1;
        if own_tag_type(p, cfg).0 == "freight" {
            e.0 += 1;
        }
    }
    by_name
}

/// Candidates for the evidence test: rail with no passenger route that its own tags make Intercity or Regional, and nothing saying passengers.
pub(super) fn evidence_candidates(tracks: &[TrackIn], cfg: &Cfg, route_lines: &RouteLines) -> Vec<bool> {
    tracks
        .iter()
        .map(|tr| {
            let p = &tr.props;
            matches!(p.g("railway"), Some("rail" | "narrow_gauge"))
                && tr.ll.len() >= 2
                && !tr.routes.iter().any(|r| route_lines.type_of(r).is_some())
                && p.g("service").is_none_or(|v| !SERVICE_TRACK.contains(&v))
                && !matches!(p.g("railway:traffic_mode"), Some("passenger" | "mixed"))
                && p.g("usage") != Some("tourism")
                && !p.g("name").is_some_and(|n| PASSENGER_NAME_WORDS.iter().any(|w| n.contains(w)))   // connectors and loops link passenger lines; the others say passengers
                && matches!(own_tag_type(p, cfg), (ref t, "tags" | "default") if t == "intercity" || t == "regional")
        })
        .collect()
}

/// Passenger needs evidence (config `passenger_needs_evidence`): main-line track typed from its own tags alone (no passenger route, not an
/// exemption) is Freight unless it runs beside passenger-routed track or its operator is a passenger agency. Measured against the BTS
/// passenger inventory in docs/13-freight-north-america.md (US precision 26% -> 95%, recall 98% -> 98% on the real output).
pub(super) fn needs_evidence_pass(tracks: &[TrackIn], cfg: &Cfg, pax: &[(i32, i32)], mode: Evidence, decided: &mut [Option<Decided>]) {
    let passenger = |t: &str| matches!(t, "intercity" | "regional" | "high_speed");
    let routed: Vec<usize> = (0..tracks.len())
        .filter(|&i| {
            matches!(tracks[i].props.g("railway"), Some("rail" | "narrow_gauge"))
                && tracks[i].ll.len() >= 2
                && decided[i].as_ref().is_some_and(|d| d.s == "route" && passenger(&d.t))
        })
        .collect();
    let targets: Vec<usize> = (0..tracks.len())
        .filter(|&i| {
            let p = &tracks[i].props;
            tracks[i].ll.iter().filter(|q| q.0 != i32::MIN).count() >= 2   // a way with no usable geometry keeps its type
                && matches!(p.g("railway"), Some("rail" | "narrow_gauge"))
                && !matches!(p.g("railway:traffic_mode"), Some("passenger" | "mixed"))
                && decided[i].as_ref().is_some_and(|d| matches!(d.s, "tags" | "default") && matches!(d.t.as_str(), "intercity" | "regional"))
                && !p.g("operator").is_some_and(|o| cfg.passenger_operators.iter().any(|r| r.is_match(o)))
        })
        .collect();
    let mut kept = beside_routed(tracks, &routed, &targets);
    if mode == Evidence::Stations {
        kept.extend(near_station(tracks, &targets, pax));
    }
    for ti in targets {
        if !kept.contains(&ti) {
            if let Some(d) = decided[ti].as_mut() {
                d.t = if tracks[ti].props.g("railway") == Some("narrow_gauge") { "other" } else { "freight" }.into(); // no evidence it is freight either (a heritage line tagged usage=branch)
                d.s = "no-evidence";
            }
        }
    }
}
