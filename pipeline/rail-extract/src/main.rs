//! rail-extract: OSM planet -> world rail cache -> per-region cuts -> station model, side files and MLT tiles, for Rail Globe.
//!
//! Subcommands (each prints its own flags with `--help`; paths default to this repository, wherever the command runs from):
//!
//!   cache      read the planet once (about a minute) -> data/cache/<planet>.railcache: every rail candidate of the world with its coordinates
//!   outlines   country (`--level 2`), province / state (`--level 4`, 5) or city (`--cities WANTED.json`) outlines from the cache -> data/regions
//!   cut        per region outline (pipeline/regions/<slug>.geojson, else data/regions/<slug>.geojson): data/<slug>/{features.geojsonl,
//!              features.typed.geojsonl (rail types, classify.rs), relations.jsonl, stop_positions.jsonl, qa.json, airports.geojsonl, ...}, in about a second
//!   build      from a cut: station model, stations under construction, tile input, region statistics and side files, then
//!              `<out>/<slug>.pmtiles` through the Rust MLT writer (tilewriter.rs) unless `--no-tiles`; `cut ... --build` builds straight after the cut
//!   world      one PMTiles file (`world.pmtiles`) and `world.parts.json` over the tile input of many slugs, in memory-bounded passes, then `places`
//!   world-list which countries of a world cut have passenger rail -> kept.txt, dropped.txt
//!   places     city / town / country labels (places.json) and the search index (search.json) from the cache's place nodes
//!   pminfo     header, tiles and bytes per zoom of a PMTiles file
//!
//! A user mistake (no cut before a build, a missing outline, cache or config parent) is reported as `error: ...` with exit code 1.

// The pipeline passes tuples between stages and a few stages take many inputs; type aliases or parameter structs for these
// single-use shapes would add names without adding clarity.
#![allow(clippy::type_complexity, clippy::too_many_arguments)]

mod assemble;
mod build;
mod cache;
mod classify;
mod config;
mod construction;
mod cut;
mod out;
mod outlines;
mod places;
mod geomx;
mod pbf;
mod pminfo;
mod region;
mod rules;
mod site;
mod stations;
mod stats;
mod tilewriter;
mod tiles;
mod world_list;

use clap::{Args, Parser, Subcommand};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Print `error: msg` and exit with code 1 (a user mistake, not a bug).
pub fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1)
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap_or_else(|e| fail(format!("repository root: {e}")))
}

