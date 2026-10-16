//! Region outlines from the admin boundaries in the cache (levels 2 and 4: countries, provinces / states).
//!
//!     rail-extract outlines --level 4 --country CN      -> data/regions/cn-sichuan.geojson, cn-guangdong.geojson, ...
//!     rail-extract outlines --level 2                   -> data/regions/cn.geojson, fr.geojson, ...
//!
//! Every outline comes from the same planet as the rail data, simplified once when the cache was built (about 20 m), and keeps
//! its boundary relation id. A level-4 boundary belongs to the country whose outline holds an interior point of it.
//! Slugs: countries by ISO code ("cn", "us"; by English name when they have none); provinces and states by country code and English name
//! ("cn-sichuan", "us-california"). data/regions/index.json lists every outline written, for picking regions to compare.

use crate::assemble::{self, Poly};
use crate::cache::{bbox_add, Cache, BBox, EMPTY};
use crate::out::*;
use crate::rules::{self, List, Tags};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::path::Path;

struct Bnd {
    id: i64,
    level: String,
    name: Option<String>,
    name_en: Option<String>,
    iso1: Option<String>, // ISO3166-1 alpha-2 (countries)
    iso2: Option<String>, // ISO3166-2 (provinces / states)
    wikidata: Option<String>,
    polys: Vec<Poly>,
    bbox: BBox,
    gaps: Vec<f64>, // gaps closed while assembling, in degrees
}

/// loose ring ends closer than this are joined (about 1 km): a broken border in the planet, or a vertex simplified away
const MAX_GAP_DEG: f64 = 0.01;

