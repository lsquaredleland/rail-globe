//! rail-extract: OSM planet -> world rail cache -> per-region filter output for Rail Globe.
//!
//!     rail-extract cache --planet planet-260928.osm.pbf [--out data/cache/planet-260928.railcache] [--threads N]
//!     rail-extract cut [--cache data/cache/<name>.railcache] [--regions pipeline/regions] [--data data] <slug>...
//!     rail-extract outlines --level 2|4 [--country CC] [--out data/regions]
//!     rail-extract build <slug>... [--no-tiles | --combined NAME]       (or cut ... --build)
//!
//! `cache` reads the planet once (about a minute) and keeps every rail candidate of the world with its coordinates.
//! `cut` writes data/<slug>/{features.geojsonl, relations.jsonl, stop_positions.jsonl, qa.json, airports.geojsonl} for each region
//! outline (pipeline/regions/<slug>.geojson, hand-made, else data/regions/<slug>.geojson), in about a second; the cut also classifies rail types (classify.rs); stations.py and the later steps read those files.
//! `build` runs the station model, the stations under construction, the tile input, the Rust MLT tile writer (tilewriter.rs) and the side files (was
//! stations.py, con_stations.py and build_tiles.py); --no-tiles writes side files only (regions drawn from a bigger region's tiles);
//! --combined NAME writes side files and tile input for every slug, then one tile writer run over all of them -> NAME.pmtiles and NAME.parts.json (no per-region PMTiles).
//! Whenever a pilot source was built with tiles, pilot.pmtiles and pilot.parts.json are rewritten from the pilot sources' tile input.
//!     rail-extract tiles <slug>... [--name NAME] [--out DIR]       the tile writer alone over existing tile input (default out data/tiles)
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
mod stats;
mod tilewriter;
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
         rail-extract build <slug>... [--no-tiles | --combined NAME]      (cut ... --build: build straight after the cut)\n  \
         rail-extract tiles <slug>... [--name NAME] [--out DIR] [--data DIR]"
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

/// the region's tile input (data/<slug>/tiles_tmp) has no features
fn no_tile_input(data: &Path, slug: &str) -> bool {
    let tmp = data.join(slug).join("tiles_tmp");
    tiles::TILE_LAYERS.iter().all(|(_, f)| std::fs::metadata(tmp.join(f)).map_or(true, |m| m.len() == 0))
}

/// data bounds [west, south, east, north] of a region from its outline (hand-made first, else data/regions)
fn outline_bounds(root: &Path, slug: &str) -> [f64; 4] {
    let outline = [root.join("pipeline/regions"), root.join("data/regions")]
        .iter()
        .map(|d| d.join(format!("{slug}.geojson")))
        .find(|p| p.exists())
        .unwrap_or_else(|| panic!("no outline {slug}.geojson"));
    let b = region::Region::load(slug, &outline).bbox; // min lat, min lon, max lat, max lon in 1e-7 degrees
    let deg = |v: i32| v as f64 / 1e7;
    [deg(b[1]), deg(b[0]), deg(b[3]), deg(b[2])]
}

/// stations, construction stations, tile input and side files for each region (in parallel); then, when `tiles` is on, the tile writer
/// for each region one after the other (it holds a whole region in memory) -> <out>/<slug>.pmtiles (not for pilot sources when the join runs), and the pilot join when a pilot source was built
fn build_regions(root: &Path, slugs: &[String], tiles: bool, data: Option<&String>, out: Option<&String>, combined: bool) {
    let paths = site::Paths {
        data: data.map(PathBuf::from).unwrap_or_else(|| root.join("data")),
        config: root.join("pipeline/config"),
        out: out.map(PathBuf::from).unwrap_or_else(|| root.join("app/public/data")),
        regions: vec![root.join("pipeline/regions"), root.join("data/regions")],
    };
    let logs: Vec<(String, Vec<String>)> = slugs.par_iter().map(|s| (s.clone(), site::build(s, &paths))).collect();
    for (s, log) in logs {
        eprintln!("build {s}{}", if tiles { "" } else if combined { " (tile input for combined)" } else { " (side files only)" });
        for l in log {
            eprintln!("{l}");
        }
    }
    if !tiles {
        return;
    }
    // a pilot source is drawn from pilot.pmtiles only (nothing in the app reads <slug>.pmtiles), so its own file is skipped when the join will run
    let pilot_built = PILOT_SOURCES.iter().any(|p| slugs.iter().any(|s| s == p));
    let missing_input = PILOT_SOURCES.iter().find(|s| no_tile_input(&paths.data, s));
    let join = pilot_built && missing_input.is_none();
    for s in slugs {
        if join && PILOT_SOURCES.contains(&s.as_str()) {
            continue;
        }
        if no_tile_input(&paths.data, s) {
            eprintln!("warning: no tiles for {s}: no features in its tile input");
            continue;
        }
        tilewriter::run(&paths.data, std::slice::from_ref(s), &paths.out, s);
    }
    if join {
        join_pilot(root, &paths.data, &paths.out);
    } else if let (true, Some(s)) = (pilot_built, missing_input) {
        eprintln!("warning: pilot join skipped: no tile input for {s}");
    }
}