#[derive(Parser)]
#[command(name = "rail-extract", about = "OSM planet -> world rail cache -> region cuts -> station model, side files and MLT tiles (Rail Globe)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Args)]
struct Threads {
    /// worker threads (default: all cores)
    #[arg(long)]
    threads: Option<usize>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Read the planet once and keep every rail candidate of the world: data/cache/<planet>.railcache
    Cache {
        /// the planet file, e.g. planet-260928.osm.pbf
        #[arg(long)]
        planet: PathBuf,
        /// output file [default: data/cache/<planet name>.railcache]
        #[arg(long)]
        out: Option<PathBuf>,
        #[command(flatten)]
        t: Threads,
    },
    /// Cut each region's outline out of the cache: data/<slug>/ (features, relations, stop positions, rail types, ...)
    Cut {
        /// the rail cache [default: the newest .railcache in data/cache]
        #[arg(long)]
        cache: Option<PathBuf>,
        /// the only folder searched for <slug>.geojson [default: pipeline/regions, then data/regions]
        #[arg(long)]
        regions: Option<PathBuf>,
        /// output folder [default: data]
        #[arg(long)]
        data: Option<PathBuf>,
        /// exit with code 1 when a config exemption matched fewer features than its expect_min
        #[arg(long)]
        strict_exemptions: bool,
        /// build straight after the cut (like `rail-extract build <slug>...`)
        #[arg(long)]
        build: bool,
        /// with --build: side files only, no <slug>.pmtiles
        #[arg(long, requires = "build")]
        no_tiles: bool,
        #[command(flatten)]
        t: Threads,
        /// region slugs
        #[arg(required = true)]
        slugs: Vec<String>,
    },
    /// Write country (level 2), province / state (level 4, 5) or city outlines from the cache's admin boundaries
    Outlines {
        /// admin level, e.g. 2, 4 or 5
        #[arg(long, conflicts_with = "cities", required_unless_present = "cities")]
        level: Option<String>,
        /// only this country code (with --level)
        #[arg(long, conflicts_with = "cities")]
        country: Option<String>,
        /// city outlines for the places in WANTED.json (written by `places`)
        #[arg(long)]
        cities: Option<PathBuf>,
        /// the rail cache [default: the newest .railcache in data/cache]
        #[arg(long)]
        cache: Option<PathBuf>,
        /// output folder [default: data/regions]
        #[arg(long)]
        out: Option<PathBuf>,
        #[command(flatten)]
        t: Threads,
    },
    /// Station model, construction stations, tile input, statistics and side files for each cut region, then <slug>.pmtiles
    Build {
        /// skip the tile writer: side files only (regions drawn from a bigger region's tiles, or the world build)
        #[arg(long)]
        no_tiles: bool,
        /// folder of the cut [default: data]
        #[arg(long)]
        data: Option<PathBuf>,
        /// output folder for side files and <slug>.pmtiles [default: app/public/data]
        #[arg(long)]
        out: Option<PathBuf>,
        #[command(flatten)]
        t: Threads,
        /// region slugs (each needs a prior `cut`)
        #[arg(required = true)]
        slugs: Vec<String>,
    },
    /// One PMTiles file over the tile input of many slugs (memory-bounded passes), then places.json and search.json
    World {
        /// folder of the cut and build [default: data/world]
        #[arg(long)]
        data: Option<PathBuf>,
        /// output folder [default: app/public/data/world]
        #[arg(long)]
        out: Option<PathBuf>,
        /// output name: <name>.pmtiles and <name>.parts.json
        #[arg(long, default_value = "world")]
        name: String,
        /// zoom whose tiles are the unit of one pass (5 and 6 give the same file)
        #[arg(long, default_value_t = 6)]
        deep_zoom: u8,
        /// folder searched for outlines [default: data/regions]
        #[arg(long)]
        regions: Option<PathBuf>,
        /// the rail cache for places [default: the newest .railcache in data/cache]
        #[arg(long)]
        cache: Option<PathBuf>,
        /// file with slugs separated by white space
        #[arg(long)]
        slugs_file: Option<PathBuf>,
        #[command(flatten)]
        t: Threads,
        /// region slugs (or --slugs-file)
        #[arg(required_unless_present = "slugs_file")]
        slugs: Vec<String>,
    },
    /// Which countries of a world cut have passenger rail: kept.txt and dropped.txt
    WorldList {
        /// folder of the cut [default: data/world]
        #[arg(long)]
        data: Option<PathBuf>,
        /// least passenger route km to be kept
        #[arg(long, default_value_t = 1.0)]
        min_km: f64,
        /// file with slugs separated by white space
        #[arg(long)]
        slugs_file: Option<PathBuf>,
        /// region slugs (or --slugs-file)
        #[arg(required_unless_present = "slugs_file")]
        slugs: Vec<String>,
    },
    /// City, town and country labels (places.json) and the search index (search.json)
    Places {
        /// folder of the cut [default: data]
        #[arg(long)]
        data: Option<PathBuf>,
        /// output folder [default: app/public/data]
        #[arg(long)]
        out: Option<PathBuf>,
        /// the rail cache [default: the newest .railcache in data/cache]
        #[arg(long)]
        cache: Option<PathBuf>,
        #[command(flatten)]
        t: Threads,
        /// region slugs
        #[arg(required = true)]
        slugs: Vec<String>,
    },
    /// Header, tiles and bytes per zoom of a PMTiles file
    Pminfo {
        file: PathBuf,
        /// also write one row per tile size to this TSV file
        #[arg(long)]
        tsv: Option<PathBuf>,
        /// how many of the biggest tiles to list
        #[arg(long, default_value_t = 15)]
        top: usize,
    },
}

