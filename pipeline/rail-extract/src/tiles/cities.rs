//! City dots: one point per city with a metro, tram or monorail system, from the track itself.

use super::props::CityProps;
use super::util::feat;
use rustc_hash::FxHashMap;

const CITY_LINK_KM: f64 = 3.0; // track samples this close (directly or through others) are one city dot
const CITY_ABSORB_KM: f64 = 15.0; // a small group this close to a much bigger one (see CITY_ABSORB_RATIO) is part of that city
const CITY_ABSORB_RATIO: f64 = 5.0;
const CITY_MIN_KM: f64 = 6.0; // a city dot needs this much metro / tram / monorail track

/// One dot per city with a metro, tram or monorail system, from the track itself: open passenger ways of type metro, `tram` ways and
/// monorail (not light_rail, which OSM uses for German S-Bahn, and not service track), sampled about every km. Samples within CITY_LINK_KM of
/// each other, directly or through others, are one city (single linkage), so a city is one dot however far it spreads; groups with
/// less than CITY_MIN_KM of track are dropped. Properties: m / l / r = metro / tram / monorail km, n = all km.
pub(super) fn city_dots(samples: &[(f64, f64, f64, u8)]) -> String {
    let km = |a: (f64, f64), b: (f64, f64)| {
        let dy = (a.1 - b.1) * 111.2;
        let dx = (a.0 - b.0) * 111.2 * ((a.1 + b.1) / 2.0).to_radians().cos();
        (dx * dx + dy * dy).sqrt()
    };
    // grid of 0.2 degrees: neighbours are in the surrounding 3 x 3 cells
    let cell = |v: f64| (v / 0.2).floor() as i32;
    let mut grid: FxHashMap<(i32, i32), Vec<usize>> = FxHashMap::default();
    for (i, p) in samples.iter().enumerate() {
        grid.entry((cell(p.0), cell(p.1))).or_default().push(i);
    }
    let mut parent: Vec<usize> = (0..samples.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..samples.len() {
        let (cx, cy) = (cell(samples[i].0), cell(samples[i].1));
        for gx in cx - 1..=cx + 1 {
            for gy in cy - 1..=cy + 1 {
                for &j in grid.get(&(gx, gy)).into_iter().flatten() {
                    if j > i && km((samples[i].0, samples[i].1), (samples[j].0, samples[j].1)) <= CITY_LINK_KM {
                        let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                        if a != b {
                            parent[a.max(b)] = a.min(b);
                        }
                    }
                }
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for i in 0..samples.len() {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    // a small group near a much bigger one is part of that city (a satellite district or suburb whose track is not linked to the main
    // network): absorb it, biggest groups first, so a big city is one dot. Equal neighbours (two cities) stay apart.
    let km_of = |m: &[usize]| m.iter().map(|&j| samples[j].2).sum::<f64>();
    let mut order: Vec<usize> = groups.keys().copied().collect();
    order.sort_by(|a, b| km_of(&groups[b]).partial_cmp(&km_of(&groups[a])).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(b)));
    let mut owner: Vec<usize> = (0..samples.len()).map(|i| find(&mut parent, i)).collect();
    let mut total: FxHashMap<usize, f64> = order.iter().map(|&g| (g, km_of(&groups[&g]))).collect();
    for &g in &order {
        let mine = total[&g];
        let mut target = None;
        'find: for &i in &groups[&g] {
            let (cx, cy) = (cell(samples[i].0), cell(samples[i].1));
            for gx in cx - 2..=cx + 2 {
                for gy in cy - 2..=cy + 2 {
                    for &j in grid.get(&(gx, gy)).into_iter().flatten() {
                        let o = owner[j];
                        if o != g && total[&o] >= CITY_ABSORB_RATIO * mine && km((samples[i].0, samples[i].1), (samples[j].0, samples[j].1)) <= CITY_ABSORB_KM {
                            target = Some(o);
                            break 'find;
                        }
                    }
                }
            }
        }
        if let Some(o) = target {
            for &i in &groups[&g] {
                owner[i] = o;
            }
            *total.get_mut(&o).unwrap() += mine;
            total.insert(g, 0.0);
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for i in 0..samples.len() {
        groups.entry(owner[i]).or_default().push(i);
    }
    let mut out = String::new();
    for members in groups.values() {
        let by = |k: u8| members.iter().filter(|&&j| samples[j].3 == k).map(|&j| samples[j].2).sum::<f64>();
        let total = by(0) + by(1) + by(2);
        if total < CITY_MIN_KM {
            continue;
        }
        let n = members.len() as f64;
        let mid = (members.iter().map(|&j| samples[j].0).sum::<f64>() / n, members.iter().map(|&j| samples[j].1).sum::<f64>() / n);
        // the dot sits on the sample nearest the middle, so it is never off the network (a city can be long and thin)
        let at = members.iter().copied().min_by(|&a, &b| km((samples[a].0, samples[a].1), mid).partial_cmp(&km((samples[b].0, samples[b].1), mid)).unwrap_or(std::cmp::Ordering::Equal)).unwrap();
        let pr = CityProps { m: by(0).round() as i64, l: by(1).round() as i64, r: by(2).round() as i64, n: total.round() as i64 }.to_map();
        feat(&mut out, &pr, &format!("{{\"type\":\"Point\",\"coordinates\":[{:.5},{:.5}]}}", samples[at].0, samples[at].1), Some(2), Some(7));
    }
    out
}
