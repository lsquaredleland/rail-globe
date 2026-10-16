//! City, town and country labels for the map, and the search index, from the place nodes in the cache (no network).
//!
//!     rail-extract places [--data DIR] [--out DIR] [--cache FILE] (--parts FILE | <slug>...)
//!
//! Reads the station complexes of each region (<data>/<slug>/complexes.geojsonl) and writes to <out>:
//!   places.json  [[lon, lat, class, rank, name, name_en, n_stations], ...], class country | city | town (what the app draws)
//!   search.json  [[name, name_en, local_name, kind, lon, lat, minLon, minLat, maxLon, maxLat, zoom, slug, n_stations], ...]
//!
//! Rules (they are the ones places.py used with OpenFreeMap's tiles):
//!   - a city (place=city) is kept when a station complex lies within CITY_KM of its node, a town (place=town) within TOWN_KM;
//!     n_stations is the number of complexes within that distance; the same class, name and position (0.1 degree) is kept once;
//!   - a country gets a label when it is one of the regions' countries (the region's slug is the country's ISO code, or the
//!     country's label point lies inside the region's outline and the country is about as big as the region): at the
//!     centroid of its largest polygon (its interior point when the centroid is outside it), from the outline in data/regions;
//!   - names: `name` is the OSM name (local), `name_en` is name:en, else int_name, else name.
//! Rank (lower is more important; the map's layers split it at 3 / 7 and sort by it): from the place's population, one step
//! better for a national capital (capital=yes). Population >= 10 M 1, 5 M 2, 3 M 3, 1.5 M 4, 1 M 5, 500 k 6, 250 k 7,
//! 100 k 8, 50 k 9, 20 k 10, 10 k 11, else 12; a city with no population tag is 10, a town 12; a town is never better than 8.
//! (OpenFreeMap's rank was a place's order within its label-grid cell, so it cannot be reproduced; these steps give about
//! the same share of cities in each of the style's three bands: 0-3, 4-7, 8+.)
//! Countries: 0 for the biggest, up to 12 for the smallest, by log2 of the area relative to the biggest region's country.
//!
//! search.json also holds the countries, the provinces / states (admin level 4) and the cities (level 5, where outlines
//! exist) that contain a station complex, with the bounding box of their outline (only the polygons that are at least 5 % of the
//! largest one and hold a station complex: overseas territories and far islands do not stretch it) and a zoom that fits it. Cities and towns carry no box and a fixed zoom by rank.
//! `name` is name_en, else name; `name_en` and `local_name` are null when absent or the same as `name`.

use crate::assemble::Poly;
use crate::cache::Cache;
use crate::region::{rings_of, Region};
use rayon::prelude::*;
use rstar::{RTree, AABB};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const CITY_KM: f64 = 12.0;
pub const TOWN_KM: f64 = 5.0;

