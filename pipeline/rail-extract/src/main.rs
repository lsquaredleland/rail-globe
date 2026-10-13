//! rail-extract: OSM planet -> world rail cache -> per-region filter output for Rail Globe.
//!
//!     rail-extract cache --planet planet-260928.osm.pbf [--out data/cache/planet-260928.railcache] [--threads N]
//!     rail-extract cut [--cache data/cache/<name>.railcache] [--regions pipeline/regions] [--data data] <slug>...
//!     rail-extract outlines --level 2|4 [--country CC] [--out data/regions]
//!     rail-extract build <slug>... [--no-tiles]       (or cut ... --build)
//!
//! `cache` reads the planet once (about a minute) and keeps every rail candidate of the world with its coordinates.
//! `cut` writes data/<slug>/{features.geojsonl, relations.jsonl, stop_positions.jsonl, qa.json, airports.geojsonl} for each region
//! outline (pipeline/regions/<slug>.geojson, hand-made, else data/regions/<slug>.geojson), in about a second; the cut also classifies rail types (classify.rs); stations.py and the later steps read those files.
//! `build` runs the station model, the stations under construction, the tile input, tippecanoe and the side files (was
//! stations.py, con_stations.py and build_tiles.py); --no-tiles writes side files only (regions drawn from a bigger region's tiles).
//! `outlines` writes country (level 2) or province / state (level 4) outlines from the admin boundaries in the cache.
//! Paths default to this repository, wherever the command runs from.

mod assemble;
mod build;
mod cache;
mod classify;
mod construction;
mod cut;
mod out;
mod outlines;
mod geomx;
mod pbf;
mod pyjson;
mod region;
mod rules;
mod site;
mod stations;
mod tiles;

use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repository root")
}

fn usage() -> ! {
    eprintln!(
        "usage:\n  rail-extract cache --planet PLANET.osm.pbf [--out data/cache/<planet>.railcache] [--threads N]\n  \
         rail-extract cut [--cache FILE] [--regions DIR] [--data DIR] [--threads N] <slug>...\n  \
         rail-extract outlines --level 2|4 [--country CC] [--cache FILE] [--out data/regions]\n  \
         rail-extract build <slug>... [--no-tiles]      (cut ... --build: build straight after the cut)"
    );
    std::process::exit(2)
}

/// the one .railcache in data/cache (the newest when there are several)
fn default_cache(root: &Path) -> PathBuf {
    let dir = root.join("data/cache");
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|_| panic!("no cache folder {}: run `rail-extract cache` first", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |x| x == "railcache"))
        .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
        .collect();
    v.sort();
    v.pop().map(|x| x.1).unwrap_or_else(|| panic!("no .railcache in {}: run `rail-extract cache` first", dir.display()))
}

