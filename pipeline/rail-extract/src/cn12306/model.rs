//! Run-time model, calibration and per-train corridor inference of the corridor router (corridor-v3).
//!
//! A candidate path's predicted run time for a class is the sum over its
//! edges of length / min(edge limit, class cap, braking curve), where the
//! braking curve v = sqrt(2 a d) caps speed by the distance d to the
//! nearer end (the train starts and stops there), plus a small fixed
//! overhead. Real timetables carry recovery margin, so the scheduled /
//! predicted ratio is learned per class from legs whose corridor is not
//! in doubt (all candidates predict within a few percent of each other).
//! A leg's evidence for candidate k is the log density of
//! x = ln(scheduled / predicted_k) under that learned distribution, with
//! a hard floor for trains faster than the class has ever been observed
//! to run against the model. Per train, candidates of consecutive legs
//! are linked by a reversal penalty (a train cannot turn back through a
//! station with a short dwell) and forward-backward gives each leg's
//! posterior over its candidates.
use super::graph::NO_LINE;
use super::rgraph::{bearing, hav, Metric, RGraph, F_HSR, P};
use rustc_hash::FxHashMap as HashMap;
use std::collections::BTreeMap;

pub const NCLASS: usize = 7;
pub const CLASS_NAMES: [&str; NCLASS] = ["G", "D", "C", "S", "Z", "T", "K"];
/// Top running speed (km/h) per class.
const CAP: [f64; NCLASS] = [350.0, 250.0, 350.0, 200.0, 160.0, 140.0, 120.0];
/// Classes barred from dedicated high-speed lines.
const CONVENTIONAL: [bool; NCLASS] = [false, false, false, false, true, true, true];
/// Track km on dedicated high-speed line above which a conventional
/// class is infeasible on a candidate (short station-throat overlaps
/// are mapping noise).
const CONV_HSR_KM: f64 = 5.0;
/// Speed (km/h) assumed across an unmapped gap of a bridged path.
const GAP_KMH: f64 = 80.0;

/// The model parameters. corridor-v3 read them from `CV3_*` environment variables to sweep them; here they are fixed, so a build
/// does not depend on the environment.
pub struct Params {
    /// acceleration and braking rate, m/s^2
    pub accel: f64,
    /// fixed minutes per nonstop leg
    pub overhead: f64,
    /// slow-side tail scale in ln-ratio units (0: symmetric, the plain learned density)
    pub tail_tau: f64,
    /// log-prior per ln(length / shortest)
    pub length_prior: f64,
    /// posterior needed to leave the per-pair path
    pub switch_post: f64,
    /// the per-pair path's posterior must be at most this
    pub base_max: f64,
    /// posterior needed to draw a chord pair on track
    pub fill_post: f64,
    /// weight of the demonstrated-corridor prior
    pub service_prior: f64,
}

pub const fn params() -> &'static Params {
    &Params { accel: 0.4, overhead: 1.0, tail_tau: 0.25, length_prior: 3.0, switch_post: 0.9, base_max: 0.05, fill_post: 0.8, service_prior: 2.0 }
}

/// Speed (km/h) reachable `d_km` from a standstill at rate `accel` m/s^2.
fn brake_curve_kmh(d_km: f64, accel: f64) -> f64 {
    3.6 * (2.0 * accel * d_km * 1000.0).sqrt()
}
/// Log-penalty for reversing through a station with a short dwell.
const REVERSAL_PENALTY: f64 = 4.0;
const REVERSAL_DWELL_MIN: i64 = 6;
const INFEASIBLE_LOGLIK: f64 = -30.0;
const DENSITY_FLOOR: f64 = 1e-4;
const KDE_BW: f64 = 0.04;
const GRID_LO: f64 = -0.8;
const GRID_HI: f64 = 2.0;
const GRID_STEP: f64 = 0.005;

/// Routing groups for the demonstrated-corridor prior: classes the
/// operator routes alike (350 EMU, 250 EMU, intercity S, conventional).
pub const NGROUP: usize = 4;

pub fn class_group(c: usize) -> usize {
    match c {
        0 | 2 => 0,
        1 => 1,
        3 => 2,
        _ => 3,
    }
}

/// Demonstrated-corridor log-prior for one leg: `compat[k]` stopping
/// services of the leg's routing group between the same two stations
/// fit candidate k (all their intermediate stops lie on it) out of `n`
/// such services. Laplace-smoothed rate, weighted by `service_prior`.
pub fn service_logprior(compat: &[u32], n: u32) -> Vec<f64> {
    let w = params().service_prior;
    compat.iter().map(|&k| w * ((k as f64 + 1.0) / (n as f64 + 2.0)).ln()).collect()
}