impl Cmd {
    fn threads(&self) -> Option<usize> {
        match self {
            Cmd::Cache { t, .. } | Cmd::Cut { t, .. } | Cmd::Outlines { t, .. } | Cmd::Build { t, .. } | Cmd::World { t, .. } | Cmd::Places { t, .. } => t.threads,
            Cmd::WorldList { .. } | Cmd::Pminfo { .. } => None,
        }
    }
}

/// the slugs given on the command line plus those in `--slugs-file` (white-space separated)
fn with_slugs_file(mut slugs: Vec<String>, file: Option<&Path>) -> Vec<String> {
    if let Some(f) = file {
        let text = std::fs::read_to_string(f).unwrap_or_else(|e| fail(format!("read {}: {e}", f.display())));
        slugs.extend(text.split_whitespace().map(String::from));
    }
    if slugs.is_empty() {
        fail("no slugs given (list them, or use --slugs-file with a non-empty file)");
    }
    slugs
}

/// the one .railcache in data/cache (the newest when there are several)
fn default_cache(root: &Path) -> PathBuf {
    let dir = root.join("data/cache");
    find_cache(root).unwrap_or_else(|| fail(format!("no .railcache in {}: run `rail-extract cache --planet <planet.osm.pbf>` first (or pass --cache)", dir.display())))
}

fn open_cache(path: &Path) -> cache::Cache {
    if !path.exists() {
        fail(format!("cache {} does not exist: run `rail-extract cache --planet <planet.osm.pbf>` first", path.display()));
    }
    cache::Cache::open(path).unwrap_or_else(|e| fail(format!("cache {}: {e}", path.display())))
}

fn find_cache(root: &Path) -> Option<PathBuf> {
    let dir = root.join("data/cache");
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "railcache"))
        .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
        .collect();
    v.sort();
    v.pop().map(|x| x.1)
}

/// city, town and country labels and the search index for `slugs` -> <out>/places.json, search.json (places.rs); the world build calls it last
fn run_places(root: &Path, data: &Path, out: &Path, cache: Option<PathBuf>, slugs: &[String]) {
    match cache.or_else(|| find_cache(root)) {
        Some(c) => places::run(data, out, &c, slugs, &[root.join("pipeline/regions"), root.join("data/regions")], &root.join("data/regions")),
        None => eprintln!("warning: no .railcache in data/cache: places.json and search.json not written (run `rail-extract cache`, then `rail-extract places`)"),
    }
}

/// the region's tile input (data/<slug>/tiles_tmp) has no features
fn no_tile_input(data: &Path, slug: &str) -> bool {
    let tmp = data.join(slug).join("tiles_tmp");
    tiles::TILE_LAYERS.iter().all(|(_, f)| std::fs::metadata(tmp.join(f)).map_or(true, |m| m.len() == 0))
}

/// data bounds [west, south, east, north] of a region from its outline, searching the given folders in order
fn outline_bounds_in(dirs: &[PathBuf], slug: &str) -> [f64; 4] {
    let outline = dirs
        .iter()
        .map(|d| d.join(format!("{slug}.geojson")))
        .find(|p| p.exists())
        .unwrap_or_else(|| fail(format!("no outline {slug}.geojson in {dirs:?}: run `rail-extract outlines` or add one")));
    let b = region::Region::load(slug, &outline).bbox; // min lat, min lon, max lat, max lon in 1e-7 degrees
    let deg = |v: i32| v as f64 / 1e7;
    [deg(b[1]), deg(b[0]), deg(b[3]), deg(b[2])]
}

/// stations, construction stations, tile input and side files for each region (in parallel); then, when `tiles` is on, the tile writer
/// for each region one after the other (it holds a whole region in memory) -> <out>/<slug>.pmtiles
fn build_regions(root: &Path, slugs: &[String], tiles: bool, data: Option<&PathBuf>, out: Option<&PathBuf>) {
    let paths = site::Paths {
        data: data.cloned().unwrap_or_else(|| root.join("data")),
        config: root.join("pipeline/config"),
        out: out.cloned().unwrap_or_else(|| root.join("app/public/data")),
        regions: vec![root.join("pipeline/regions"), root.join("data/regions")],
    };
    for s in slugs {
        if let Err(e) = site::preflight(s, &paths) {
            fail(e);
        }
    }
    let logs: Vec<(String, Vec<String>)> = slugs.par_iter().map(|s| (s.clone(), site::build(s, &paths))).collect();
    for (s, log) in logs {
        eprintln!("build {s}{}", if tiles { "" } else { " (side files only)" });
        for l in log {
            eprintln!("{l}");
        }
    }
    if !tiles {
        return;
    }
    for s in slugs {
        if no_tile_input(&paths.data, s) {
            eprintln!("warning: no tiles for {s}: no features in its tile input");
            continue;
        }
        tilewriter::run(&paths.data, std::slice::from_ref(s), &paths.out, s);
    }
}

