//! Station records: what a click on a station in China shows from the 12306 timetable. One record per app station that has trains,
//! keyed by the station id of `stations/` (the train station of a complex, nearest to the 12306 point): departures and arrivals,
//! hourly bars, the cities its trains go to, a table of every destination station both ways, and the stretches its trains use in
//! five width bands. The destination cities follow the prototype: a stop is a city by the outline of its prefecture, and a stop in
//! Beijing, Shanghai, Tianjin or Chongqing outside the core is its own place.

use super::stations::TrainRun;
use crate::region::Region;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::OnceLock;

/// One record of cn12306/<rev>/<n>.json (the shape of app/src/data.ts `CnRecord`). The field order is the key order of the file.
#[derive(Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub(crate) struct CnRecord<'a> {
    pub name: &'a str,
    pub name_en: Option<String>,
    pub stops: usize,
    pub dep: usize,
    pub arr: usize,
    pub first: Option<i32>,
    pub last: Option<i32>,
    pub hd: [u32; 24],
    pub ha: [u32; 24],
    #[cfg_attr(test, schemars(with = "Vec<serde_json::Value>"))]
    pub dest: Vec<Value>,
    #[cfg_attr(test, schemars(with = "std::collections::BTreeMap<String, serde_json::Value>"))]
    pub dst: serde_json::Map<String, Value>,
    pub st: Vec<Vec<u32>>,
    pub pts: Vec<[f64; 2]>,
}

/// a regex compiled once
fn re(cell: &'static OnceLock<regex::Regex>, pattern: &str) -> &'static regex::Regex {
    cell.get_or_init(|| regex::Regex::new(pattern).unwrap())
}

/// trains a station's stretch must carry for the next width band
const BAND_EDGES: [u32; 4] = [5, 15, 40, 100];
/// destination stations listed under a city
const CITY_PARTS: usize = 6;
/// the four municipalities whose outline holds many separate places
const MUNICIPALITIES: [&str; 4] = ["cn-beijing", "cn-shanghai", "cn-tianjin", "cn-chongqing"];
/// a 12306 point and an app station of the same name are the same station within about this much, degrees squared (3 km)
const MATCH_DEG2: f64 = 0.03 * 0.03;

pub struct AppStation {
    pub id: String,
    pub complex: String,
    pub pt: (f64, f64),
    pub train: bool,
    pub name_en: Option<String>,
}

/// stations by 12306-style name (blanks and a trailing 站 removed) and complexes by id
pub struct AppStations {
    by_name: FxHashMap<String, Vec<usize>>,
    pub list: Vec<AppStation>,
    pub complex_members: FxHashMap<String, Vec<String>>,
}

fn norm(s: &str) -> String {
    let t: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    t.strip_suffix('站').filter(|x| !x.is_empty()).map(String::from).unwrap_or(t)
}

/// the Chinese part of a bilingual name ("哈密 قۇمۇل", "延吉西 연길서"): the words with a Han character, joined
fn han_part(s: &str) -> Option<String> {
    let words: Vec<&str> = s.split_whitespace().filter(|w| w.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))).collect();
    (!words.is_empty()).then(|| norm(&words.concat()))
}