pub fn class_index(class: &str) -> usize {
    match class {
        "G" => 0,
        "D" => 1,
        "C" => 2,
        "S" => 3,
        "Z" => 4,
        "T" => 5,
        _ => 6, // K, Y, plain numbered trains
    }
}

/// Extra minutes a train of class `c` loses by stopping once at an
/// intermediate station (braking + re-accelerating from a typical
/// cruising speed), plus the fixed overhead. Used to turn a stopping
/// A->B->C run into an equivalent nonstop A->C time.
pub fn stop_loss_min(c: usize) -> f64 {
    let p = params();
    let v = CAP[c].min(300.0) / 3.6;
    v / p.accel / 60.0 + p.overhead
}

pub fn class_metric(c: usize) -> Metric {
    Metric::Time { cap: CAP[c], hsr_mult: if CONVENTIONAL[c] { 8.0 } else { 1.0 } }
}

/// A candidate corridor for one unordered station pair, stored in
/// key.0 -> key.1 direction.
#[derive(Clone)]
pub struct Cand {
    pub nodes: Vec<u32>,
    pub km: f64,
    /// Straight-line km of unmapped gaps inside the path (bridged).
    pub gap_km: f64,
    /// Indices into `nodes` where a gap jump starts (node i -> i+1).
    pub gaps_at: Vec<usize>,
    pub hsr_km: f64,
    /// Predicted pure running minutes per class (no overhead).
    pub run_min: [f64; NCLASS],
    pub source: String,
    /// Same corridor as the production geometry for this pair.
    pub is_base: bool,
    /// Initial / final bearings (degrees) over the first / last ~1.5 km.
    pub head_start: f64,
    pub head_end: f64,
    /// Line names by km, longest first (top 3).
    pub lines: Vec<(String, f64)>,
}

impl Cand {
    /// Measure a node path on the graph. Consecutive nodes that are not
    /// adjacent are a gap jump (bridged geometry).
    pub fn measure(g: &RGraph, nodes: Vec<u32>, source: &str) -> Cand {
        let accel = params().accel;
        // pass 1: per-step length, edge (None = gap jump)
        let steps: Vec<(f64, Option<&super::rgraph::Edge>)> = nodes
            .windows(2)
            .map(|w| match g.edge(w[0], w[1]) {
                Some(e) => (e.km as f64, Some(e)),
                None => (hav(g.xy[w[0] as usize], g.xy[w[1] as usize]), None),
            })
            .collect();
        let km: f64 = steps.iter().map(|s| s.0).sum();
        let mut gap_km = 0.0;
        let mut gaps_at = Vec::new();
        let mut hsr_km = 0.0;
        let mut run = [0.0f64; NCLASS];
        let mut by_line: BTreeMap<u32, f64> = BTreeMap::new();
        let mut cum = 0.0;
        for (i, &(d, e)) in steps.iter().enumerate() {
            let mid = cum + d / 2.0;
            cum += d;
            let vb = brake_curve_kmh(mid.min(km - mid).max(0.0), accel).max(20.0);
            let vmax = match e {
                Some(e) => {
                    if e.flags & F_HSR != 0 && e.vmax >= 250 {
                        hsr_km += d;
                    }
                    *by_line.entry(g.line(e)).or_default() += d;
                    e.vmax as f64
                }
                None => {
                    gap_km += d;
                    gaps_at.push(i);
                    GAP_KMH
                }
            };
            for (c, r) in run.iter_mut().enumerate() {
                *r += d / vmax.min(CAP[c]).min(vb) * 60.0;
            }
        }
        let mut lines: Vec<(String, f64)> = by_line.into_iter().filter(|(l, _)| *l != NO_LINE).map(|(l, d)| (g.lines[l as usize].clone(), d)).collect();
        lines.sort_by(|a, b| b.1.total_cmp(&a.1));
        lines.truncate(3);
        let (head_start, head_end) = end_bearings(g, &nodes);
        Cand { nodes, km, gap_km, gaps_at, hsr_km, run_min: run, source: source.to_string(), is_base: false, head_start, head_end, lines }
    }

    /// Predicted scheduled-free minutes for class `c`, or None when the
    /// class cannot use this corridor.
    pub fn predict(&self, c: usize) -> Option<f64> {
        if CONVENTIONAL[c] && self.hsr_km > CONV_HSR_KM {
            return None;
        }
        Some(self.run_min[c] + params().overhead)
    }
}