fn main() {
    // a panic that is a user mistake caught late (a missing config parent, a broken outline) reads as `error: ...` with exit code 1;
    // RUST_BACKTRACE keeps the default hook (location and backtrace) for debugging
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        std::panic::set_hook(Box::new(|info| {
            let p = info.payload();
            let msg = p.downcast_ref::<String>().map(String::as_str).or_else(|| p.downcast_ref::<&str>().copied()).unwrap_or("internal error");
            fail(msg)
        }));
    }
    let cli = Cli::parse();
    let root = repo();
    if let Some(t) = cli.cmd.threads() {
        rayon::ThreadPoolBuilder::new().num_threads(t).build_global().unwrap_or_else(|e| fail(format!("--threads: {e}")));
    }

    match cli.cmd {
        Cmd::Cache { planet, out, .. } => {
            if !planet.exists() {
                fail(format!("planet file {} does not exist", planet.display()));
            }
            let out = out.unwrap_or_else(|| {
                let stem = planet.file_name().unwrap().to_string_lossy().trim_end_matches(".osm.pbf").to_string();
                root.join("data/cache").join(format!("{stem}.railcache"))
            });
            std::fs::create_dir_all(out.parent().unwrap()).unwrap_or_else(|e| fail(format!("create cache folder: {e}")));
            build::build(&planet, &out);
        }
        Cmd::Cut { cache, regions, data, strict_exemptions, build, no_tiles, slugs, .. } => {
            let t0 = Instant::now();
            let path = cache.unwrap_or_else(|| default_cache(&root));
            // hand-made outlines (pipeline/regions) first, then the ones `outlines` wrote (data/regions)
            let dirs: Vec<PathBuf> = match regions {
                Some(d) => vec![d],
                None => vec![root.join("pipeline/regions"), root.join("data/regions")],
            };
            let data_dir = data.clone().unwrap_or_else(|| root.join("data"));
            let outline_files: Vec<PathBuf> = slugs
                .iter()
                .map(|slug| {
                    dirs.iter()
                        .map(|d| d.join(format!("{slug}.geojson")))
                        .find(|p| p.exists())
                        .unwrap_or_else(|| fail(format!("no outline {slug}.geojson in {dirs:?}: run `rail-extract outlines` or add one")))
                })
                .collect();
            let c = open_cache(&path);
            eprintln!(
                "[{:6.2}s] cache {}: {} ways, {} tagged nodes, {} relations",
                t0.elapsed().as_secs_f64(),
                path.display(),
                c.way_idx().len(),
                c.node_idx().len(),
                c.rel_idx().len()
            );
            // regions are independent: cut them in parallel
            let sums: Vec<cut::Summary> = slugs
                .par_iter()
                .zip(outline_files.par_iter())
                .map(|(slug, path)| {
                    let reg = region::Region::load(slug, path);
                    // rail types need the region's rules: its own config, else the template
                    let (cfg_path, _) = site::config_path(&root.join("pipeline/config"), slug);
                    let cfg = Some(classify::Cfg::load(&cfg_path, &root.join("pipeline/regions")));
                    cut::cut(&c, &reg, &data_dir.join(slug), cfg.as_ref())
                })
                .collect();
            for s in &sums {
                eprintln!("  {}", s.line);
            }
            let stale: Vec<&String> = sums.iter().flat_map(|s| &s.stale).collect();
            for w in &stale {
                eprintln!("  WARNING: {w}");
            }
            if !stale.is_empty() && strict_exemptions {
                eprintln!("{} stale exemption(s) and --strict-exemptions: failing", stale.len());
                std::process::exit(1);
            }
            let n_default = slugs.iter().filter(|s| site::config_path(&root.join("pipeline/config"), s).1).count();
            if n_default > 0 {
                eprintln!("  {n_default} region(s) without their own pipeline/config/<slug>.json use the template pipeline/config/_default.json");
            }
            eprintln!("[{:6.2}s] cut", t0.elapsed().as_secs_f64());
            if build {
                build_regions(&root, &slugs, !no_tiles, data.as_ref(), None);
            }
            eprintln!("[{:6.2}s] done", t0.elapsed().as_secs_f64());
        }
        Cmd::Build { no_tiles, data, out, slugs, .. } => {
            let t0 = Instant::now();
            build_regions(&root, &slugs, !no_tiles, data.as_ref(), out.as_ref());
            eprintln!("[{:6.2}s] done", t0.elapsed().as_secs_f64());
        }
        Cmd::World { data, out, name, deep_zoom, regions, cache, slugs_file, slugs, .. } => {
            let slugs = with_slugs_file(slugs, slugs_file.as_deref());
            let data = data.unwrap_or_else(|| root.join("data/world"));
            let out = out.unwrap_or_else(|| root.join("app/public/data/world"));
            // not the same dirs as the other commands on purpose: the world cut reads data/regions only, and a hand-made outline in
            // pipeline/regions would change the bounds in world.parts.json
            let dirs = vec![regions.unwrap_or_else(|| root.join("data/regions"))];
            let parts: Vec<_> = slugs.iter().filter(|s| !no_tile_input(&data, s)).map(|s| serde_json::json!({ "slug": s, "bounds": outline_bounds_in(&dirs, s) })).collect();
            let used: Vec<String> = parts.iter().map(|p| p["slug"].as_str().unwrap().to_string()).collect();
            if used.is_empty() {
                fail(format!("no slug has tile input in {}: run `rail-extract build <slug>... --no-tiles --data {}` first", data.display(), data.display()));
            }
            let t = Instant::now();
            tilewriter::run_world(&data, &used, &out, &name, deep_zoom);
            std::fs::write(out.join(format!("{name}.parts.json")), serde_json::to_string_pretty(&parts).unwrap() + "\n").unwrap_or_else(|e| fail(format!("write parts.json: {e}")));
            eprintln!("world {name}: {} slugs, {:.1} s", used.len(), t.elapsed().as_secs_f64());
            run_places(&root, &data, &out, cache, &used);
        }
        Cmd::WorldList { data, min_km, slugs_file, slugs } => {
            let slugs = with_slugs_file(slugs, slugs_file.as_deref());
            let data = data.unwrap_or_else(|| root.join("data/world"));
            world_list::run(&data, &slugs, min_km);
        }
        Cmd::Places { data, out, cache, slugs, .. } => {
            let data = data.unwrap_or_else(|| root.join("data"));
            let out = out.unwrap_or_else(|| root.join("app/public/data"));
            let cache = cache.unwrap_or_else(|| default_cache(&root));
            if !cache.exists() {
                fail(format!("cache {} does not exist", cache.display()));
            }
            places::run(&data, &out, &cache, &slugs, &[root.join("pipeline/regions"), root.join("data/regions")], &root.join("data/regions"));
        }
        Cmd::Pminfo { file, tsv, top } => {
            if !file.exists() {
                fail(format!("{} does not exist", file.display()));
            }
            pminfo::run(&file, tsv.as_deref(), top);
        }
        Cmd::Outlines { level, country, cities, cache, out, .. } => {
            let path = cache.unwrap_or_else(|| default_cache(&root));
            let out = out.unwrap_or_else(|| root.join("data/regions"));
            let c = open_cache(&path);
            if let Some(wanted) = cities {
                if !wanted.exists() {
                    fail(format!("{} does not exist: `rail-extract places` writes it", wanted.display()));
                }
                outlines::city_outlines(&c, &wanted, &out);
            } else {
                outlines::outlines(&c, &level.unwrap(), country.as_deref(), &out);
            }
        }
    }
}