fn txt(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

enum Loaded {
    Skip,
    Failed(String), // "name (relation/id): reason"
    Ok(Bnd),
}

/// boundaries of the given levels, assembled; also the ones that did not assemble
fn load(c: &Cache, levels: &[&str]) -> (Vec<Bnd>, Vec<String>) {
    let res: Vec<Loaded> = (0..c.brel_idx().len())
        .into_par_iter()
        .map(|i| {
            let (tags, mem) = c.brel(i);
            let t = List(tags);
            let Some(level) = t.get(b"admin_level").map(txt) else { return Loaded::Skip };
            if !levels.contains(&level.as_str()) {
                return Loaded::Skip;
            }
            let what = |why: &str| {
                Loaded::Failed(format!(
                    "{} {} (relation/{}): {why}",
                    t.get(b"ISO3166-2").or(t.get(b"ISO3166-1")).map(txt).unwrap_or_default(),
                    rules::english_name(&t).or(t.get(b"name")).map(txt).unwrap_or_default(),
                    c.brel_idx()[i].id
                ))
            };
            let Some(geoms) = mem.iter().map(|(id, _)| c.bway(*id)).collect::<Option<Vec<_>>>() else {
                return what("a member way is missing from the planet");
            };
            let refs: Vec<&[(i32, i32)]> = geoms.iter().map(|g| &g[..]).collect();
            let (polys, gaps) = match assemble::assemble_outline(&refs, MAX_GAP_DEG) {
                Ok(p) if !p.0.is_empty() => p,
                _ => return what("rings do not close, even joining loose ends within about 1 km"),
            };
            let mut bbox = EMPTY;
            for p in &polys {
                for q in &p[0] {
                    bbox_add(&mut bbox, q.0, q.1);
                }
            }
            Loaded::Ok(Bnd {
                id: c.brel_idx()[i].id,
                level,
                name: t.get(b"name").map(txt),
                name_en: rules::english_name(&t).map(txt),
                iso1: t.get(b"ISO3166-1:alpha2").or(t.get(b"ISO3166-1")).map(txt),
                iso2: t.get(b"ISO3166-2").map(txt),
                wikidata: t.get(b"wikidata").map(txt),
                polys,
                bbox,
                gaps,
            })
        })
        .collect();
    let mut v = Vec::new();
    let mut failed = Vec::new();
    for r in res {
        match r {
            Loaded::Ok(b) => v.push(b),
            Loaded::Failed(f) => failed.push(f),
            Loaded::Skip => {}
        }
    }
    (v, failed)
}

/// even-odd test of a point (degrees) against every ring of a set of polygons
pub fn inside(polys: &[Poly], lat: f64, lon: f64) -> bool {
    let mut c = false;
    for p in polys {
        for r in p {
            for w in r.windows(2) {
                let (y1, x1, y2, x2) = (w[0].0 as f64 * 1e-7, w[0].1 as f64 * 1e-7, w[1].0 as f64 * 1e-7, w[1].1 as f64 * 1e-7);
                if (y1 > lat) != (y2 > lat) && lon < x1 + (lat - y1) * (x2 - x1) / (y2 - y1) {
                    c = !c;
                }
            }
        }
    }
    c
}

/// a point inside the largest polygon: the middle of the widest inside span on the horizontal line through its middle
pub fn interior_point(polys: &[Poly]) -> Option<(f64, f64)> {
    let area = |r: &[(i32, i32)]| -> f64 {
        r.windows(2).map(|w| w[0].1 as f64 * w[1].0 as f64 - w[1].1 as f64 * w[0].0 as f64).sum::<f64>().abs()
    };
    let p = polys.iter().max_by(|a, b| area(&a[0]).total_cmp(&area(&b[0])))?;
    let (lo, hi) = p[0].iter().fold((i32::MAX, i32::MIN), |(lo, hi), q| (lo.min(q.0), hi.max(q.0)));
    let lat = (lo as f64 + hi as f64) / 2.0 * 1e-7 + 1.234e-9; // off any vertex
    let mut xs: Vec<f64> = Vec::new();
    for r in p {
        for w in r.windows(2) {
            let (y1, x1, y2, x2) = (w[0].0 as f64 * 1e-7, w[0].1 as f64 * 1e-7, w[1].0 as f64 * 1e-7, w[1].1 as f64 * 1e-7);
            if (y1 > lat) != (y2 > lat) {
                xs.push(x1 + (lat - y1) * (x2 - x1) / (y2 - y1));
            }
        }
    }
    xs.sort_by(|a, b| a.total_cmp(b));
    xs.chunks_exact(2).max_by(|a, b| (a[1] - a[0]).total_cmp(&(b[1] - b[0]))).map(|s| (lat, (s[0] + s[1]) / 2.0))
}

/// lower-case ASCII slug; common Latin accents folded ("Île-de-France" -> "ile-de-france")
pub fn slugify(s: &str) -> String {
    let fold = |c: char| -> Option<char> {
        Some(match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' => 'a',
            'ç' | 'ć' | 'č' | 'Ç' | 'Č' => 'c',
            'ď' | 'đ' => 'd',
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' | 'È' | 'É' | 'Ê' | 'Ë' => 'e',
            'ğ' | 'ģ' => 'g',
            'ì' | 'í' | 'î' | 'ï' | 'ī' | 'ı' | 'Ì' | 'Í' | 'Î' | 'Ï' | 'İ' => 'i',
            'ł' | 'ľ' | 'ļ' | 'Ł' => 'l',
            'ñ' | 'ń' | 'ň' | 'ņ' | 'Ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => 'o',
            'ř' | 'Ř' => 'r',
            'ś' | 'š' | 'ş' | 'ș' | 'Ś' | 'Š' | 'Ş' | 'Ș' => 's',
            'ť' | 'ţ' | 'ț' => 't',
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' | 'Ù' | 'Ú' | 'Û' | 'Ü' => 'u',
            'ý' | 'ÿ' | 'Ý' => 'y',
            'ź' | 'ż' | 'ž' | 'Ź' | 'Ż' | 'Ž' => 'z',
            c if c.is_ascii_alphanumeric() => c.to_ascii_lowercase(),
            _ => return None,
        })
    };
    let mut out = String::new();
    for c in s.chars() {
        match fold(c) {
            Some(c) => out.push(c),
            None => {
                if !out.is_empty() && !out.ends_with('-') {
                    out.push('-');
                }
            }
        }
    }
    out.trim_end_matches('-').to_string()
}