fn end_bearings(g: &RGraph, nodes: &[u32]) -> (f64, f64) {
    let pt = |i: usize| g.xy[nodes[i] as usize];
    let n = nodes.len();
    if n < 2 {
        return (0.0, 0.0);
    }
    let mut i = 1;
    while i < n - 1 && hav(pt(0), pt(i)) < 1.5 {
        i += 1;
    }
    let mut j = n - 2;
    while j > 0 && hav(pt(j), pt(n - 1)) < 1.5 {
        j -= 1;
    }
    (bearing(pt(0), pt(i)), bearing(pt(j), pt(n - 1)))
}

/// Geometric footprint of a path: points every ~0.3 km plus a grid of
/// all its nodes. OSM maps double track as two parallel ways, so "same
/// corridor" must be judged by proximity, not by shared edges.
pub struct Shape {
    samples: Vec<P>,
    grid: HashMap<(i32, i32), Vec<P>>,
}

const SHAPE_STEP_KM: f64 = 0.3;
const SHAPE_NEAR_KM: f64 = 0.15;
const SHAPE_CELL: f64 = 400.0; // 1/400 degree, ~0.28 km

impl Shape {
    pub fn of(g: &RGraph, k: &Cand) -> Shape {
        let mut samples = Vec::new();
        let mut grid: HashMap<(i32, i32), Vec<P>> = Default::default();
        let mut since = f64::INFINITY;
        let mut prev: Option<P> = None;
        for &n in &k.nodes {
            let p = g.xy[n as usize];
            if let Some(q) = prev {
                since += hav(q, p);
            }
            if since >= SHAPE_STEP_KM {
                samples.push(p);
                since = 0.0;
            }
            grid.entry(((p.0 * SHAPE_CELL) as i32, (p.1 * SHAPE_CELL) as i32)).or_default().push(p);
            prev = Some(p);
        }
        Shape { samples, grid }
    }

    /// Whether any node of this path lies within `km` of `p`.
    pub fn near(&self, p: P, km: f64) -> bool {
        let r = (km * SHAPE_CELL / 111.0).ceil() as i32 + 1;
        let (cx, cy) = ((p.0 * SHAPE_CELL) as i32, (p.1 * SHAPE_CELL) as i32);
        (-r..=r).any(|dx| (-r..=r).any(|dy| self.grid.get(&(cx + dx, cy + dy)).is_some_and(|v| v.iter().any(|q| hav(p, *q) <= km))))
    }

    pub fn samples(&self) -> &[P] {
        &self.samples
    }

    /// Share of this path lying within 150 m of `other` (0..=1).
    pub fn covered_by(&self, other: &Shape) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let near = self
            .samples
            .iter()
            .filter(|p| {
                let (cx, cy) = ((p.0 * SHAPE_CELL) as i32, (p.1 * SHAPE_CELL) as i32);
                (-1..=1).any(|dx| (-1..=1).any(|dy| other.grid.get(&(cx + dx, cy + dy)).is_some_and(|v| v.iter().any(|q| hav(**p, *q) <= SHAPE_NEAR_KM))))
            })
            .count();
        near as f64 / self.samples.len() as f64
    }
}

/// Learned distribution of x = ln(scheduled / predicted) per class.
pub struct Calibration {
    /// Log density on the x grid, per class.
    logpdf: Vec<Vec<f64>>,
    /// Hard lower bound on x per class (faster than ever observed).
    pub x_min: [f64; NCLASS],
    pub n: [usize; NCLASS],
    pub median: [f64; NCLASS],
    pub q05: [f64; NCLASS],
    pub q95: [f64; NCLASS],
    /// Log density at the median, per class (anchor of the slow tail).
    med_logpdf: [f64; NCLASS],
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[i]
}