fn txt(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// polygons of a GeoJSON geometry, Feature or FeatureCollection, as rings of (lat, lon) in 1e-7 degrees
fn polys_of(v: &Value) -> Vec<Poly> {
    let ring = |r: &Value| -> Vec<(i32, i32)> {
        r.as_array().map_or(Vec::new(), |a| a.iter().map(|p| ((p[1].as_f64().unwrap_or(0.0) * 1e7).round() as i32, (p[0].as_f64().unwrap_or(0.0) * 1e7).round() as i32)).collect())
    };
    let poly = |p: &Value| -> Poly { p.as_array().map_or(Vec::new(), |a| a.iter().map(ring).collect()) };
    match v["type"].as_str() {
        Some("Polygon") => vec![poly(&v["coordinates"])],
        Some("MultiPolygon") => v["coordinates"].as_array().map_or(Vec::new(), |a| a.iter().map(poly).collect()),
        Some("Feature") => polys_of(&v["geometry"]),
        Some("FeatureCollection") => v["features"].as_array().map_or(Vec::new(), |a| a.iter().flat_map(polys_of).collect()),
        _ => Vec::new(),
    }
}

/// where a country's label goes: the centroid of its largest polygon, or an interior point of it when the centroid falls outside (lat, lon)
fn label_point(polys: &[Poly]) -> Option<(f64, f64)> {
    let big = polys.iter().filter(|p| p.first().map_or(false, |r| r.len() > 2)).max_by(|a, b| area(a).total_cmp(&area(b)))?;
    let r = &big[0];
    let (mut a2, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for w in r.windows(2) {
        let (y1, x1, y2, x2) = (w[0].0 as f64 * 1e-7, w[0].1 as f64 * 1e-7, w[1].0 as f64 * 1e-7, w[1].1 as f64 * 1e-7);
        let f = x1 * y2 - x2 * y1;
        a2 += f;
        cx += (x1 + x2) * f;
        cy += (y1 + y2) * f;
    }
    if a2.abs() > 1e-12 {
        let (lon, lat) = (cx / (3.0 * a2), cy / (3.0 * a2));
        if crate::outlines::inside(std::slice::from_ref(big), lat, lon) {
            return Some((lat, lon));
        }
    }
    crate::outlines::interior_point(std::slice::from_ref(big))
}

/// the area of a polygon's outer ring in square degrees, longitude scaled by cos(latitude)
fn area(p: &Poly) -> f64 {
    let r = &p[0];
    let lat0 = r.iter().map(|q| q.0 as f64).sum::<f64>() / r.len().max(1) as f64 * 1e-7;
    let k = lat0.to_radians().cos();
    (r.windows(2).map(|w| w[0].1 as f64 * w[1].0 as f64 - w[1].1 as f64 * w[0].0 as f64).sum::<f64>() * 0.5e-14 * k).abs()
}

/// bounding box [minLon, minLat, maxLon, maxLat] of the polygons that are at least 5 % of the largest and pass `keep`, and the total area of those
fn main_bbox(polys: &[Poly], keep: &dyn Fn(&Poly) -> bool) -> Option<([f64; 4], f64)> {
    let polys: Vec<&Poly> = polys.iter().filter(|p| p.first().map_or(false, |r| r.len() > 2)).collect();
    let areas: Vec<f64> = polys.iter().map(|p| area(p)).collect();
    let big = areas.iter().cloned().fold(0.0, f64::max);
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let mut total = 0.0;
    for (p, a) in polys.iter().zip(&areas) {
        if *a < big * 0.05 || !keep(p) {
            continue;
        }
        total += a;
        for q in &p[0] {
            let (lat, lon) = (q.0 as f64 * 1e-7, q.1 as f64 * 1e-7);
            b = [b[0].min(lon), b[1].min(lat), b[2].max(lon), b[3].max(lat)];
        }
    }
    (b[0] <= b[2]).then_some((b, total))
}

/// the web-map zoom at which a box fits a 1000 x 700 pixel map, in steps of 0.5, between 0 and 12
fn fit_zoom(b: &[f64; 4]) -> f64 {
    let y = |lat: f64| (std::f64::consts::FRAC_PI_4 + lat.clamp(-85.0, 85.0).to_radians() / 2.0).tan().ln().to_degrees();
    let (dx, dy) = ((b[2] - b[0]).max(1e-4), (y(b[3]) - y(b[1])).max(1e-4));
    let z = (1000.0 / 256.0 * 360.0 / dx).log2().min((700.0 / 256.0 * 360.0 / dy).log2());
    ((z * 2.0).floor() / 2.0).clamp(0.0, 12.0)
}

fn population(t: &str) -> Option<u64> {
    let first = t.split(';').next()?;
    let d: String = first.chars().filter(|c| c.is_ascii_digit()).collect();
    d.parse().ok().filter(|&p| p > 0)
}

fn rank(city: bool, pop: Option<u64>, capital: bool) -> i64 {
    const STEPS: [(u64, i64); 11] = [
        (10_000_000, 1), (5_000_000, 2), (3_000_000, 3), (1_500_000, 4), (1_000_000, 5), (500_000, 6), (250_000, 7), (100_000, 8), (50_000, 9),
        (20_000, 10), (10_000, 11),
    ];
    let mut r = match pop {
        Some(p) => STEPS.iter().find(|(min, _)| p >= *min).map_or(12, |s| s.1),
        None => if city { 10 } else { 12 },
    };
    if capital {
        r = (r - 1).max(1);
    }
    if city { r } else { r.max(8) }
}

struct Place {
    city: bool,
    lon: f64,
    lat: f64,
    rank: i64,
    name: String,
    en: String,
    n: usize,
}

struct Outline {
    slug: String,
    name: Option<String>,
    name_en: Option<String>,
    level: String,
    osm_id: i64,
}

fn load_outline(dir: &Path, slug: &str) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{slug}.geojson"))).ok()?).ok()
}

fn write_json(path: &Path, v: &Value) {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string(v).expect("json")).unwrap_or_else(|e| panic!("write {}: {e}", tmp.display()));
    std::fs::rename(&tmp, path).unwrap_or_else(|e| panic!("rename {}: {e}", path.display()));
}