/// the GeoJSON Feature of one outline (same layout for every level); `city` marks a city outline (index.json `city: true`)
fn feature_bytes(slug: &str, b: &Bnd, cc: Option<&str>, iso: Option<&str>, city: bool) -> Vec<u8> {
    let mut o = b"{\"type\":\"Feature\",\"properties\":{\"slug\":".to_vec();
        js(&mut o, slug.as_bytes());
        for (k, v) in [
            ("name", b.name.as_deref()),
            ("name_en", b.name_en.as_deref()),
            ("admin_level", Some(b.level.as_str())),
            ("country", cc.as_deref()),
            ("iso", iso),
        ] {
            o.push(b',');
            js(&mut o, k.as_bytes());
            o.push(b':');
            js_opt(&mut o, v.map(str::as_bytes));
        }
        o.extend_from_slice(format!(",\"osm_id\":\"relation/{}\"{}}},\"geometry\":{{\"type\":\"MultiPolygon\",\"coordinates\":[", b.id, if city { ",\"city\":true" } else { "" }).as_bytes());
        for (i, poly) in b.polys.iter().enumerate() {
            o.extend_from_slice(if i > 0 { b",[" } else { b"[" });
            for (j, ring) in poly.iter().enumerate() {
                o.extend_from_slice(if j > 0 { b",[" } else { b"[" });
                for (k, q) in ring.iter().enumerate() {
                    if k > 0 {
                        o.push(b',');
                    }
                    pt(&mut o, q.0, q.1);
                }
                o.push(b']');
            }
            o.push(b']');
        }
        o.extend_from_slice(b"]}}\n");
    o
}

/// the country of a boundary: its own code at level 2, the prefix of its ISO 3166-2 code ("CN-SC"), else the country outline holding its interior point
fn country_of(b: &Bnd, countries: &[&Bnd]) -> Option<String> {
    if b.level == "2" {
        return b.iso1.clone();
    }
    if let Some(cc) = b.iso2.as_deref().and_then(|c| c.split_once('-')).map(|(cc, _)| cc).filter(|cc| cc.len() == 2) {
        return Some(cc.to_ascii_uppercase());
    }
    let (lat, lon) = interior_point(&b.polys)?;
    let (la, lo) = ((lat * 1e7) as i32, (lon * 1e7) as i32);
    countries
        .iter()
        .filter(|k| k.bbox[0] <= la && la <= k.bbox[2] && k.bbox[1] <= lo && lo <= k.bbox[3])
        .find(|k| inside(&k.polys, lat, lon))
        .and_then(|k| k.iso1.clone())
}

/// area in km2 (planar, longitude scaled by cos(latitude) at the polygon's middle), holes subtracted
fn area_km2(polys: &[Poly]) -> f64 {
    let ring = |r: &[(i32, i32)]| -> f64 {
        let lat0 = (r.iter().map(|q| q.0 as f64).sum::<f64>() / r.len().max(1) as f64 * 1e-7).to_radians().cos();
        r.windows(2).map(|w| (w[0].1 as f64 * 1e-7 * lat0) * (w[1].0 as f64 * 1e-7) - (w[1].1 as f64 * 1e-7 * lat0) * (w[0].0 as f64 * 1e-7)).sum::<f64>().abs() / 2.0 * 111.32 * 110.54
    };
    polys.iter().map(|p| ring(&p[0]) - p[1..].iter().map(|h| ring(h)).sum::<f64>()).sum()
}