impl AppStations {
    pub fn load(dir: &Path) -> super::R<AppStations> {
        let read = |f: &str| super::io(std::fs::read_to_string(dir.join(f)), "read", &dir.join(f));
        let mut list = Vec::new();
        let mut by_name: FxHashMap<String, Vec<usize>> = FxHashMap::default();
        for l in read("stations.geojsonl")?.lines() {
            let f: Value = serde_json::from_str(l).map_err(|e| format!("stations.geojsonl: {e}"))?;
            let p = &f["properties"];
            let (Some(id), Some(name), Some(lon), Some(lat)) =
                (p["id"].as_str(), p["name"].as_str(), f["geometry"]["coordinates"][0].as_f64(), f["geometry"]["coordinates"][1].as_f64())
            else {
                continue;
            };
            let own = norm(name);
            if let Some(zh) = han_part(name).filter(|z| *z != own) {
                by_name.entry(zh).or_default().push(list.len());
            }
            by_name.entry(own).or_default().push(list.len());
            list.push(AppStation {
                id: id.to_string(),
                complex: p["complex"].as_str().unwrap_or(id).to_string(),
                pt: (lon, lat),
                train: p["modes"].as_array().is_some_and(|m| m.iter().any(|x| x == "train")),
                name_en: p["name_en"].as_str().map(String::from),
            });
        }
        let mut complex_members: FxHashMap<String, Vec<String>> = FxHashMap::default();
        for l in read("complexes.geojsonl")?.lines() {
            let f: Value = serde_json::from_str(l).map_err(|e| format!("complexes.geojsonl: {e}"))?;
            if let Some(id) = f["properties"]["id"].as_str() {
                let ids = f["properties"]["station_ids"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
                complex_members.insert(id.to_string(), ids);
            }
        }
        Ok(AppStations { by_name, list, complex_members })
    }

    /// the app station of a 12306 stop: of that name within 3 km, a train station before any other, the nearest first
    pub fn find(&self, name: &str, pt: (f64, f64)) -> Option<usize> {
        let hv = |a: (f64, f64)| ((a.0 - pt.0) * 0.87).powi(2) + (a.1 - pt.1).powi(2);
        let near: Vec<usize> = self.by_name.get(&norm(name))?.iter().copied().filter(|&i| hv(self.list[i].pt) < MATCH_DEG2).collect();
        let pool: Vec<usize> = if near.iter().any(|&i| self.list[i].train) { near.iter().copied().filter(|&i| self.list[i].train).collect() } else { near };
        pool.into_iter().min_by(|&a, &b| hv(self.list[a].pt).total_cmp(&hv(self.list[b].pt)).then(self.list[a].id.cmp(&self.list[b].id)))
    }
}

struct City {
    slug: String,
    zh: String,
    en: String,
    region: Region,
    area: i64,
}

/// the prefecture-level cities (admin level 5) and the municipalities of China, from data/regions
pub struct Cities(Vec<City>);

/// the Chinese short name of a prefecture: no 市, no 族 tail, no 自治州 / 地区 / 盟
pub(super) fn city_zh(name: &str) -> String {
    let first = name.split_whitespace().next().unwrap_or("");
    let mut zh: Vec<char> = first.chars().collect();
    if first.ends_with('市') && zh.len() > 2 {
        zh.pop();
    }
    let mut s: String = zh.iter().collect();
    static PEOPLE: OnceLock<regex::Regex> = OnceLock::new();
    static KIND: OnceLock<regex::Regex> = OnceLock::new();
    static PEOPLE_TAIL: OnceLock<regex::Regex> = OnceLock::new();
    s = re(&PEOPLE, r"(?:土家|布依|哈尼|景颇|傈僳|柯尔克孜|哈萨克|蒙古|朝鲜|[藏羌彝苗白傣壮回满维])族.*$").replace(&s, "").to_string();
    if s.chars().count() > 3 {
        s = re(&KIND, r"(自治州|地区|盟)$").replace(&s, "").to_string();
    }
    if s.chars().count() > 3 {
        s = re(&PEOPLE_TAIL, r"(蒙古|柯尔克孜|哈萨克)$").replace(&s, "").to_string();
    }
    s
}

pub(super) fn city_en(name_en: &str, slug: &str) -> String {
    static SUFFIX: OnceLock<regex::Regex> = OnceLock::new();
    let t = re(&SUFFIX, r"\s+(City|Prefecture|Municipality|Autonomous Prefecture|League)$").replace(name_en, "").trim().to_string();
    if !t.is_empty() {
        return t;
    }
    slug[3..]
        .split('-')
        .map(|w| w.chars().next().map(|c| c.to_uppercase().collect::<String>() + &w[c.len_utf8()..]).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" ")
}

impl Cities {
    /// rectangular cities (zh, en, [west, south, east, north] in degrees), for tests
    #[cfg(test)]
    pub(super) fn rectangles(list: &[(&str, &str, [f64; 4])]) -> Cities {
        let d7 = |v: f64| (v * 1e7).round() as i64;
        Cities(
            list.iter()
                .map(|(zh, en, b)| {
                    let ring = vec![(d7(b[0]), d7(b[1])), (d7(b[2]), d7(b[1])), (d7(b[2]), d7(b[3])), (d7(b[0]), d7(b[3])), (d7(b[0]), d7(b[1]))];
                    City { slug: format!("cn-{en}"), zh: zh.to_string(), en: en.to_string(), region: Region::new(en, vec![ring]), area: 0 }
                })
                .collect(),
        )
    }