impl Calibration {
    /// Fit from (class, x) samples; classes with < 200 samples borrow
    /// the pooled distribution.
    pub fn fit(samples: &[(usize, f64)]) -> Calibration {
        let mut per: Vec<Vec<f64>> = vec![Vec::new(); NCLASS];
        for &(c, x) in samples {
            per[c].push(x);
        }
        let mut pooled: Vec<f64> = samples.iter().map(|s| s.1).collect();
        pooled.sort_by(f64::total_cmp);
        let ngrid = ((GRID_HI - GRID_LO) / GRID_STEP) as usize + 1;
        let kde = |xs: &[f64]| -> Vec<f64> {
            // Gaussian KDE evaluated on the grid via binning (fast, exact
            // enough at this bandwidth / step ratio).
            let mut bins = vec![0.0f64; ngrid];
            for &x in xs {
                let i = ((x.clamp(GRID_LO, GRID_HI) - GRID_LO) / GRID_STEP).round() as usize;
                bins[i.min(ngrid - 1)] += 1.0;
            }
            let half = (4.0 * KDE_BW / GRID_STEP) as isize;
            let kern: Vec<f64> = (-half..=half).map(|k| (-0.5 * (k as f64 * GRID_STEP / KDE_BW).powi(2)).exp()).collect();
            let norm = xs.len() as f64 * KDE_BW * (2.0 * std::f64::consts::PI).sqrt();
            (0..ngrid)
                .map(|i| {
                    let mut s = 0.0;
                    for (k, w) in kern.iter().enumerate() {
                        let j = i as isize + k as isize - half;
                        if j >= 0 && (j as usize) < ngrid {
                            s += w * bins[j as usize];
                        }
                    }
                    (s / norm).max(DENSITY_FLOOR).ln()
                })
                .collect()
        };
        let pooled_pdf = kde(&pooled);
        let mut cal = Calibration {
            logpdf: Vec::new(),
            x_min: [0.0; NCLASS],
            n: [0; NCLASS],
            median: [0.0; NCLASS],
            q05: [0.0; NCLASS],
            q95: [0.0; NCLASS],
            med_logpdf: [0.0; NCLASS],
        };
        for (c, xs) in per.iter_mut().enumerate() {
            xs.sort_by(f64::total_cmp);
            cal.n[c] = xs.len();
            let src: &[f64] = if xs.len() >= 200 { xs } else { &pooled };
            cal.logpdf.push(if xs.len() >= 200 { kde(xs) } else { pooled_pdf.clone() });
            cal.x_min[c] = quantile(src, 0.005) - 0.05;
            cal.median[c] = quantile(src, 0.5);
            cal.q05[c] = quantile(src, 0.05);
            cal.q95[c] = quantile(src, 0.95);
            cal.med_logpdf[c] = cal.grid_logpdf(c, cal.median[c]);
        }
        cal
    }

    fn grid_logpdf(&self, c: usize, x: f64) -> f64 {
        let i = ((x.clamp(GRID_LO, GRID_HI) - GRID_LO) / GRID_STEP).round() as usize;
        self.logpdf[c][i.min(self.logpdf[c].len() - 1)]
    }

    /// Learned log density of x; on the slow side of the median it never
    /// falls faster than an exponential tail of scale `tail_tau`
    /// (running slow is weak evidence, running fast is strong).
    pub fn loglik(&self, c: usize, x: f64) -> f64 {
        if x < self.x_min[c] {
            return INFEASIBLE_LOGLIK;
        }
        let base = self.grid_logpdf(c, x);
        let tau = params().tail_tau;
        if tau > 0.0 && x > self.median[c] {
            base.max(self.med_logpdf[c] - (x - self.median[c]) / tau)
        } else {
            base
        }
    }

    pub fn is_feasible(&self, c: usize, x: f64) -> bool {
        x >= self.x_min[c]
    }
}

/// Emission log-score of each candidate for one leg (scheduled minutes
/// `m`, class `c`): learned run-time likelihood + length prior.
pub fn emissions(cal: &Calibration, cands: &[Cand], c: usize, m: f64) -> Vec<f64> {
    let kmin = cands.iter().map(|k| k.km).fold(f64::INFINITY, f64::min);
    cands
        .iter()
        .map(|k| match k.predict(c) {
            Some(p) if p > 0.0 && m > 0.0 => cal.loglik(c, (m / p).ln()) - params().length_prior * (k.km / kmin).ln(),
            _ => INFEASIBLE_LOGLIK * 2.0,
        })
        .collect()
}

/// Heading of a candidate when traversed forward (true) or backward.
fn heading_in(k: &Cand, forward: bool) -> f64 {
    // arriving heading at the leg's destination end
    if forward {
        k.head_end
    } else {
        (k.head_start + 180.0).rem_euclid(360.0)
    }
}

fn heading_out(k: &Cand, forward: bool) -> f64 {
    if forward {
        k.head_start
    } else {
        (k.head_end + 180.0).rem_euclid(360.0)
    }
}

/// One leg of a train as seen by the inference: its candidates (by
/// reference), traversal direction, emissions, and the dwell at the
/// station where it ENDS.
pub struct LegObs<'a> {
    pub cands: &'a [Cand],
    pub forward: bool,
    pub emit: Vec<f64>,
    pub dwell_end: i64,
}

