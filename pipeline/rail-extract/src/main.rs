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
//! `build` runs the station model, the stations under construction, the tile input, tippecanoe and the side files (was
//! stations.py, con_stations.py and build_tiles.py); --no-tiles writes side files only (regions drawn from a bigger region's tiles);
//! --combined NAME writes side files and tile input for every slug, then one tippecanoe run over all of them -> NAME.pmtiles and NAME.parts.json (no per-region PMTiles, no tile-join).
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
         rail-extract build <slug>... [--no-tiles | --combined NAME]      (cut ... --build: build straight after the cut)"
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
fn build_regions(root: &Path, slugs: &[String], tiles: bool, data: Option<&String>, out: Option<&String>, combined: bool) {
    let paths = site::Paths {
        data: data.map(PathBuf::from).unwrap_or_else(|| root.join("data")),
        config: root.join("pipeline/config"),
        out: out.map(PathBuf::from).unwrap_or_else(|| root.join("app/public/data")),
        regions: vec![root.join("pipeline/regions"), root.join("data/regions")],
    };
    let logs: Vec<(String, Vec<String>)> = slugs.par_iter().map(|s| (s.clone(), site::build(s, &paths, tiles))).collect();
    for (s, log) in logs {
        eprintln!("build {s}{}", if tiles { "" } else if combined { " (tile input for combined)" } else { " (side files only)" });
        for l in log {
            eprintln!("{l}");
        }
    }
    if tiles && PILOT_SOURCES.iter().any(|p| slugs.iter().any(|s| s == p)) {
        join_pilot(&paths.out);
    }
}

/// One tippecanoe run over the tile input (data/<slug>/tiles_tmp) that `build_regions` just wrote for every slug, same flags and layers as
/// the per-region run: <out>/<name>.pmtiles, plus <name>.parts.json (each slug and its outline's bounds [west, south, east, north]).
/// Replaces the per-region tippecanoe runs and the tile-join. A slug with no features is warned about and left out. Both outputs
/// are written to a temporary file and renamed on success.
fn build_combined(root: &Path, slugs: &[String], name: &str, data: Option<&String>, out: Option<&String>) {
    assert!(name != "pilot" && !slugs.iter().any(|s| s == name) && !name.contains(['/', '\\']), "bad --combined name {name:?}: not pilot, not a slug, no path separators");
    let data = data.map(PathBuf::from).unwrap_or_else(|| root.join("data"));
    let out = out.map(PathBuf::from).unwrap_or_else(|| root.join("app/public/data"));
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut parts = Vec::new();
    for slug in slugs {
        let tmp = data.join(slug).join("tiles_tmp");
        let empty = tiles::TILE_LAYERS.iter().all(|(_, f)| std::fs::metadata(tmp.join(f)).map_or(true, |m| m.len() == 0));
        if empty {
            eprintln!("warning: combined build skips {slug}: no features in {}", tmp.display());
            continue;
        }
        let outline = [root.join("pipeline/regions"), root.join("data/regions")]
            .iter()
            .map(|d| d.join(format!("{slug}.geojson")))
            .find(|p| p.exists())
            .unwrap_or_else(|| panic!("no outline {slug}.geojson"));
        // bounds come from the outlines (pilot.parts.json reads them from the PMTiles headers)
        let b = region::Region::load(slug, &outline).bbox; // min lat, min lon, max lat, max lon in 1e-7 degrees
        let deg = |v: i32| v as f64 / 1e7;
        parts.push(serde_json::json!({ "slug": slug, "bounds": [deg(b[1]), deg(b[0]), deg(b[3]), deg(b[2])] }));
        dirs.push(tmp);
    }
    assert!(!dirs.is_empty(), "combined build: no region has tile input");
    std::fs::create_dir_all(&out).expect("create output folder");
    // tippecanoe picks the format from the extension, so the temporary name keeps .pmtiles last
    let tmp = out.join(format!("{name}.tmp.pmtiles"));
    let t = Instant::now();
    let status = tiles::tippecanoe(&tmp, &dirs).status().expect("start tippecanoe (is it installed?)");
    assert!(status.success(), "tippecanoe failed for {name}");
    let tmp_json = out.join(format!("{name}.parts.tmp.json"));
    std::fs::write(&tmp_json, serde_json::to_string_pretty(&parts).unwrap() + "\n").expect("write parts.json");
    std::fs::rename(&tmp, out.join(format!("{name}.pmtiles"))).expect("rename pmtiles");
    std::fs::rename(&tmp_json, out.join(format!("{name}.parts.json"))).expect("rename parts.json");
    let mb = std::fs::metadata(out.join(format!("{name}.pmtiles"))).map_or(0.0, |m| m.len() as f64 / 1e6);
    eprintln!("combined {name}.pmtiles: {mb:.1} MB, tippecanoe {:.1} s", t.elapsed().as_secs_f64());
}