fn r3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Writes <out>/places.json and <out>/search.json for the regions `slugs` (station complexes in <data>/<slug>/complexes.geojsonl).
/// `region_dirs` hold the regions' own outlines (hand-made first); `outline_dir` holds the generated ones and index.json (data/regions).
pub fn run(data: &Path, out: &Path, cache_path: &Path, slugs: &[String], region_dirs: &[PathBuf], outline_dir: &Path) {
    let t0 = Instant::now();
    let lap = |what: &str| eprintln!("places [{:5.2}s] {what}", t0.elapsed().as_secs_f64());
    let c = Cache::open(cache_path).unwrap_or_else(|e| panic!("{e}"));

    // ---- stations: every complex of every region
    let per: Vec<Vec<[f64; 2]>> = slugs
        .par_iter()
        .map(|s| {
            let Ok(text) = std::fs::read_to_string(data.join(s).join("complexes.geojsonl")) else {
                eprintln!("places: no complexes.geojsonl for {s} (build it first); its stations are not counted");
                return Vec::new();
            };
            text.lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| {
                    let v: Value = serde_json::from_str(l).ok()?;
                    let p = &v["geometry"]["coordinates"];
                    Some([p[0].as_f64()?, p[1].as_f64()?])
                })
                .collect()
        })
        .collect();
    let stations: Vec<[f64; 2]> = per.into_iter().flatten().collect();
    let tree = RTree::bulk_load(stations.clone());
    lap(&format!("{} station complexes in {} regions", stations.len(), slugs.len()));
    // stations within `km` of a point (the distance places.py used: longitude scaled by cos(latitude), 111.2 km per degree)
    let n_near = |lon: f64, lat: f64, km: f64| -> usize {
        let k = lat.to_radians().cos().max(0.01);
        let (dy, dx) = (km / 111.2, km / (111.2 * k));
        tree.locate_in_envelope(AABB::from_corners([lon - dx, lat - dy], [lon + dx, lat + dy]))
            .filter(|s| ((s[0] - lon) * k).hypot(s[1] - lat) * 111.2 <= km)
            .count()
    };

    // a polygon (rings of (lat, lon) in 1e-7 degrees) that holds at least one station complex
    let has_station = |p: &Poly| -> bool {
        let (mut b, rings) = ([i32::MAX, i32::MAX, i32::MIN, i32::MIN], p.iter().map(|r| r.iter().map(|q| (q.1 as i64, q.0 as i64)).collect::<Vec<_>>()).collect::<Vec<_>>());
        for q in &p[0] {
            b = [b[0].min(q.0), b[1].min(q.1), b[2].max(q.0), b[3].max(q.1)];
        }
        let (lo, hi) = ([b[1] as f64 * 1e-7, b[0] as f64 * 1e-7], [b[3] as f64 * 1e-7, b[2] as f64 * 1e-7]);
        let mut near = tree.locate_in_envelope(AABB::from_corners(lo, hi)).peekable();
        near.peek().is_some() && {
            let reg = Region::new("", rings);
            near.any(|s| reg.contains((s[1] * 1e7).round() as i32, (s[0] * 1e7).round() as i32))
        }
    };

    // ---- cities and towns
    let np = c.place_idx().len();
    let found: Vec<(i64, Option<Place>)> = (0..np)
        .into_par_iter()
        .map(|i| {
            let ni = c.place_idx()[i];
            let (lat, lon) = (ni.lat as f64 * 1e-7, ni.lon as f64 * 1e-7);
            let tags = crate::rules::List(c.place_tags(i));
            use crate::rules::Tags;
            let city = tags.get(b"place") == Some(b"city");
            let km = if city { CITY_KM } else { TOWN_KM };
            let n = n_near(lon, lat, km);
            if n == 0 {
                return (ni.id, None);
            }
            let name = tags.get(b"name").filter(|v| !v.is_empty()).map(txt).or(tags.get(b"name:en").map(txt));
            let Some(name) = name else { return (ni.id, None) };
            let en = crate::rules::english_name(&tags).map(txt).unwrap_or_else(|| name.clone());
            let pop = tags.get(b"population").and_then(|p| population(&txt(p)));
            let capital = tags.get(b"capital") == Some(b"yes");
            (ni.id, Some(Place { city, lon, lat, rank: rank(city, pop, capital), name, en, n }))
        })
        .collect();
    let mut seen = HashSet::new();
    let mut places: Vec<Place> = Vec::new();
    for (_, p) in found {
        if let Some(p) = p {
            if seen.insert((p.city, p.name.clone(), (p.lon * 10.0).round() as i64, (p.lat * 10.0).round() as i64)) {
                places.push(p);
            }
        }
    }
    places.sort_by(|a, b| (!a.city, a.rank, &a.name).cmp(&(!b.city, b.rank, &b.name)));
    lap(&format!(
        "{np} place nodes (city, town) in the cache: {} cities and {} towns have a station",
        places.iter().filter(|p| p.city).count(),
        places.iter().filter(|p| !p.city).count()
    ));

    // ---- outlines: the index of data/regions, and the relation boxes of the cache
    let index: Value = std::fs::read_to_string(outline_dir.join("index.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| panic!("no {}/index.json: run `rail-extract outlines --level 2` and `--level 4` first", outline_dir.display()));
    let outlines: Vec<Outline> = index
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(slug, v)| Outline {
                    slug: slug.clone(),
                    name: v["name"].as_str().map(String::from),
                    name_en: v["name_en"].as_str().map(String::from),
                    level: v["admin_level"].as_str().unwrap_or("").to_string(),
                    osm_id: v["osm_id"].as_str().and_then(|s| s.trim_start_matches("relation/").parse().ok()).unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default();
    let bbox_of: BTreeMap<i64, [i32; 4]> = c.brel_idx().iter().map(|r| (r.id, r.bbox)).collect();
    let display = |o: &Outline| o.name_en.clone().or(o.name.clone()).unwrap_or_else(|| o.slug.clone());

    // ---- countries: those of the regions
    let is_country = |s: &str| outlines.iter().any(|o| o.slug == s && o.level == "2");
    let mut wanted: BTreeMap<String, String> = BTreeMap::new(); // country slug -> region slug (the search entry's slug)
    for s in slugs.iter().filter(|s| is_country(s)) {
        wanted.insert(s.clone(), s.clone());
    }
    let others: Vec<&String> = slugs.iter().filter(|s| !is_country(s)).collect();
    if !others.is_empty() {
        let l2: Vec<(&Outline, Vec<Poly>)> = outlines
            .par_iter()
            .filter(|o| o.level == "2")
            .filter_map(|o| Some((o, polys_of(&load_outline(outline_dir, &o.slug)?))))
            .collect();
        for s in others {
            let Some(path) = region_dirs.iter().map(|d| d.join(format!("{s}.geojson"))).find(|p| p.exists()) else { continue };
            let reg = Region::load(s, &path);
            let b = reg.bbox;
            let reg_area = (b[2] - b[0]) as f64 * (b[3] - b[1]) as f64 * 1e-14;
            for (o, polys) in &l2 {
                let Some(pt) = label_point(polys) else { continue };
                let Some((cb, _)) = main_bbox(polys, &|_| true) else { continue };
                let c_area = (cb[3] - cb[1]) * (cb[2] - cb[0]);
                if reg.contains((pt.0 * 1e7) as i32, (pt.1 * 1e7) as i32) && c_area >= 0.5 * reg_area {
                    wanted.entry(o.slug.clone()).or_insert_with(|| s.clone());
                }
            }
        }
    }
    struct Country {
        region: String,
        name: String,
        en: Option<String>,
        local: Option<String>,
        pt: (f64, f64), // lon, lat
        bbox: [f64; 4],
        area: f64,
    }
    let mut countries: Vec<Country> = wanted
        .iter()
        .filter_map(|(cs, rs)| {
            let o = outlines.iter().find(|o| &o.slug == cs)?;
            let polys = polys_of(&load_outline(outline_dir, cs)?);
            let (bbox, area) = main_bbox(&polys, &has_station).or_else(|| main_bbox(&polys, &|_| true))?;
            let (lat, lon) = label_point(&polys)?;
            Some(Country { region: rs.clone(), name: display(o), en: o.name_en.clone(), local: o.name.clone(), pt: (lon, lat), bbox, area })
        })
        .collect();
    countries.sort_by(|a, b| b.area.total_cmp(&a.area));
    let biggest = countries.first().map_or(1.0, |c| c.area);
    lap(&format!("{} countries", countries.len()));

    // ---- provinces / states (level 4) and cities (level 5) that contain a station
    struct Area {
        o: usize,
        pt: (f64, f64),
        bbox: [f64; 4],
        n: usize,
    }
    let areas: Vec<Area> = (0..outlines.len())
        .into_par_iter()
        .filter(|&i| outlines[i].level == "4" || outlines[i].level == "5")
        .filter_map(|i| {
            let o = &outlines[i];
            let b = bbox_of.get(&o.osm_id)?; // min lat, min lon, max lat, max lon
            let (lo, hi) = ([b[1] as f64 * 1e-7, b[0] as f64 * 1e-7], [b[3] as f64 * 1e-7, b[2] as f64 * 1e-7]);
            if tree.locate_in_envelope(AABB::from_corners(lo, hi)).next().is_none() {
                return None;
            }
            let v = load_outline(outline_dir, &o.slug)?;
            let reg = Region::new(&o.slug, rings_of(&v));
            let n = tree.locate_in_envelope(AABB::from_corners(lo, hi)).filter(|s| reg.contains((s[1] * 1e7).round() as i32, (s[0] * 1e7).round() as i32)).count();
            if n == 0 {
                return None;
            }
            let polys = polys_of(&v);
            let (bbox, _) = main_bbox(&polys, &has_station).or_else(|| main_bbox(&polys, &|_| true))?;
            let (lat, lon) = label_point(&polys)?;
            Some(Area { o: i, pt: (lon, lat), bbox, n })
        })
        .collect();
    let n_prov = areas.iter().filter(|a| outlines[a.o].level == "4").count();
    lap(&format!("{n_prov} provinces / states and {} level-5 cities with a station", areas.len() - n_prov));

    // ---- places.json
    let mut rows: Vec<Value> = Vec::new();
    for ct in &countries {
        let rk = ((biggest / ct.area.max(1e-9)).log2().round() as i64).clamp(0, 12);
        rows.push(json!([r3(ct.pt.0), r3(ct.pt.1), "country", rk, ct.local.clone().unwrap_or(ct.name.clone()), ct.name, 0]));
    }
    for p in &places {
        rows.push(json!([r3(p.lon), r3(p.lat), if p.city { "city" } else { "town" }, p.rank, p.name, p.en, p.n]));
    }
    std::fs::create_dir_all(out).expect("create output folder");
    write_json(&out.join("places.json"), &Value::Array(rows));

    // ---- search.json
    let b3 = |v: f64| json!(r3(v));
    let mut s: Vec<Value> = Vec::new();
    let local = |l: &Option<String>, name: &str| l.clone().filter(|l| l != name);
    for ct in &countries {
        // the stations of a country's regions are not counted per country: n is 0 (the map shows no count for a country either)
        s.push(json!([ct.name, ct.en, local(&ct.local, &ct.name), "country", b3(ct.pt.0), b3(ct.pt.1), b3(ct.bbox[0]), b3(ct.bbox[1]), b3(ct.bbox[2]), b3(ct.bbox[3]), fit_zoom(&ct.bbox), ct.region, 0]));
    }
    let mut prov: Vec<&Area> = areas.iter().collect();
    prov.sort_by(|a, b| (&outlines[a.o].level, std::cmp::Reverse(a.n)).cmp(&(&outlines[b.o].level, std::cmp::Reverse(b.n))));
    for a in prov {
        let o = &outlines[a.o];
        let name = display(o);
        s.push(json!([name, o.name_en, local(&o.name, &name), "province", b3(a.pt.0), b3(a.pt.1), b3(a.bbox[0]), b3(a.bbox[1]), b3(a.bbox[2]), b3(a.bbox[3]), fit_zoom(&a.bbox), o.slug, a.n]));
    }
    for p in &places {
        let zoom = if !p.city { 12.0 } else if p.rank <= 3 { 10.0 } else if p.rank <= 7 { 11.0 } else { 11.5 };
        let kind = if p.city { "city" } else { "town" };
        let en = (p.en != p.name).then(|| p.en.clone());
        s.push(json!([p.en, en, local(&Some(p.name.clone()), &p.en), kind, r3(p.lon), r3(p.lat), null, null, null, null, zoom, null, p.n]));
    }
    write_json(&out.join("search.json"), &Value::Array(s));
    let size = |f: &str| std::fs::metadata(out.join(f)).map_or(0, |m| m.len());
    lap(&format!("wrote {}/places.json ({} bytes) and search.json ({} bytes)", out.display(), size("places.json"), size("search.json")));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn population_tag() {
        assert_eq!(population("1 234 567"), Some(1_234_567));
        assert_eq!(population("12,345"), Some(12_345));
        assert_eq!(population("8000;9000"), Some(8000));
        assert_eq!(population("many"), None);
        assert_eq!(population("0"), None);
    }

    #[test]
    fn rank_steps_capital_and_town_floor() {
        assert_eq!(rank(true, Some(12_000_000), false), 1);
        assert_eq!(rank(true, Some(12_000_000), true), 1); // never better than 1
        assert_eq!(rank(true, Some(130_000), false), 8);
        assert_eq!(rank(true, Some(130_000), true), 7);
        assert_eq!(rank(true, None, false), 10);
        assert_eq!(rank(false, None, false), 12);
        assert_eq!(rank(false, Some(900_000), false), 8); // a town is never better than 8
        assert_eq!(rank(true, Some(3_000), false), 12);
    }

    #[test]
    fn zoom_fits_the_box() {
        assert_eq!(fit_zoom(&[0.0, 0.0, 360.0, 1.0]), 1.5);
        assert!(fit_zoom(&[2.0, 48.0, 2.5, 48.3]) >= 8.0);
        assert_eq!(fit_zoom(&[0.0, 0.0, 0.0001, 0.0001]), 12.0);
    }

    #[test]
    fn label_point_is_inside_a_crescent() {
        // a C-shaped polygon: its centroid lies in the gap, so the label falls back to an interior point
        let c = [(0, 0), (0, 30), (10, 30), (10, 10), (20, 10), (20, 30), (30, 30), (30, 0), (0, 0)];
        let poly: Poly = vec![c.iter().map(|&(lat, lon)| (lat * 10_000_000, lon * 10_000_000)).collect()];
        let (lat, lon) = label_point(&[poly.clone()]).unwrap();
        assert!(crate::outlines::inside(&[poly], lat, lon));
    }
}
