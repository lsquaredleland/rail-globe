//! The 12306 timetable and the station table -> the stop pairs to route. The table (stations.tsv) gives every station name a point;
//! a name with several candidates (two stations of that name in OSM) takes the one nearest the neighbouring stops, and a stop that
//! sits nowhere between its neighbours is dropped, as the prototype did.

use rustc_hash::FxHashMap;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// one row of stations.tsv
pub struct Row {
    pub pt: Option<(f64, f64)>,
    /// `list` and `fix+list`: the station is on the 12306 list, so it is never dropped as misplaced
    pub trusted: bool,
    /// one of several places of that name
    pub candidate: bool,
}

pub type Table = FxHashMap<String, Vec<Row>>;

#[derive(Deserialize)]
pub struct Train {
    /// 12306 class letter, `plain` for the rest (the file also has the train code, which the build does not read)
    pub class: String,
    /// name, arrival, departure ("HH:MM", or empty)
    pub stops: Vec<(String, String, String)>,
}

pub struct PairIn {
    pub a: String,
    pub b: String,
    pub pa: (f64, f64),
    pub pb: (f64, f64),
    /// the fastest scheduled time between the two stops over all trains, minutes
    pub fs: Option<f64>,
    /// every train that has the pair runs it over an unplaced stop: without a path it is dropped, not drawn as a chord
    pub joined: bool,
}

#[derive(Default)]
pub struct Counts {
    pub trains: usize,
    pub stops: usize,
    /// stops left out: no point for the name, or placed nowhere between the neighbours
    pub unplaced_stops: usize,
    /// pairs that run over one or more of those stops, from the stop before to the stop after
    pub spanning_pairs: usize,
}

pub fn load_table(path: &Path) -> Table {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| crate::fail(format!("read {}: {e}", path.display())));
    let mut t = Table::default();
    for l in text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() != 4 {
            crate::fail(format!("{}: expected 4 tab-separated fields: {l}", path.display()));
        }
        let num = |s: &str| s.parse::<f64>().unwrap_or_else(|_| crate::fail(format!("{}: bad number {s:?}", path.display())));
        let pt = (!f[1].is_empty()).then(|| (num(f[1]), num(f[2])));
        t.entry(f[0].to_string()).or_default().push(Row { pt, trusted: f[3] == "list" || f[3] == "fix+list", candidate: f[3] == "candidate" });
    }
    t
}

pub fn load_timetable(path: &Path) -> Vec<Train> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| crate::fail(format!("read {}: {e}", path.display())));
    serde_json::from_str(&text).unwrap_or_else(|e| crate::fail(format!("{}: {e}", path.display())))
}

/// squared distance in degrees with longitude shrunk, to compare candidates
fn hv(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0) * 0.87).powi(2) + (a.1 - b.1).powi(2)
}

/// km on a flat map, for the sanity test of a stop
fn flat_km(p: (f64, f64), q: (f64, f64)) -> f64 {
    let la = (p.1 + q.1) / 2.0;
    ((p.0 - q.0) * 111.0 * la.to_radians().cos()).hypot((p.1 - q.1) * 111.0)
}

/// the point of every stop of a train, None for a stop that has none
pub fn place_stops(table: &Table, names: &[&str]) -> Vec<Option<(f64, f64)>> {
    let rows = |i: usize| table.get(names[i]).map(|v| v.as_slice()).unwrap_or(&[]);
    let mut pts: Vec<Option<(f64, f64)>> = (0..names.len()).map(|i| if let [r] = rows(i) { r.pt } else { None }).collect();
    // two stations of that name: the one nearest the nearest placed stops before and after it
    for i in 0..names.len() {
        if !rows(i).first().is_some_and(|r| r.candidate) {
            continue;
        }
        let before = (0..i).rev().find_map(|j| pts[j]);
        let after = (i + 1..names.len()).find_map(|j| pts[j]);
        let refs: Vec<(f64, f64)> = before.into_iter().chain(after).collect();
        let score = |p: (f64, f64)| refs.iter().map(|&r| hv(p, r)).sum::<f64>();
        pts[i] = rows(i)
            .iter()
            .filter_map(|r| r.pt)
            .fold(None::<((f64, f64), f64)>, |b, p| if b.is_none_or(|b| score(p) < b.1) { Some((p, score(p))) } else { b })
            .map(|b| b.0);
    }
    // a name that matches the wrong place (福田, 明光, 黄水 resolve into Chengdu): drop a stop that does not sit between its neighbours
    for i in 0..names.len() {
        let Some(p) = pts[i] else { continue };
        if rows(i).first().is_some_and(|r| r.trusted) {
            continue;
        }
        // the nearest placed stops on either side
        let nb: Vec<(f64, f64)> = (0..i).rev().find_map(|j| pts[j]).into_iter().chain((i + 1..names.len()).find_map(|j| pts[j])).collect();
        if (nb.len() == 2 && flat_km(nb[0], p) + flat_km(p, nb[1]) > 1.6 * flat_km(nb[0], nb[1]) + 60.0) || (nb.len() == 1 && flat_km(nb[0], p) > 400.0) {
            pts[i] = None;
        }
    }
    pts
}

