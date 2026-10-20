//! Construction dashes cut as geometry, line merging, and the highlight records built from merged ways.

use super::constants::con_band;
use super::props::{ConDashProps, ToMap};
use super::records::{Highlight, HlGeom};
use super::util::feat;
use crate::geomx::round;
use geo::{Geometry, LineString, Simplify};
use rustc_hash::FxHashMap;
use serde_json::{json, Value};

// The track is merged end to end (per type, proposed flag and speed band), parallel tracks made one line, and cut into real dashes: MapLibre's
// line-dasharray restarts at tile edges and stretches between whole zooms, so the dashes are geometry. One copy per band of `con_band.step` zoom
// levels from `con_band.first` (shared/constants.json); the dash is CON_DASH_PX / CON_GAP_PX on screen at the middle of its band (within a band the
// map scales it by up to 2^step either way). The app shows one band at a time (property `b`).
const CON_DASH_PX: f64 = 12.0;
const CON_GAP_PX: f64 = 8.0;
const HL_SIMPLIFY: f64 = 0.001; // degrees, about 100 m: under a pixel at z7

/// shapely's linemerge: pieces joined end to end through points where exactly two of them meet
/// web mercator on the unit square (x east, y south) and back
pub(super) fn to_merc((lon, lat): (f64, f64)) -> (f64, f64) {
    ((lon + 180.0) / 360.0, 0.5 - (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0).tan().ln() / (2.0 * std::f64::consts::PI))
}
pub(super) fn from_merc((x, y): (f64, f64)) -> (f64, f64) {
    (x * 360.0 - 180.0, (2.0 * ((0.5 - y) * 2.0 * std::f64::consts::PI).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees())
}

/// cut a line (mercator) into dashes of `dash` with `gap` between, the pattern running along the whole line from its start
pub(super) fn cut_dashes(line: &[(f64, f64)], dash: f64, gap: f64) -> Vec<Vec<(f64, f64)>> {
    let mut out = Vec::new();
    let (mut cur, mut on, mut left) = (vec![line[0]], true, dash);
    for w in line.windows(2) {
        let (mut a, b) = (w[0], w[1]);
        let mut seg = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        while seg > left {
            let t = left / seg;
            let p = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            if on {
                cur.push(p);
                out.push(std::mem::take(&mut cur));
            } else {
                cur = vec![p];
            }
            seg -= left;
            a = p;
            on = !on;
            left = if on { dash } else { gap };
        }
        left -= seg;
        if on {
            cur.push(b);
        }
    }
    if on && cur.len() > 1 {
        out.push(cur);
    }
    out
}

/// Parallel tracks (double track is mapped as two ways) get the same dashes only if they are one line: each gets its own pattern from its own start, so
/// two dashed lines a few metres apart interleave into a solid one. Lines are taken longest first; a stretch within about CON_PARALLEL_CELL of one already
/// taken is dropped, so each corridor keeps one centreline. Input and output in mercator.
const CON_PARALLEL_CELL: f64 = 1.8e-6; // about 60-80 m
pub(super) fn dedupe_parallel(mut lines: Vec<Vec<(f64, f64)>>) -> Vec<Vec<(f64, f64)>> {
    let len = |l: &Vec<(f64, f64)>| l.windows(2).map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()).sum::<f64>();
    lines.sort_by(|a, b| len(b).partial_cmp(&len(a)).unwrap_or(std::cmp::Ordering::Equal));
    let cell = |p: (f64, f64)| ((p.0 / CON_PARALLEL_CELL).floor() as i64, (p.1 / CON_PARALLEL_CELL).floor() as i64);
    let mut taken: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for l in lines {
        // samples along the line, half a cell apart
        let mut s: Vec<(f64, f64)> = vec![l[0]];
        for w in l.windows(2) {
            let d = ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
            let n = (d / (CON_PARALLEL_CELL / 2.0)).ceil().max(1.0) as usize;
            for i in 1..=n {
                let t = i as f64 / n as f64;
                s.push((w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t));
            }
        }
        let covered: Vec<bool> = s
            .iter()
            .map(|&p| {
                let (cx, cy) = cell(p);
                (-1..=1).any(|dx| (-1..=1).any(|dy| taken.contains(&(cx + dx, cy + dy))))
            })
            .collect();
        let mut run: Vec<(f64, f64)> = Vec::new();
        for (i, &p) in s.iter().enumerate() {
            if !covered[i] {
                run.push(p);
            } else if !run.is_empty() {
                if run.len() > 1 {
                    out.push(std::mem::take(&mut run));
                } else {
                    run.clear();
                }
            }
        }
        if run.len() > 1 {
            out.push(run);
        }
        for &p in &s {
            taken.insert(cell(p));
        }
    }
    out
}

