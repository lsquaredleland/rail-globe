//! What the app draws for a stretch: its ways merged into a few simplified lines (below zoom 9), with the track's own rail type;
//! from zoom 9 the ways themselves, by id, in the tiles.

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub struct WayGeo {
    pub ty: String,
    pub coords: Vec<(f64, f64)>,
    pub km: f64,
}

#[derive(Deserialize)]
struct RawFeature {
    properties: RawProps,
    geometry: RawGeom,
}

#[derive(Deserialize)]
struct RawProps {
    osm_id: Option<String>,
    #[serde(rename = "type")]
    ty: Option<String>,
}

#[derive(Deserialize)]
struct RawGeom {
    coordinates: Vec<[f64; 2]>,
}

/// the rail type and coordinates of the wanted ways, from the typed cut (features.typed.geojsonl)
pub fn read_ways(typed: &Path, wanted: &FxHashSet<&str>) -> super::R<FxHashMap<String, WayGeo>> {
    let text = super::io(std::fs::read_to_string(typed), "read", typed)?;
    let lines: Vec<&str> = text.lines().filter(|l| l.contains("\"railway\":\"rail\"") && l.contains("\"type\":\"LineString\"")).collect();
    let found = lines
        .par_iter()
        .filter_map(|l| {
            let f: RawFeature = match serde_json::from_str(l) {
                Ok(f) => f,
                Err(e) => return Some(Err(format!("{}: {e}", typed.display()))),
            };
            let id = f.properties.osm_id?;
            wanted.contains(id.as_str()).then(|| {
                let coords: Vec<(f64, f64)> = f.geometry.coordinates.iter().map(|c| (c[0], c[1])).collect();
                let km = coords.windows(2).map(|w| super::graph::hav(w[0].0, w[0].1, w[1].0, w[1].1)).sum();
                Ok((id, WayGeo { ty: f.properties.ty.unwrap_or_else(|| "other".into()), coords, km }))
            })
        })
        .collect::<super::R<Vec<_>>>()?;
    Ok(found.into_iter().collect())
}

/// the rail type of a stretch: the type with the most km of its ways. Yard and freight track only counts when nothing else is
/// there (a yard lead or a freight spur on the way is not what the stretch is)
pub fn stretch_type<'a>(ways: impl Iterator<Item = &'a WayGeo>) -> String {
    let mut km: BTreeMap<&str, f64> = BTreeMap::new();
    for w in ways {
        *km.entry(w.ty.as_str()).or_default() += w.km;
    }
    let pick = |skip_minor: bool| {
        km.iter()
            .filter(|(t, _)| !(skip_minor && matches!(**t, "yard" | "freight")))
            .max_by(|a, b| a.1.total_cmp(b.1).then(b.0.cmp(a.0)))
            .map(|(t, _)| t.to_string())
    };
    pick(true).or_else(|| pick(false)).unwrap_or_else(|| "other".to_string())
}

/// Douglas-Peucker: the points to keep of a line within `tol` degrees
fn dp(pts: &[(f64, f64)], tol: f64) -> Vec<(f64, f64)> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    *keep.last_mut().unwrap() = true;
    let mut stack = vec![(0, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (ax, ay) = pts[a];
        let (dx, dy) = (pts[b].0 - ax, pts[b].1 - ay);
        let l2 = dx * dx + dy * dy;
        let (mut md, mut mi) = (-1.0, 0);
        for (i, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let d = if l2 == 0.0 {
                (p.0 - ax).hypot(p.1 - ay)
            } else {
                let t = (((p.0 - ax) * dx + (p.1 - ay) * dy) / l2).clamp(0.0, 1.0);
                (p.0 - ax - t * dx).hypot(p.1 - ay - t * dy)
            };
            if d > md {
                md = d;
                mi = i;
            }
        }
        if md > tol {
            keep[mi] = true;
            stack.push((a, mi));
            stack.push((mi, b));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

type P = (i64, i64);

/// the lines simplified within `tol` degrees and rounded to `dec` decimals, shared segments once, chains joined where only two meet.
/// Coordinates come out as integers in units of 10^-dec degrees.
pub fn simplify(lines: &[&[(f64, f64)]], tol: f64, dec: i32) -> Vec<Vec<P>> {
    let scale = 10f64.powi(dec);
    let r = |p: (f64, f64)| -> P { ((p.0 * scale).round() as i64, (p.1 * scale).round() as i64) };
    let mut seg: BTreeSet<(P, P)> = BTreeSet::new();
    for ln in lines {
        let s: Vec<P> = {
            let mut v: Vec<P> = Vec::new();
            for p in dp(ln, tol) {
                let q = r(p);
                if v.last() != Some(&q) {
                    v.push(q);
                }
            }
            v
        };
        for w in s.windows(2) {
            seg.insert(if w[0] < w[1] { (w[0], w[1]) } else { (w[1], w[0]) });
        }
    }
    let mut nb: BTreeMap<P, BTreeSet<P>> = BTreeMap::new();
    for &(p, q) in &seg {
        nb.entry(p).or_default().insert(q);
        nb.entry(q).or_default().insert(p);
    }
    let key = |a: P, b: P| if a < b { (a, b) } else { (b, a) };
    let mut used: BTreeSet<(P, P)> = BTreeSet::new();
    let mut out: Vec<Vec<P>> = Vec::new();
    let walk = |a: P, b: P, used: &mut BTreeSet<(P, P)>| -> Vec<P> {
        let mut ln = vec![a, b];
        used.insert(key(a, b));
        loop {
            let last = *ln.last().unwrap();
            if nb[&last].len() != 2 {
                break;
            }
            let Some(n) = nb[&last].iter().copied().find(|&x| !used.contains(&key(last, x))) else { break };
            used.insert(key(last, n));
            ln.push(n);
        }
        ln
    };
    for (&p, ns) in &nb {
        if ns.len() != 2 {
            for &q in ns {
                if !used.contains(&key(p, q)) {
                    out.push(walk(p, q, &mut used));
                }
            }
        }
    }
    for &(p, q) in &seg {
        if !used.contains(&(p, q)) {
            out.push(walk(p, q, &mut used));
        }
    }
    out.into_iter()
        .map(|ln| {
            let f: Vec<(f64, f64)> = ln.iter().map(|&(x, y)| (x as f64 / scale, y as f64 / scale)).collect();
            dp(&f, tol).into_iter().map(r).collect()
        })
        .collect()
}