/// City outlines from the cache (`outlines --cities FILE`): the boundary relation (admin levels 5 to 9, the highest wins) whose Wikidata id
/// is one of the cities in FILE (`city-outlines-wanted.json`, written by `places`), with its country found like a province's. Written
/// like the other outlines, with `city: true` in index.json (the real admin_level stays). Polygons under 2 km2 or over 20,000 km2 are
/// skipped. An index entry with the same relation (an earlier run, or the fetch script) is replaced. Needs a cache built with pass 0
/// (the cities' boundaries are only in the cache then).
pub fn city_outlines(c: &Cache, wanted_file: &Path, out: &Path) {
    let t0 = std::time::Instant::now();
    let wanted: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(wanted_file).unwrap_or_else(|e| panic!("read {}: {e} (run `rail-extract places` first)", wanted_file.display())))
        .unwrap_or_else(|e| panic!("parse {}: {e}", wanted_file.display()));
    let want: BTreeMap<&str, &str> = wanted.iter().filter_map(|w| Some((w["wikidata"].as_str()?, w["name_en"].as_str().unwrap_or("")))).collect();
    let (all, failed) = load(c, &["2", "5", "6", "7", "8", "9"]);
    let countries: Vec<&Bnd> = all.iter().filter(|b| b.level == "2").collect();
    let mut best: BTreeMap<&str, &Bnd> = BTreeMap::new();
    for b in all.iter().filter(|b| b.level != "2") {
        if let Some(w) = b.wikidata.as_deref().filter(|w| want.contains_key(w)) {
            let lvl = |x: &Bnd| x.level.parse::<u32>().unwrap_or(0);
            if best.get(w).map_or(true, |o| lvl(b) > lvl(o)) {
                best.insert(w, b);
            }
        }
    }
    std::fs::create_dir_all(out).expect("create outline folder");
    let idx_path = out.join("index.json");
    let mut index: BTreeMap<String, serde_json::Value> = std::fs::read_to_string(&idx_path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let (mut made, mut too_small, mut too_big, mut already, mut no_country) = (0, 0, 0, 0, 0);
    let mut bytes = 0usize;
    let mut todo: Vec<(&&str, &&Bnd)> = best.iter().collect();
    todo.sort_by_key(|(_, b)| b.id);
    for (w, b) in todo {
        let osm = format!("relation/{}", b.id);
        let same: Vec<String> = index.iter().filter(|(_, v)| v["osm_id"].as_str() == Some(osm.as_str())).map(|(k, _)| k.clone()).collect();
        if same.iter().any(|k| index[k]["city"] != true) {
            already += 1; // a province or a level-5 outline already (Shanghai, Chengdu)
            continue;
        }
        let area = area_km2(&b.polys);
        if area < 2.0 {
            too_small += 1;
            continue;
        }
        if area > 20_000.0 {
            too_big += 1;
            continue;
        }
        let Some(cc) = country_of(b, &countries) else {
            no_country += 1;
            continue;
        };
        for k in &same {
            index.remove(k);
            let _ = std::fs::remove_file(out.join(format!("{k}.geojson")));
        }
        let label = b.name_en.clone().or(b.name.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| want[*w].to_string());
        let base = format!("{}-{}", cc.to_ascii_lowercase(), slugify(&label));
        let mut slug = base.clone();
        let mut n = 1;
        while index.contains_key(&slug) {
            n += 1; // New York is also a state; Sofia has a province and a city-province
            slug = format!("{base}-city{}", if n == 2 { String::new() } else { (n - 1).to_string() });
        }
        let o = feature_bytes(&slug, b, Some(&cc), None, true);
        std::fs::write(out.join(format!("{slug}.geojson")), &o).expect("write outline");
        bytes += o.len();
        index.insert(
            slug,
            serde_json::json!({ "name": b.name, "name_en": b.name_en, "admin_level": b.level, "country": cc, "iso": null, "osm_id": osm, "city": true }),
        );
        made += 1;
    }
    std::fs::write(&idx_path, serde_json::to_string_pretty(&index).expect("index json")).expect("write index");
    let missing = want.keys().filter(|w| !best.contains_key(*w)).count();
    eprintln!(
        "{made} city outlines -> {} ({:.1} MB, {:.2}s); {} cities asked for: {missing} with no boundary of level 5 to 9 in the cache, {already} already an outline, {too_small} under 2 km2, {too_big} over 20,000 km2, {no_country} with no country; {} boundaries did not assemble",
        out.display(),
        bytes as f64 / 1e6,
        t0.elapsed().as_secs_f64(),
        want.len(),
        failed.len()
    );
}