/// the tiled pilot sources the app draws from one file (app/src/regions.ts, TILES); the app learns each one's bounds from pilot.parts.json
const PILOT_SOURCES: [&str; 6] = ["sf-bay-area", "china", "de", "fr", "ch", "it"];

/// data bounds [west, south, east, north] from a PMTiles v3 header (127 bytes; min lon, min lat, max lon, max lat are i32 in 1e-7 degrees at 102, 106, 110, 114)
fn pmtiles_bounds(file: &Path) -> std::io::Result<[f64; 4]> {
    use std::io::Read;
    let mut h = [0u8; 127];
    std::fs::File::open(file)?.read_exact(&mut h)?;
    if &h[..7] != b"PMTiles" || h[7] != 3 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "not a PMTiles v3 file"));
    }
    let e7 = |o: usize| i32::from_le_bytes(h[o..o + 4].try_into().unwrap()) as f64 / 1e7;
    Ok([e7(102), e7(106), e7(110), e7(114)])
}

/// tile-join the pilot sources into pilot.pmtiles: one tile source (about 52 style layers instead of 155), and write pilot.parts.json
/// (each source's slug and bounds, so the app can tell which side files a feature belongs to). Joins the per-region files already in
/// `out`, so a partial rebuild refreshes the join; a missing file is warned about and skipped. -pk: no tile size limit; layers and attributes are all kept. Both outputs are written
/// to a temporary file and renamed on success.
fn join_pilot(out: &Path) {
    let t = Instant::now();
    let mut parts: Vec<(&str, [f64; 4], PathBuf)> = Vec::new();
    for s in PILOT_SOURCES {
        let f = out.join(format!("{s}.pmtiles"));
        match pmtiles_bounds(&f) {
            Ok(b) => parts.push((s, b, f)),
            Err(e) => eprintln!("warning: pilot join skipped: {}: {e}", f.display()),
        }
    }
    if parts.len() < PILOT_SOURCES.len() {
        return;
    }
    // bounds may overlap (neighbouring countries): the outlines are cut apart, so a feature is in one file, except the few ways that
    // cross a border (a handful per pair), which are drawn twice, identically; the app tells the files apart by outline (regions.ts partAt)
    // tile-join picks the format from the extension, so the temporary name keeps .pmtiles last
    let tmp = out.join("pilot.tmp.pmtiles");
    let status = std::process::Command::new("tile-join")
        .args(["-f", "-pk", "-q", "-o"])
        .arg(&tmp)
        .args(parts.iter().map(|p| &p.2))
        .stdout(std::process::Stdio::null())
        .status()
        .expect("start tile-join (comes with tippecanoe)");
    assert!(status.success(), "tile-join failed");
    let json: Vec<_> = parts.iter().map(|p| serde_json::json!({ "slug": p.0, "bounds": p.1 })).collect();
    let tmp_json = out.join("pilot.parts.tmp.json");
    std::fs::write(&tmp_json, serde_json::to_string_pretty(&json).unwrap() + "\n").expect("write pilot.parts.json");
    std::fs::rename(&tmp, out.join("pilot.pmtiles")).expect("rename pilot.pmtiles");
    std::fs::rename(&tmp_json, out.join("pilot.parts.json")).expect("rename pilot.parts.json");
    eprintln!("join pilot.pmtiles: {:.1} s", t.elapsed().as_secs_f64());
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
            if opt.contains_key("--combined") {
                usage(); // --combined belongs to `build`
            }
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
        "tiles-rs" => {
            if rest.is_empty() {
                usage();
            }
            let out = opt.get("--out").map(PathBuf::from).unwrap_or_else(|| root.join("data/tiles-rs"));
            let name = opt.get("--name").cloned().unwrap_or_else(|| rest.join("+"));
            tilewriter::run(&root, &rest, opt.get("--data"), &out, &name);
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
