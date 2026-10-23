//! The leave-one-out self-check of the corridor router (corridor-v3): hide an intermediate stop B of a stopping train A -> B -> C, ask
//! the model which A-C corridor the train took, and compare with the corridor that passes B. Only tests where some candidate passes B
//! (within 1 km) and another misses it by more than 5 km count. The train's own span is taken out of the service evidence.
//!
//! Reported: the share of tests the stage-1 path gets right (per-pair routing), the shortest path, the model's top choice, what the
//! router ships (the stage-1 path plus the decisive switches), and the precision of the switches.

use super::corridor::{dist_to_path, SvcCounts, SVC_STOP_KM};
use super::model::{self, class_group, class_index, Calibration, Cand, Shape, CLASS_NAMES, NCLASS};
use super::rgraph::RGraph;
use super::stations::{self, TrainRun};
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use serde_json::{json, Value};

pub struct Ctx<'a> {
    pub g: &'a RGraph,
    pub cands: &'a [Vec<Cand>],
    pub shapes: &'a [Vec<Shape>],
    pub svc: &'a [SvcCounts],
    pub cal: &'a Calibration,
    pub runs: &'a [TrainRun],
    pub keys: &'a FxHashMap<stations::Key, usize>,
}

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

struct T {
    detail: Option<Value>,
    model_ok: bool,
    model_switch: Option<bool>,
    base_ok: Option<bool>,
    short_ok: bool,
    class: usize,
}

fn tests(c: &Ctx) -> Vec<T> {
    let params = model::params();
    c.runs
        .par_iter()
        .flat_map_iter(|t| {
            let cl = class_index(&t.class);
            let mut out = Vec::new();
            for j in 0..t.stops.len().saturating_sub(2) {
                let (a, b, cc) = (&t.stops[j], &t.stops[j + 1], &t.stops[j + 2]);
                let (Some(pa), Some(bp), Some(pc)) = (a.pt, b.pt, cc.pt) else { continue };
                if a.name == b.name || b.name == cc.name || a.name == cc.name {
                    continue;
                }
                let Some(&pi) = c.keys.get(&stations::pair_key(&a.name, pa, &cc.name, pc).0) else { continue };
                let pcs = &c.cands[pi];
                if pcs.len() < 2 {
                    continue;
                }
                let (Some(ta), Some(tb), Some(tc)) = (a.times, b.times, cc.times) else { continue };
                let m = (tc.0 - ta.1) as f64 - (tb.1 - tb.0) as f64 - model::stop_loss_min(cl);
                if m <= 0.0 {
                    continue;
                }
                let d: Vec<f64> = pcs.iter().map(|k| dist_to_path(c.g, k, bp)).collect();
                let pass: Vec<bool> = d.iter().map(|x| *x <= 1.0).collect();
                if !pass.iter().any(|x| *x) || !d.iter().any(|x| *x > 5.0) {
                    continue;
                }
                // leave one out: remove this train's own A-[B]-C span from the evidence
                let gidx = class_group(cl);
                let own: Vec<bool> = c.shapes[pi].iter().map(|s| s.near(bp, SVC_STOP_KM)).collect();
                let counted = own.iter().any(|x| *x);
                let compat: Vec<u32> = c.svc[pi].0.iter().zip(&own).map(|(x, o)| x[gidx] - u32::from(*o && x[gidx] > 0)).collect();
                let n = c.svc[pi].1[gidx] - u32::from(counted && c.svc[pi].1[gidx] > 0);
                let sp = model::service_logprior(&compat, n);
                let emit: Vec<f64> = model::emissions(c.cal, pcs, cl, m).iter().zip(&sp).map(|(e, p)| e + p).collect();
                let z = emit.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let post: Vec<f64> = {
                    let e: Vec<f64> = emit.iter().map(|x| (x - z).exp()).collect();
                    let s: f64 = e.iter().sum();
                    e.iter().map(|x| x / s).collect()
                };
                let best = (0..pcs.len()).max_by(|&x, &y| post[x].total_cmp(&post[y])).unwrap();
                let base = pcs.iter().position(|k| k.is_base);
                let shortest = (0..pcs.len()).min_by(|&x, &y| pcs[x].km.total_cmp(&pcs[y].km)).unwrap();
                let switch = base.and_then(|bi| (best != bi && post[best] >= params.switch_post && post[bi] <= params.base_max).then_some(pass[best]));
                let detail = (switch == Some(false)).then(|| {
                    let kd = |i: usize| {
                        json!({"source": pcs[i].source, "km": r1(pcs[i].km), "pred": pcs[i].predict(cl).map(r1),
                               "post": (post[i] * 1000.0).round() / 1000.0, "d_stop_km": r1(d[i]),
                               "lines": pcs[i].lines.iter().map(|x| &x.0).collect::<Vec<_>>()})
                    };
                    json!({"code": t.code, "a": a.name, "hidden_stop": b.name, "c": cc.name, "minutes": m,
                           "chosen": kd(best), "production": base.map(kd), "all": (0..pcs.len()).map(kd).collect::<Vec<_>>()})
                });
                out.push(T { detail, model_ok: pass[best], model_switch: switch, base_ok: base.map(|bi| pass[bi]), short_ok: pass[shortest], class: cl });
            }
            out.into_iter()
        })
        .collect()
}

/// the report, and the wrong switches as JSON lines
pub fn validate(c: &Ctx) -> (Value, String) {
    let tests = tests(c);
    let wrong: String = tests.iter().filter_map(|t| t.detail.as_ref()).map(|d| d.to_string() + "\n").collect();
    let n = tests.len().max(1) as f64;
    let pct = |k: usize, of: f64| (k as f64 / of.max(1.0) * 1000.0).round() / 10.0;
    let with_base: Vec<&T> = tests.iter().filter(|t| t.base_ok.is_some()).collect();
    let nb = with_base.len() as f64;
    let base_ok = with_base.iter().filter(|t| t.base_ok == Some(true)).count();
    // the stage-1 path plus the decisive switches: what the router ships
    let shipped = with_base.iter().filter(|t| t.model_switch.unwrap_or(t.base_ok == Some(true))).count();
    let switches = with_base.iter().filter(|t| t.model_switch.is_some()).count();
    let switch_ok = with_base.iter().filter(|t| t.model_switch == Some(true)).count();
    let per_class: Vec<Value> = (0..NCLASS)
        .map(|cl| {
            let ts: Vec<&T> = tests.iter().filter(|t| t.class == cl).collect();
            let k = ts.len() as f64;
            json!({
                "class": CLASS_NAMES[cl], "n": ts.len(),
                "model_pct": pct(ts.iter().filter(|t| t.model_ok).count(), k),
                "production_pct": pct(ts.iter().filter(|t| t.base_ok == Some(true)).count(), k),
            })
        })
        .collect();
    let report = json!({
        "tests": tests.len(),
        "with_production_path": with_base.len(),
        "production_pct": pct(base_ok, nb),
        "shortest_pct": pct(tests.iter().filter(|t| t.short_ok).count(), n),
        "model_top_pct": pct(tests.iter().filter(|t| t.model_ok).count(), n),
        "shipped_pct": pct(shipped, nb),
        "switches": switches,
        "switches_correct": switch_ok,
        "switch_precision_pct": pct(switch_ok, switches as f64),
        "per_class": per_class,
    });
    (report, wrong)
}