pub fn outlines(c: &Cache, level: &str, country: Option<&str>, out: &Path) {
    let t0 = std::time::Instant::now();
    assert!(rules::ADMIN_LEVELS.contains(&level.as_bytes()), "--level must be one of 2, 4, 5 (the levels in the cache)");
    let (all, failed) = load(c, &["2", level]);
    let countries: Vec<&Bnd> = all.iter().filter(|b| b.level == "2").collect();

    let country_of = |b: &Bnd| country_of(b, &countries);
    let want = country.map(|c| c.to_ascii_uppercase());
    let mut picked: Vec<(&Bnd, Option<String>)> = all
        .par_iter()
        .filter(|b| b.level == level)
        .map(|b| (b, country_of(b)))
        .filter(|(_, cc)| want.as_ref().map_or(true, |w| cc.as_deref().map(str::to_ascii_uppercase).as_ref() == Some(w)))
        .collect();
    picked.sort_by_key(|(b, _)| b.id);

    std::fs::create_dir_all(out).expect("create outline folder");
    let idx_path = out.join("index.json");
    let mut index: BTreeMap<String, serde_json::Value> = std::fs::read_to_string(&idx_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let mut used: BTreeMap<String, i64> = index
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), v["osm_id"].as_str()?.trim_start_matches("relation/").parse().ok()?)))
        .collect();
    let mut bytes = 0usize;
    let mut written = Vec::new();
    for (b, cc) in &picked {
        let label = b.name_en.clone().or(b.name.clone()).unwrap_or_default();
        // countries by ISO code ("cn", matching province slugs "cn-sichuan"), else by name
        let iso_slug = if level == "2" { b.iso1.as_deref().filter(|c| c.len() == 2).map(str::to_ascii_lowercase) } else { None };
        let mut base = iso_slug.unwrap_or_else(|| slugify(&label));
        if base.is_empty() {
            base = b.iso2.as_deref().or(b.iso1.as_deref()).map(slugify).unwrap_or_default();
        }
        if base.is_empty() {
            base = format!("r{}", b.id);
        }
        if level != "2" {
            base = format!("{}-{base}", cc.as_deref().map(|c| c.to_ascii_lowercase()).unwrap_or_else(|| "xx".into()));
        }
        // the same relation keeps its slug; another relation with the same name gets its id appended
        let slug = match used.get(&base) {
            Some(&id) if id != b.id => format!("{base}-{}", b.id),
            _ => base,
        };
        used.insert(slug.clone(), b.id);

        let o = feature_bytes(&slug, b, cc.as_deref(), if level == "2" { b.iso1.as_deref() } else { b.iso2.as_deref() }, false);
        std::fs::write(out.join(format!("{slug}.geojson")), &o).expect("write outline");
        bytes += o.len();
        index.insert(
            slug.clone(),
            serde_json::json!({
                "name": b.name, "name_en": b.name_en, "admin_level": b.level, "country": cc,
                "iso": if level == "2" { &b.iso1 } else { &b.iso2 }, "osm_id": format!("relation/{}", b.id),
            }),
        );
        written.push(slug);
    }
    std::fs::write(&idx_path, serde_json::to_string_pretty(&index).expect("index json")).expect("write index");
    let no_country = picked.iter().filter(|(_, cc)| cc.is_none()).count();
    eprintln!(
        "{} level-{level} outlines{} -> {} ({:.1} MB, {:.2}s); {} level-2/{level} boundaries in the cache did not assemble{}",
        written.len(),
        want.map(|w| format!(" in {w}")).unwrap_or_default(),
        out.display(),
        bytes as f64 / 1e6,
        t0.elapsed().as_secs_f64(),
        failed.len(),
        if no_country > 0 { format!("; {no_country} with no country found") } else { String::new() }
    );
    if written.len() <= 60 {
        eprintln!("  {}", written.join(" "));
    }
    for f in &failed {
        eprintln!("  not assembled: {f}");
    }
    for (b, _) in picked.iter().filter(|(b, _)| !b.gaps.is_empty()) {
        let worst = b.gaps.iter().cloned().fold(0.0, f64::max);
        eprintln!(
            "  joined {} gap(s) in {} (relation/{}), largest about {:.0} m",
            b.gaps.len(),
            b.name_en.as_deref().or(b.name.as_deref()).unwrap_or(""),
            b.id,
            worst * 111_000.0
        );
    }
    for (b, _) in picked.iter().filter(|(_, cc)| cc.is_none()) {
        eprintln!("  no country: {} (relation/{})", b.name_en.as_deref().or(b.name.as_deref()).unwrap_or(""), b.id);
    }
}
