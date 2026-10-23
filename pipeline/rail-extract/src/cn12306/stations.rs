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

/// a pair of stops as the build keeps it: the two names in order and the places, in 1e-6 degrees
pub type Key = (String, String, [i64; 4]);

/// the key of the pair between two stops, and whether the stops run in the key's order (the first name is not after the second)
pub fn pair_key(na: &str, pa: (f64, f64), nb: &str, pb: (f64, f64)) -> (Key, bool) {
    let q = |p: (f64, f64)| [(p.0 * 1e6).round() as i64, (p.1 * 1e6).round() as i64];
    let swap = na > nb;
    let (a, b, pa, pb) = if swap { (nb, na, pb, pa) } else { (na, nb, pa, pb) };
    let [qa, qb] = [q(pa), q(pb)];
    ((a.to_string(), b.to_string(), [qa[0], qa[1], qb[0], qb[1]]), !swap)
}

#[derive(Deserialize)]
pub struct Train {
    /// the train code ("G1", "D2208"), for the debugging files
    #[serde(default)]
    pub code: String,
    /// 12306 class letter, `plain` for the rest
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

/// one stop of a train as the records read it
pub struct RunStop {
    pub name: String,
    pub pt: Option<(f64, f64)>,
    /// arrival and departure in minutes from the first stop's day (past midnight they run on), None when 12306 gives no time
    pub times: Option<(i32, i32)>,
}

/// a train with its stops and the pairs it runs: (index of the stop before, index of the stop after, index into the pair list)
pub struct TrainRun {
    pub code: String,
    pub class: String,
    pub stops: Vec<RunStop>,
    pub legs: Vec<(usize, usize, usize)>,
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

pub fn load_table(path: &Path) -> super::R<Table> {
    let text = super::io(std::fs::read_to_string(path), "read", path)?;
    let mut t = Table::default();
    for l in text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() < 4 || f.len() > 5 {
            return Err(format!("{}: expected 4 or 5 tab-separated fields: {l}", path.display()));
        }
        let num = |s: &str| s.parse::<f64>().map_err(|_| format!("{}: bad number {s:?}", path.display()));
        let pt = if f[1].is_empty() { None } else { Some((num(f[1])?, num(f[2])?)) };
        t.entry(f[0].to_string()).or_default().push(Row { pt, trusted: f[3] == "list" || f[3] == "fix+list", candidate: f[3] == "candidate" });
    }
    Ok(t)
}

/// the English names of the stations the 12306 list has them for (stations.tsv, fifth field)
pub fn load_english(path: &Path) -> super::R<FxHashMap<String, String>> {
    let text = super::io(std::fs::read_to_string(path), "read", path)?;
    Ok(text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() == 5 && !f[4].is_empty()).then(|| (f[0].to_string(), super::records::clean_en(f[0], f[4])))
        })
        .collect())
}

/// the days of the timetable as `shards.json` gives them: [{ "date": "2026-10-21", "classes": ["G", "D", ...] }, ...], from
/// pipeline/config/cn-12306/meta.json (written by build_inputs.py)
pub fn load_snapshot(input: &Path) -> super::R<serde_json::Value> {
    let p = input.join("meta.json");
    let text = super::io(std::fs::read_to_string(&p), "read", &p)?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
    let list = v["snapshots"].as_array().filter(|l| !l.is_empty()).ok_or_else(|| format!("{} has no \"snapshots\"", p.display()))?;
    let day = |s: &serde_json::Value| {
        let date = s["date"].as_str().ok_or_else(|| format!("{}: a snapshot has no \"date\"", p.display()))?;
        Ok(serde_json::json!({ "date": date, "classes": s["classes"] }))
    };
    Ok(serde_json::Value::Array(list.iter().map(day).collect::<super::R<Vec<_>>>()?))
}

/// the English gazetteer of city and station names (city_en.json, from the corridor-v3 project)
pub fn load_gazetteer(input: &Path) -> super::R<FxHashMap<String, String>> {
    let p = input.join("city_en.json");
    let text = super::io(std::fs::read_to_string(&p), "read", &p)?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))
}

pub fn load_timetable(path: &Path) -> super::R<Vec<Train>> {
    let text = super::io(std::fs::read_to_string(path), "read", path)?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
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
pub fn pairs(trains: &[Train], table: &Table) -> (Vec<PairIn>, Vec<TrainRun>, Counts) {
    let mut by_key: BTreeMap<Key, PairIn> = BTreeMap::new();
    let mut counts = Counts { trains: trains.len(), ..Default::default() };
    let mut runs: Vec<(&Train, Vec<RunStop>, Vec<(usize, usize, Key)>)> = Vec::new();
    for t in trains {
        let names: Vec<&str> = t.stops.iter().map(|s| s.0.as_str()).collect();
        let pts = place_stops(table, &names);
        counts.stops += names.len();
        counts.unplaced_stops += pts.iter().filter(|p| p.is_none()).count();
        let times = clock(&t.stops);
        let placed: Vec<usize> = (0..names.len()).filter(|&i| pts[i].is_some()).collect();
        let mut legs = Vec::new();
        for w in placed.windows(2) {
            let (i, j) = (w[0], w[1]);
            if names[i] == names[j] {
                continue;
            }
            if j > i + 1 {
                counts.spanning_pairs += 1;
            }
            let (pi, pj) = (pts[i].unwrap(), pts[j].unwrap());
            let (key, forward) = pair_key(names[i], pi, names[j], pj);
            let (a, b, pa, pb) = if forward { (names[i], names[j], pi, pj) } else { (names[j], names[i], pj, pi) };
            legs.push((i, j, key.clone()));
            let e = by_key.entry(key).or_insert_with(|| PairIn { a: a.to_string(), b: b.to_string(), pa, pb, fs: None, joined: true });
            e.joined &= j > i + 1;
            if let (Some(ti), Some(tj)) = (times[i], times[j]) {
                let lt = tj.0 - ti.1;
                if lt > 0 {
                    e.fs = Some(e.fs.map_or(lt as f64, |f| f.min(lt as f64)));
                }
            }
        }
        let stops = (0..names.len()).map(|i| RunStop { name: names[i].to_string(), pt: pts[i], times: times[i] }).collect();
        runs.push((t, stops, legs));
    }
    let index: BTreeMap<Key, usize> = by_key.keys().cloned().enumerate().map(|(i, k)| (k, i)).collect();
    let runs = runs
        .into_iter()
        .map(|(t, stops, legs)| TrainRun {
            code: t.code.clone(),
            class: t.class.clone(),
            stops,
            legs: legs.into_iter().map(|(i, j, k)| (i, j, index[&k])).collect(),
        })
        .collect();
    (by_key.into_values().collect(), runs, counts)
}