/// One tile writer run over the tile input (data/<slug>/tiles_tmp) that `build_regions` just wrote for every slug:
/// <out>/<name>.pmtiles, plus <name>.parts.json (each slug and its outline's bounds [west, south, east, north]).
/// A slug with no features is warned about and left out. Both outputs are written to a temporary file and renamed on success.
fn build_combined(root: &Path, slugs: &[String], name: &str, data: Option<&String>, out: Option<&String>) {
    assert!(name != "pilot" && !slugs.iter().any(|s| s == name) && !name.contains(['/', '\\']), "bad --combined name {name:?}: not pilot, not a slug, no path separators");
    let data = data.map(PathBuf::from).unwrap_or_else(|| root.join("data"));
    let out = out.map(PathBuf::from).unwrap_or_else(|| root.join("app/public/data"));
    let mut used: Vec<String> = Vec::new();
    let mut parts = Vec::new();
    for slug in slugs {
        if no_tile_input(&data, slug) {
            eprintln!("warning: combined build skips {slug}: no features in its tile input");
            continue;
        }
        parts.push(serde_json::json!({ "slug": slug, "bounds": outline_bounds(root, slug) }));
        used.push(slug.clone());
    }
    assert!(!used.is_empty(), "combined build: no region has tile input");
    write_joined(&data, &used, &out, name, &parts);
}

/// the tile writer over `slugs` -> <out>/<name>.pmtiles and <name>.parts.json (both through a temporary file and a rename)
fn write_joined(data: &Path, slugs: &[String], out: &Path, name: &str, parts: &[serde_json::Value]) {
    let t = Instant::now();
    std::fs::create_dir_all(out).expect("create output folder");
    let _ = std::fs::remove_file(out.join(format!("{name}.tmp.pmtiles")));
    let tmp_json = out.join(format!("{name}.parts.tmp.json"));
    tilewriter::run(data, slugs, out, name);
    // written after the tile writer returns, so a writer panic leaves no parts.tmp.json behind
    std::fs::write(&tmp_json, serde_json::to_string_pretty(parts).unwrap() + "\n").expect("write parts.json");
    std::fs::rename(&tmp_json, out.join(format!("{name}.parts.json"))).expect("rename parts.json");
    let mb = std::fs::metadata(out.join(format!("{name}.pmtiles"))).map_or(0.0, |m| m.len() as f64 / 1e6);
    eprintln!("combined {name}.pmtiles: {mb:.1} MB, {:.1} s", t.elapsed().as_secs_f64());
}

/// the tiled pilot sources the app draws from one file (app/src/regions.ts, TILES); the app learns each one's bounds from pilot.parts.json
const PILOT_SOURCES: [&str; 6] = ["sf-bay-area", "china", "de", "fr", "ch", "it"];

/// One tile writer run over the pilot sources' tile input -> pilot.pmtiles: one tile source (about 52 style layers instead of 155), and
/// pilot.parts.json (each source's slug and outline bounds, so the app can tell which side files a feature belongs to). Reads the tile
/// input every earlier `build` left in data/<slug>/tiles_tmp, so a partial rebuild refreshes the join (the caller has checked that every source has tile input).
/// Bounds may overlap (neighbouring countries): the outlines are cut apart, so a feature is in one source, except the few ways that
/// cross a border (a handful per pair), which are drawn twice, identically; the app tells the sources apart by outline (regions.ts partAt).
fn join_pilot(root: &Path, data: &Path, out: &Path) {
    let slugs: Vec<String> = PILOT_SOURCES.iter().map(|s| s.to_string()).collect();
    let parts: Vec<_> = slugs.iter().map(|s| serde_json::json!({ "slug": s, "bounds": outline_bounds(root, s) })).collect();
    write_joined(data, &slugs, out, "pilot", &parts);
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
            k @ ("--planet" | "--out" | "--cache" | "--regions" | "--data" | "--threads" | "--level" | "--country" | "--combined" | "--name") => {
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
            if rest.is_empty() || opt.contains_key("--combined") {
                usage(); // --combined belongs to `build`
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
                build_regions(&root, &rest, !opt.contains_key("--no-tiles"), opt.get("--data"), None, false);
            }
            eprintln!("[{:6.2}s] done", t0.elapsed().as_secs_f64());
        }
        "build" => {
            if rest.is_empty() {
                usage();
            }
            let t0 = Instant::now();
            match opt.get("--combined") {
                Some(name) => {
                    assert!(!opt.contains_key("--no-tiles"), "--combined and --no-tiles do not go together");
                    build_regions(&root, &rest, false, opt.get("--data"), opt.get("--out"), true);
                    build_combined(&root, &rest, name, opt.get("--data"), opt.get("--out"));
                }
                None => build_regions(&root, &rest, !opt.contains_key("--no-tiles"), opt.get("--data"), opt.get("--out"), false),
            }
            eprintln!("[{:6.2}s] done", t0.elapsed().as_secs_f64());
        }
        "tiles" => {
            if rest.is_empty() {
                usage();
            }
            let out = opt.get("--out").map(PathBuf::from).unwrap_or_else(|| root.join("data/tiles"));
            let name = opt.get("--name").cloned().unwrap_or_else(|| rest.join("+"));
            let data = opt.get("--data").map(PathBuf::from).unwrap_or_else(|| root.join("data"));
            tilewriter::run(&data, &rest, &out, &name);
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