/// the dashes of one merged construction line (mercator) for every band (property `b`), each band in the tile zoom where the app shows it.
/// `band` is the speed band's lower bound, -1 for track with no usable maxspeed.
pub(super) fn con_dashes(out: &mut String, ty: &str, prop: bool, band: i64, merc: &[(f64, f64)], first_tile_zoom: i64) {
    if merc.len() < 2 {
        return;
    }
    let bands = con_band();
    let nb = ((bands.last - bands.first) / bands.step).round() as i64;
    for b in 0..=nb {
        let z = bands.first + b as f64 * bands.step;
        let tile_zoom = (z.floor() as i64).min(14);
        if tile_zoom < first_tile_zoom {
            continue;
        }
        let zc = if b == nb { z + bands.step } else { z + bands.step / 2.0 };
        let px = 1.0 / (512.0 * 2f64.powf(zc));
        let p = ConDashProps { ty: ty.to_string(), prop, kmh: (band >= 0).then_some(band), b }.to_map();
        for d in cut_dashes(merc, CON_DASH_PX * px, CON_GAP_PX * px) {
            let coords: Vec<Value> = d
                .iter()
                .map(|&q| {
                    let (x, y) = from_merc(q);
                    json!([x, y])
                })
                .collect();
            feat(out, &p, &json!({"type": "LineString", "coordinates": coords}).to_string(), Some(tile_zoom), Some(tile_zoom));
        }
    }
}

pub(super) fn linemerge(lines: Vec<Vec<(f64, f64)>>) -> Vec<Vec<(f64, f64)>> {
    let key = |p: (f64, f64)| (p.0.to_bits(), p.1.to_bits());
    let mut at: FxHashMap<(u64, u64), Vec<usize>> = FxHashMap::default();
    for (i, l) in lines.iter().enumerate() {
        at.entry(key(l[0])).or_default().push(i);
        at.entry(key(*l.last().unwrap())).or_default().push(i);
    }
    let mut used = vec![false; lines.len()];
    let mut out = Vec::new();
    // walk from every line end that is not a simple pass-through (degree != 2), then close up the remaining rings
    let walk = |start: usize, from_end: bool, used: &mut Vec<bool>| -> Vec<(f64, f64)> {
        let mut path: Vec<(f64, f64)> = Vec::new();
        let (mut i, mut rev) = (start, from_end);
        loop {
            used[i] = true;
            let mut l = lines[i].clone();
            if rev {
                l.reverse();
            }
            if path.is_empty() {
                path.extend(l);
            } else {
                path.extend(l.into_iter().skip(1));
            }
            let end = *path.last().unwrap();
            let next = at.get(&key(end)).filter(|v| v.len() == 2).and_then(|v| v.iter().copied().find(|&j| !used[j]));
            match next {
                Some(j) => {
                    rev = key(lines[j][0]) != key(end);
                    i = j;
                }
                None => break,
            }
        }
        path
    };
    for i in 0..lines.len() {
        if used[i] {
            continue;
        }
        let deg_start = at[&key(lines[i][0])].len();
        let deg_end = at[&key(*lines[i].last().unwrap())].len();
        if deg_start != 2 {
            out.push(walk(i, false, &mut used));
        } else if deg_end != 2 {
            out.push(walk(i, true, &mut used));
        }
    }
    for i in 0..lines.len() {
        if !used[i] {
            out.push(walk(i, false, &mut used));
        }
    }
    out
}

/// highlight data of a line or named railway: its track pieces' ids and a simplified geometry (build_tiles.py hl_record)
pub(super) fn hl_record<'a>(ids: &'a [String], way_geom: &FxHashMap<String, Vec<(f64, f64)>>, ty: Value, oc: Option<String>) -> Highlight<'a> {
    let lines: Vec<Vec<(f64, f64)>> = ids.iter().filter_map(|i| way_geom.get(i)).filter(|g| g.len() >= 2).cloned().collect();
    let geom = (!lines.is_empty()).then(|| {
        let coordinates = linemerge(lines)
            .into_iter()
            .map(|p| {
                let ls: LineString<f64> = p.into_iter().map(|(x, y)| geo::Coord { x, y }).collect();
                let ls = ls.simplify(HL_SIMPLIFY);
                ls.0.iter().map(|c| [round(c.x, 5), round(c.y, 5)]).collect()
            })
            .collect();
        HlGeom { ty: "MultiLineString", coordinates }
    });
    Highlight { ways: ids, geom, ty, oc }
}

pub(super) fn geojson_value(g: &Geometry<f64>) -> Value {
    let ring = |l: &LineString<f64>| Value::Array(l.0.iter().map(|c| json!([Value::from(c.x), Value::from(c.y)])).collect());
    match g {
        Geometry::Polygon(p) => json!({"type": "Polygon", "coordinates": std::iter::once(p.exterior()).chain(p.interiors()).map(ring).collect::<Vec<_>>()}),
        Geometry::MultiPolygon(m) => {
            json!({"type": "MultiPolygon", "coordinates": m.0.iter().map(|p| std::iter::once(p.exterior()).chain(p.interiors()).map(ring).collect::<Vec<_>>()).collect::<Vec<_>>()})
        }
        Geometry::Point(p) => json!({"type": "Point", "coordinates": [Value::from(p.x()), Value::from(p.y())]}),
        _ => Value::Null,
    }
}