    pub fn load(regions: &Path, gazetteer: &FxHashMap<String, String>) -> super::R<Cities> {
        let text = super::io(std::fs::read_to_string(regions.join("index.json")), "read", &regions.join("index.json"))?;
        let idx: Value = serde_json::from_str(&text).map_err(|e| format!("index.json: {e}"))?;
        let slugs: Vec<(&String, &Value)> = idx
            .as_object()
            .ok_or("index.json is not an object")?
            .iter()
            .filter(|(s, v)| s.starts_with("cn-") && (v["admin_level"] == "5" || MUNICIPALITIES.contains(&s.as_str()) || s.as_str() == "cn-hong-kong"))
            .collect();
        let mut cities: Vec<City> = slugs
            .par_iter()
            .map(|(slug, v)| {
                let region = Region::load(slug, &regions.join(format!("{slug}.geojson")));
                let b = region.bbox; // min lat, min lon, max lat, max lon, 1e-7 degrees
                let (zh, osm_en) = (city_zh(v["name"].as_str().unwrap_or("")), v["name_en"].as_str().unwrap_or(""));
                // a city whose outline has no English name takes the gazetteer's before a name made from its slug
                let en = if osm_en.trim().is_empty() { gazetteer.get(&zh).cloned().unwrap_or_else(|| city_en(osm_en, slug)) } else { city_en(osm_en, slug) };
                City { slug: slug.to_string(), zh, en, area: (b[2] as i64 - b[0] as i64) / 1000 * ((b[3] as i64 - b[1] as i64) / 1000), region }
            })
            .collect();
        cities.sort_by_key(|c| c.area); // the smallest first: a prefecture inside another is the one asked for
        Ok(Cities(cities))
    }

    fn of(&self, pt: (f64, f64)) -> Option<usize> {
        let (lat, lon) = ((pt.1 * 1e7).round() as i32, (pt.0 * 1e7).round() as i32);
        self.0.iter().position(|c| c.region.contains(lat, lon))
    }

    /// a station named like its city, or the city and a direction ("沧州西"): the English of the city, with East / West / South / North
    fn english_by_city(&self, zh: &str, pt: (f64, f64)) -> Option<String> {
        let c = &self.0[self.of(pt)?];
        let rest = zh.strip_prefix(c.zh.as_str())?;
        let dir = match rest {
            "" => return Some(c.en.clone()),
            "东" => "East",
            "西" => "West",
            "南" => "South",
            "北" => "North",
            _ => return None,
        };
        Some(format!("{} {dir}", c.en))
    }

