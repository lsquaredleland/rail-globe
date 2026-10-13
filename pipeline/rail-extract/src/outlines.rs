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
fn inside(polys: &[Poly], lat: f64, lon: f64) -> bool {
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
fn interior_point(polys: &[Poly]) -> Option<(f64, f64)> {
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

pub fn outlines(c: &Cache, level: &str, country: Option<&str>, out: &Path) {
    let t0 = std::time::Instant::now();
    assert!(rules::ADMIN_LEVELS.contains(&level.as_bytes()), "--level must be one of 2, 4 (the levels in the cache)");
    let (all, failed) = load(c, &["2", level]);
    let countries: Vec<&Bnd> = all.iter().filter(|b| b.level == "2").collect();

    // the country of each boundary: its own code at level 2, the prefix of its ISO 3166-2 code ("CN-SC"), else the country
    // outline holding its interior point
    let country_of = |b: &Bnd| -> Option<String> {
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
    };
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

        let mut o = b"{\"type\":\"Feature\",\"properties\":{\"slug\":".to_vec();
        js(&mut o, slug.as_bytes());
        for (k, v) in [
            ("name", b.name.as_deref()),
            ("name_en", b.name_en.as_deref()),
            ("admin_level", Some(b.level.as_str())),
            ("country", cc.as_deref()),
            ("iso", if level == "2" { b.iso1.as_deref() } else { b.iso2.as_deref() }),
        ] {
            o.push(b',');
            js(&mut o, k.as_bytes());
            o.push(b':');
            js_opt(&mut o, v.map(str::as_bytes));
        }
        o.extend_from_slice(format!(",\"osm_id\":\"relation/{}\"}},\"geometry\":{{\"type\":\"MultiPolygon\",\"coordinates\":[", b.id).as_bytes());
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