fn log_sum_exp(v: &[f64]) -> f64 {
    let m = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if m == f64::NEG_INFINITY {
        return m;
    }
    m + v.iter().map(|x| (x - m).exp()).sum::<f64>().ln()
}

fn transition(a: &LegObs, ka: usize, b: &LegObs, kb: usize) -> f64 {
    if a.dwell_end >= REVERSAL_DWELL_MIN {
        return 0.0;
    }
    let hin = heading_in(&a.cands[ka], a.forward);
    let hout = heading_out(&b.cands[kb], b.forward);
    let diff = (hin - hout).rem_euclid(360.0);
    let diff = diff.min(360.0 - diff);
    if diff > 150.0 {
        -REVERSAL_PENALTY
    } else {
        0.0
    }
}

/// Forward-backward over a chain of consecutive legs; returns each leg's
/// posterior probability per candidate.
pub fn posteriors(chain: &[LegObs]) -> Vec<Vec<f64>> {
    let n = chain.len();
    if n == 0 {
        return Vec::new();
    }
    let mut fwd: Vec<Vec<f64>> = Vec::with_capacity(n);
    fwd.push(chain[0].emit.clone());
    for i in 1..n {
        let row: Vec<f64> = (0..chain[i].emit.len())
            .map(|kb| {
                let terms: Vec<f64> = (0..chain[i - 1].emit.len()).map(|ka| fwd[i - 1][ka] + transition(&chain[i - 1], ka, &chain[i], kb)).collect();
                log_sum_exp(&terms) + chain[i].emit[kb]
            })
            .collect();
        fwd.push(row);
    }
    let mut bwd: Vec<Vec<f64>> = vec![Vec::new(); n];
    bwd[n - 1] = vec![0.0; chain[n - 1].emit.len()];
    for i in (0..n - 1).rev() {
        bwd[i] = (0..chain[i].emit.len())
            .map(|ka| {
                let terms: Vec<f64> =
                    (0..chain[i + 1].emit.len()).map(|kb| transition(&chain[i], ka, &chain[i + 1], kb) + chain[i + 1].emit[kb] + bwd[i + 1][kb]).collect();
                log_sum_exp(&terms)
            })
            .collect();
    }
    (0..n)
        .map(|i| {
            let joint: Vec<f64> = fwd[i].iter().zip(&bwd[i]).map(|(f, b)| f + b).collect();
            let z = log_sum_exp(&joint);
            joint.iter().map(|j| (j - z).exp()).collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(km: f64, run: f64, hs: f64, he: f64) -> Cand {
        Cand {
            nodes: Vec::new(),
            km,
            gap_km: 0.0,
            gaps_at: Vec::new(),
            hsr_km: 0.0,
            run_min: [run; NCLASS],
            source: String::new(),
            is_base: false,
            head_start: hs,
            head_end: he,
            lines: Vec::new(),
        }
    }

    #[test]
    fn posteriors_normalise_and_follow_evidence() {
        let c1 = vec![cand(100.0, 60.0, 90.0, 90.0), cand(110.0, 80.0, 90.0, 90.0)];
        let c2 = vec![cand(50.0, 30.0, 90.0, 90.0)];
        let chain = vec![
            LegObs { cands: &c1, forward: true, emit: vec![-1.0, -5.0], dwell_end: 2 },
            LegObs { cands: &c2, forward: true, emit: vec![-1.0], dwell_end: 0 },
        ];
        let p = posteriors(&chain);
        assert!((p[0].iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert!(p[0][0] > 0.95);
        assert!((p[1][0] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn reversal_is_penalised_on_short_dwell() {
        // leg 1 arrives heading east; leg 2 option 0 leaves west (reversal)
        let c1 = vec![cand(10.0, 10.0, 90.0, 90.0)];
        let c2 = vec![cand(10.0, 10.0, 270.0, 270.0), cand(10.0, 10.0, 90.0, 90.0)];
        let mk = |dwell| {
            vec![
                LegObs { cands: &c1, forward: true, emit: vec![0.0], dwell_end: dwell },
                LegObs { cands: &c2, forward: true, emit: vec![0.0, 0.0], dwell_end: 0 },
            ]
        };
        let short = posteriors(&mk(2));
        assert!(short[1][1] > 0.95);
        let long = posteriors(&mk(10));
        assert!((long[1][0] - 0.5).abs() < 1e-9);
    }

    #[test]
    fn conventional_barred_from_hsr() {
        let mut k = cand(300.0, 80.0, 0.0, 0.0);
        k.hsr_km = 250.0;
        assert!(k.predict(class_index("G")).is_some());
        assert!(k.predict(class_index("Z")).is_none());
    }
}