/// stations, construction stations, tile input, tippecanoe and side files for each region (in parallel)
fn build_regions(root: &Path, slugs: &[String], tiles: bool, data: Option<&String>, out: Option<&String>) {
    let paths = site::Paths {
        data: data.map(PathBuf::from).unwrap_or_else(|| root.join("data")),
        config: root.join("pipeline/config"),
        out: out.map(PathBuf::from).unwrap_or_else(|| root.join("app/public/data")),
        regions: vec![root.join("pipeline/regions"), root.join("data/regions")],
    };
    let logs: Vec<(String, Vec<String>)> = slugs.par_iter().map(|s| (s.clone(), site::build(s, &paths, tiles))).collect();
    for (s, log) in logs {
        eprintln!("build {s}{}", if tiles { "" } else { " (side files only)" });
        for l in log {
            eprintln!("{l}");
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else { usage() };
    let root = repo();
    let mut opt: std::collections::HashMap<&str, String> = Default::default();
    let mut rest = Vec::new();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            k @ ("--planet" | "--out" | "--cache" | "--regions" | "--data" | "--threads" | "--level" | "--country") => {
                opt.insert(k, args.get(i + 1).cloned().unwrap_or_else(|| usage()));
                i += 2;
            }
            "-h" | "--help" => usage(),
            k @ ("--build" | "--no-tiles") => {
                opt.insert(k, String::new());
                i += 1;
            }
            s if s.starts_with("--") => usage(),
            s => {
                rest.push(s.to_string());
                i += 1;
            }
        }
    }
    if let Some(t) = opt.get("--threads") {
        rayon::ThreadPoolBuilder::new().num_threads(t.parse().expect("--threads")).build_global().expect("thread pool");
    }

    match cmd.as_str() {
        "cache" => {
            let planet = PathBuf::from(opt.get("--planet").unwrap_or_else(|| usage()));
            let out = opt.get("--out").map(PathBuf::from).unwrap_or_else(|| {
                let stem = planet.file_name().unwrap().to_string_lossy().trim_end_matches(".osm.pbf").to_string();
                root.join("data/cache").join(format!("{stem}.railcache"))
            });
            std::fs::create_dir_all(out.parent().unwrap()).expect("create cache folder");
            build::build(&planet, &out);
        }
        "cut" => {
            if rest.is_empty() {
                usage();
            }
            let t0 = Instant::now();
            let path = opt.get("--cache").map(PathBuf::from).unwrap_or_else(|| default_cache(&root));
            // hand-made outlines (pipeline/regions) first, then the ones `outlines` wrote (data/regions)
            let dirs: Vec<PathBuf> = match opt.get("--regions") {
                Some(d) => vec![PathBuf::from(d)],
                None => vec![root.join("pipeline/regions"), root.join("data/regions")],
            };
            let data = opt.get("--data").map(PathBuf::from).unwrap_or_else(|| root.join("data"));
            let c = cache::Cache::open(&path).unwrap_or_else(|e| panic!("{e}"));
            eprintln!(
                "[{:6.2}s] cache {}: {} ways, {} tagged nodes, {} relations",
                t0.elapsed().as_secs_f64(),
                path.display(),
                c.way_idx().len(),
                c.node_idx().len(),
                c.rel_idx().len()
            );
            // regions are independent: cut them in parallel
            let lines: Vec<String> = rest
                .par_iter()
                .map(|slug| {
                    let path = dirs
                        .iter()
                        .map(|d| d.join(format!("{slug}.geojson")))
                        .find(|p| p.exists())
                        .unwrap_or_else(|| panic!("no outline {slug}.geojson in {dirs:?}: run `rail-extract outlines` or add one"));
                    let reg = region::Region::load(slug, &path);
                    // rail types need the region's rules; without them the cut still writes its files
                    let cfg_path = root.join("pipeline/config").join(format!("{slug}.json"));
                    let cfg = cfg_path.exists().then(|| classify::Cfg::load(&cfg_path, &root.join("pipeline/regions")));
                    cut::cut(&c, &reg, &data.join(slug), cfg.as_ref()).line
                })
                .collect();
            for l in lines {
                eprintln!("  {l}");
            }
            eprintln!("[{:6.2}s] cut", t0.elapsed().as_secs_f64());
            if opt.contains_key("--build") {
                build_regions(&root, &rest, !opt.contains_key("--no-tiles"), opt.get("--data"), None);
            }
            eprintln!("[{:6.2}s] done", t0.elapsed().as_secs_f64());
        }
        "build" => {
            if rest.is_empty() {
                usage();
            }
            let t0 = Instant::now();
            build_regions(&root, &rest, !opt.contains_key("--no-tiles"), opt.get("--data"), opt.get("--out"));
            eprintln!("[{:6.2}s] done", t0.elapsed().as_secs_f64());
        }
        "outlines" => {
            let level = opt.get("--level").cloned().unwrap_or_else(|| usage());
            let path = opt.get("--cache").map(PathBuf::from).unwrap_or_else(|| default_cache(&root));
            let out = opt.get("--out").map(PathBuf::from).unwrap_or_else(|| root.join("data/regions"));
            let c = cache::Cache::open(&path).unwrap_or_else(|e| panic!("{e}"));
            outlines::outlines(&c, &level, opt.get("--country").map(String::as_str), &out);
        }
        _ => usage(),
    }
}