    /// the place a stop counts as: its city, or its own name for a stop outside the core of a municipality
    fn place(&self, name: &str, pt: (f64, f64)) -> Option<(String, Option<String>)> {
        let c = &self.0[self.of(pt)?];
        if MUNICIPALITIES.contains(&c.slug.as_str()) && !(name.starts_with(&c.zh) || name == "沙坪坝" || name.contains("机场")) {
            let n = name.chars().count();
            let own =
                if n > 2 && matches!(name.chars().last(), Some('东' | '西' | '南' | '北')) { name.chars().take(n - 1).collect() } else { name.to_string() };
            return Some((own, None));
        }
        Some((c.zh.clone(), Some(c.en.clone())))
    }
}

/// an English station name from 12306 / OSM text: no "(China Railway)", no trailing Station / Zhan / RS, "Chengdudong" as "Chengdu East"
pub fn clean_en(zh: &str, en: &str) -> String {
    let mut s = en.to_string();
    for tag in ["(China Railway)", "(CR)"] {
        s = s.replace(&format!(" {tag}"), "").replace(tag, "");
    }
    // a hyphen between a lower-case and an upper-case letter is a space
    let cs: Vec<char> = s.chars().collect();
    s = cs
        .iter()
        .enumerate()
        .map(|(i, &c)| if c == '-' && i > 0 && i + 1 < cs.len() && cs[i - 1].is_ascii_lowercase() && cs[i + 1].is_ascii_uppercase() { ' ' } else { c })
        .collect();
    static TAIL: OnceLock<regex::Regex> = OnceLock::new();
    static RS: OnceLock<regex::Regex> = OnceLock::new();
    s = re(&TAIL, r"(?i)\s+(Railway\s+)?(Station|Zhan|RS)$").replace(&s, "").trim().to_string();
    s = re(&RS, r"\s+RS$").replace(&s, "").to_string();
    let dir = match zh.chars().last() {
        Some('东') => Some(("dong", "East")),
        Some('西') => Some(("xi", "West")),
        Some('南') => Some(("nan", "South")),
        Some('北') => Some(("bei", "North")),
        _ => None,
    };
    if let Some((suffix, word)) = dir {
        let plain = s.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && s.chars().all(|c| c.is_ascii_alphabetic() || matches!(c, '\'' | '’' | '-'));
        if plain && s.ends_with(suffix) && s.len() > suffix.len() + 2 {
            s = format!("{} {}", &s[..s.len() - suffix.len()], word);
        }
    }
    s
}

/// per destination station: trains forward (from this station to it) and back, fastest minutes, first and last departure of day
#[derive(Default)]
struct Dst {
    fwd: u32,
    fwd_fast: Option<i32>,
    fwd_dep: Vec<i32>,
    back: u32,
    back_fast: Option<i32>,
    back_dep: Vec<i32>,
}

fn min_opt(a: Option<i32>, b: Option<i32>) -> Option<i32> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

pub struct Out {
    /// (app station id, record as JSON text), sorted by id
    pub records: Vec<(String, String)>,
    /// station or complex id -> the key of the record (only where they differ)
    pub ids: BTreeMap<String, String>,
    pub report: Value,
}

/// `train_stretches[t]`: the stretch indexes (ascending) the train `t` of `runs` runs over, stretches numbered in the order of their ids;
/// `table_en` the English names of the station table, `gazetteer` the corridor-v3 gazetteer, which comes before the name made from the
/// city
pub fn build(
    runs: &[TrainRun],
    train_stretches: &[Vec<u32>],
    app: &AppStations,
    cities: &Cities,
    table_en: &FxHashMap<String, String>,
    gazetteer: &FxHashMap<String, String>,
) -> Out {
    // the app station of every placed stop
    let mut cache: FxHashMap<(String, i64, i64), Option<usize>> = FxHashMap::default();
    let mut unmatched: BTreeMap<String, u32> = BTreeMap::new();
    let key_of: Vec<Vec<Option<usize>>> = runs
        .iter()
        .map(|t| {
            t.stops
                .iter()
                .map(|s| {
                    let pt = s.pt?;
                    let k = (s.name.clone(), (pt.0 * 1e5).round() as i64, (pt.1 * 1e5).round() as i64);
                    let m = *cache.entry(k).or_insert_with(|| app.find(&s.name, pt));
                    if m.is_none() {
                        *unmatched.entry(s.name.clone()).or_default() += 1;
                    }
                    m
                })
                .collect()
        })
        .collect();
    let mut at: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
    for (ti, ks) in key_of.iter().enumerate() {
        for (i, k) in ks.iter().enumerate() {
            if let Some(k) = k {
                at.entry(*k).or_default().push((ti, i));
            }
        }
    }
    // the cities of the stops, once per place on the map
    let mut place_cache: FxHashMap<(String, i64, i64), Option<(String, Option<String>)>> = FxHashMap::default();
    let mut place = |name: &str, pt: (f64, f64)| -> Option<(String, Option<String>)> {
        place_cache.entry((name.to_string(), (pt.0 * 1e4).round() as i64, (pt.1 * 1e4).round() as i64)).or_insert_with(|| cities.place(name, pt)).clone()
    };
    // the 12306 name of an app station: the one most trains give it
    let mut names_of: FxHashMap<usize, BTreeMap<&str, u32>> = FxHashMap::default();
    for (ti, ks) in key_of.iter().enumerate() {
        for (i, k) in ks.iter().enumerate() {
            if let Some(k) = k {
                *names_of.entry(*k).or_default().entry(runs[ti].stops[i].name.as_str()).or_default() += 1;
            }
        }
    }
    let zh_of = |k: usize| -> &str { names_of[&k].iter().max_by_key(|(n, c)| (**c, std::cmp::Reverse(**n))).map(|(n, _)| *n).unwrap() };
    // the English of a station: the 12306 list, the app station (OSM), the gazetteer, the city's name with a direction; the first says where from
    let en_of = |zh: &str, k: Option<usize>| -> Option<(String, bool)> {
        table_en
            .get(zh)
            .cloned()
            .map(|e| (e, false))
            .or_else(|| k.and_then(|k| app.list[k].name_en.as_deref()).map(|e| (clean_en(zh, e), false)))
            .or_else(|| gazetteer.get(zh).cloned().map(|e| (e, true)))
            .or_else(|| k.and_then(|k| cities.english_by_city(zh, app.list[k].pt)).map(|e| (e, false)))
            .filter(|e| !e.0.is_empty())
    };

    let mut records: Vec<(String, String)> = Vec::new();
    let mut english_gaps = 0;
    let mut english_from_gazetteer = 0;
    let mut sizes: Vec<usize> = Vec::new();
    for (&k, items) in &at {
        let zh = zh_of(k);
        let mut dep: Vec<i32> = Vec::new();
        let mut arr: Vec<i32> = Vec::new();
        let mut dst: BTreeMap<usize, Dst> = BTreeMap::new();
        struct CityAcc {
            en: Option<String>,
            n: u32,
            fast: Option<i32>,
            parts: BTreeMap<String, (Option<usize>, u32, Option<i32>)>,
        }
        let mut cit: BTreeMap<String, CityAcc> = BTreeMap::new();
        // the stretches the station's trains run over: the number of its trains on each
        let mut per_stretch: FxHashMap<u32, u32> = FxHashMap::default();
        let mut pts: BTreeSet<(i64, i64)> = BTreeSet::new();
        for &(ti, i) in items {
            let t = &runs[ti];
            let me = t.stops[i].pt.unwrap();
            let home = place(&t.stops[i].name, me);
            if i + 1 < t.stops.len() {
                if let Some(tm) = t.stops[i].times {
                    dep.push(tm.1.rem_euclid(1440));
                }
            }
            if i > 0 {
                if let Some(tm) = t.stops[i].times {
                    arr.push(tm.0.rem_euclid(1440));
                }
            }
            let mut seen: FxHashSet<String> = FxHashSet::default();
            for (j, sj) in t.stops.iter().enumerate() {
                let Some(pj) = sj.pt else { continue };
                if j == i {
                    continue;
                }
                pts.insert(((pj.0 * 100.0).round() as i64, (pj.1 * 100.0).round() as i64));
                let jk = key_of[ti][j].filter(|&x| x != k);
                if j > i {
                    let fast = t.stops[i].times.zip(sj.times).map(|(a, b)| b.0 - a.1).filter(|&m| m > 0);
                    if let Some(jk) = jk {
                        let d = dst.entry(jk).or_default();
                        d.fwd += 1;
                        d.fwd_fast = min_opt(d.fwd_fast, fast);
                        if let Some(tm) = t.stops[i].times {
                            d.fwd_dep.push(tm.1.rem_euclid(1440));
                        }
                    }
                    if let Some(c) = place(&sj.name, pj) {
                        if Some(&c) != home.as_ref() {
                            // the city counts once per train; every stop of it on the train counts in the breakdown
                            let first = seen.insert(c.0.clone());
                            let e = cit.entry(c.0.clone()).or_insert_with(|| CityAcc { en: c.1.clone(), n: 0, fast: None, parts: BTreeMap::new() });
                            if first {
                                e.n += 1;
                            }
                            e.fast = min_opt(e.fast, fast);
                            let p = e.parts.entry(sj.name.clone()).or_insert((key_of[ti][j], 0, None));
                            p.1 += 1;
                            p.2 = min_opt(p.2, fast);
                        }
                    }
                } else if let Some(jk) = jk {
                    let fast = sj.times.zip(t.stops[i].times).map(|(a, b)| b.0 - a.1).filter(|&m| m > 0);
                    let d = dst.entry(jk).or_default();
                    d.back += 1;
                    d.back_fast = min_opt(d.back_fast, fast);
                    if let Some(tm) = sj.times {
                        d.back_dep.push(tm.1.rem_euclid(1440));
                    }
                }
            }
            for &s in &train_stretches[ti] {
                *per_stretch.entry(s).or_default() += 1;
            }
        }
        let mut hd = [0u32; 24];
        let mut ha = [0u32; 24];
        dep.iter().for_each(|m| hd[(*m / 60) as usize] += 1);
        arr.iter().for_each(|m| ha[(*m / 60) as usize] += 1);
        // per width band the stretch numbers, ascending, each as the step from the one before (the first from 0)
        let mut bands: Vec<Vec<u32>> = vec![Vec::new(); BAND_EDGES.len() + 1];
        for (s, n) in &per_stretch {
            bands[BAND_EDGES.iter().filter(|e| n >= e).count()].push(*s);
        }
        for b in &mut bands {
            b.sort_unstable();
            let mut prev = 0;
            for s in b.iter_mut() {
                (prev, *s) = (*s, *s - prev);
            }
        }

        let mut cities_out: Vec<(&String, &CityAcc)> = cit.iter().collect();
        cities_out.sort_by(|a, b| b.1.n.cmp(&a.1.n).then(a.0.cmp(b.0)));
        let dest: Vec<Value> = cities_out
            .into_iter()
            .map(|(zh_c, c)| {
                let mut parts: Vec<(&String, &(Option<usize>, u32, Option<i32>))> = c.parts.iter().collect();
                parts.sort_by(|a, b| b.1 .1.cmp(&a.1 .1).then(a.0.cmp(b.0)));
                let p: Vec<Value> = parts.into_iter().take(CITY_PARTS).map(|(n, v)| json!([n, en_of(n, v.0).map(|e| e.0), v.1, v.2.unwrap_or(0)])).collect();
                json!([zh_c, c.en, c.n, c.fast.unwrap_or(0), p])
            })
            .collect();
        let rng = |v: &[i32]| -> (i32, i32) { (v.iter().copied().min().unwrap_or(0), v.iter().copied().max().unwrap_or(0)) };
        let dst_out: serde_json::Map<String, Value> = dst
            .iter()
            .map(|(jk, d)| {
                let (f1, f2) = rng(&d.fwd_dep);
                let (b1, b2) = rng(&d.back_dep);
                (app.list[*jk].id.clone(), json!([d.fwd, d.fwd_fast.unwrap_or(0), f1, f2, d.back, d.back_fast.unwrap_or(0), b1, b2]))
            })
            .collect();
        let name_en = en_of(zh, Some(k));
        match &name_en {
            None => english_gaps += 1,
            Some((_, true)) => english_from_gazetteer += 1,
            _ => {}
        }
        let name_en = name_en.map(|e| e.0);
        let (first, last) = rng(&dep);
        let rec = CnRecord {
            name: zh,
            name_en,
            stops: items.len(),
            dep: dep.len(),
            arr: arr.len(),
            first: if dep.is_empty() { None } else { Some(first) },
            last: if dep.is_empty() { None } else { Some(last) },
            hd,
            ha,
            dest,
            dst: dst_out,
            st: bands,
            pts: pts.iter().map(|p| [p.0 as f64 / 100.0, p.1 as f64 / 100.0]).collect(),
        };
        let text = serde_json::to_string(&rec).expect("china record serializes");
        sizes.push(text.len());
        records.push((app.list[k].id.clone(), text));
    }
    records.sort();

    // any id of the station's complex finds its record
    let keys: FxHashSet<&str> = at.keys().map(|&k| app.list[k].id.as_str()).collect();
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    let mut clash = 0;
    for &k in at.keys() {
        let s = &app.list[k];
        let mut members: Vec<&String> = vec![&s.complex];
        members.extend(app.complex_members.get(&s.complex).into_iter().flatten());
        for m in members {
            // an id with a record of its own opens that record
            if *m != s.id && !keys.contains(m.as_str()) {
                match ids.get(m) {
                    // two stations of one complex claim the id: the smallest station id keeps it, whatever the order
                    Some(o) if *o != s.id => {
                        clash += 1;
                        if s.id < *o {
                            ids.insert(m.clone(), s.id.clone());
                        }
                    }
                    _ => {
                        ids.insert(m.clone(), s.id.clone());
                    }
                }
            }
        }
    }
    sizes.sort_unstable();
    let report = json!({
        "stations_with_trains": at.len(),
        "stops_without_app_station": unmatched.values().sum::<u32>(),
        "names_without_app_station": unmatched.len(),
        "unmatched": unmatched.iter().map(|(n, c)| json!([n, c])).collect::<Vec<_>>(),
        "stations_without_english_name": english_gaps,
        "stations_with_english_from_the_gazetteer": english_from_gazetteer,
        "complex_id_clashes": clash,
        "record_bytes": { "median": sizes.get(sizes.len() / 2), "p95": sizes.get(sizes.len() * 95 / 100), "max": sizes.last(), "total": sizes.iter().sum::<usize>() },
    });
    Out { records, ids, report }
}