/// minutes of arrival and departure at every stop, running past midnight (a time earlier than the one before is the next day);
/// None for a stop with no time at all, which takes no part in the day count
fn clock(stops: &[(String, String, String)]) -> Vec<Option<(i32, i32)>> {
    let min = |s: &str| -> Option<i32> {
        let (h, m) = s.split_once(':')?;
        Some(h.parse::<i32>().ok()? * 60 + m.parse::<i32>().ok()?)
    };
    let (mut day, mut prev) = (0, i32::MIN); // before the first stop nothing is earlier
    stops
        .iter()
        .map(|(_, arrive, depart)| {
            let (a, d) = (min(arrive), min(depart));
            let (a, d) = (a.or(d)?, d.or(a)?);
            while a + day < prev {
                day += 1440;
            }
            prev = a + day;
            let arr = a + day;
            if d + day < prev {
                day += 1440;
            }
            prev = d + day;
            Some((arr, d + day))
        })
        .collect()
}

/// the stop pairs of all trains, sorted by name pair and points. A stop without a place is passed over: the pair runs from the stop
/// before it to the stop after it, with the whole time between. Two places of one name make two pairs.
pub fn pairs(trains: &[Train], table: &Table) -> (Vec<PairIn>, Counts) {
    type Key = (String, String, [i64; 4]);
    let q = |p: (f64, f64)| [(p.0 * 1e6).round() as i64, (p.1 * 1e6).round() as i64];
    let mut by_key: BTreeMap<Key, PairIn> = BTreeMap::new();
    let mut counts = Counts { trains: trains.len(), ..Default::default() };
    for t in trains {
        let names: Vec<&str> = t.stops.iter().map(|s| s.0.as_str()).collect();
        let pts = place_stops(table, &names);
        counts.stops += names.len();
        counts.unplaced_stops += pts.iter().filter(|p| p.is_none()).count();
        let times = clock(&t.stops);
        let placed: Vec<usize> = (0..names.len()).filter(|&i| pts[i].is_some()).collect();
        for w in placed.windows(2) {
            let (i, j) = (w[0], w[1]);
            if names[i] == names[j] {
                continue;
            }
            if j > i + 1 {
                counts.spanning_pairs += 1;
            }
            let (pi, pj) = (pts[i].unwrap(), pts[j].unwrap());
            let swap = names[i] > names[j];
            let (a, b, pa, pb) = if swap { (names[j], names[i], pj, pi) } else { (names[i], names[j], pi, pj) };
            let [qa, qb] = [q(pa), q(pb)];
            let e = by_key.entry((a.to_string(), b.to_string(), [qa[0], qa[1], qb[0], qb[1]])).or_insert_with(|| PairIn {
                a: a.to_string(),
                b: b.to_string(),
                pa,
                pb,
                fs: None,
                joined: true,
            });
            e.joined &= j > i + 1;
            if let (Some(ti), Some(tj)) = (times[i], times[j]) {
                let lt = tj.0 - ti.1;
                if lt > 0 {
                    e.fs = Some(e.fs.map_or(lt as f64, |f| f.min(lt as f64)));
                }
            }
        }
    }
    (by_key.into_values().collect(), counts)
}
