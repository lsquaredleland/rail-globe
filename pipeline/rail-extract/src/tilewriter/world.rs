//! World mode: the same slicing, coalescing and encoding as `run`, but memory-bounded (see the comment below).

use super::{
    encode_buckets, gather_tiles, grow, layer_names, merc, slice_group, slice_jobs, tile_metadata, tiny, write_pmtiles, Kind, Piece, TileKey, MAX_ZOOM,
};
use geojson::Feature;
use pmtiles::{TileCoord, TileId};
use rayon::prelude::*;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::tiles::TILE_LAYERS;

// ---------------------------------------------------------------------------------------------------------------------------------
// World mode (`rail-extract world`): the same slicing, coalescing and encoding as `run`, but memory-bounded.
//
// `run` holds a whole region's features and every piece of every tile in memory (China 5.3 GB; the pilot 9-14 GB), so the world
// cannot go through it in one run. Here the tile input is only indexed first (byte range, zoom range and bounding box of every
// feature in the memory-mapped files; column types, layer zoom ranges and bounds of the whole input), then the tiles are made in
// passes. A pass makes the tiles of zooms zmin..=zmax inside one tile P (zoom pz) of the world: it parses only the features whose
// bounding box (with the 80/4096 clip buffer) meets P and whose zoom range meets zmin..=zmax, slices them, keeps the tiles inside P and
// encodes them straight into a spool file; the pass's memory is then freed. The spool is sorted by tile id into the PMTiles file at the end.
//   pass A: the world, z0..=3          pass B: each populated z4 tile, z4..=deep-1          pass C: each populated tile at zoom `deep`, deep..=14
// Features are sliced independently and ordered by input order within a tile, so a tile has the same pieces as in `run`; the only
// difference is which features share a slicer (see slice_group: a tile with no points is not descended), which can only matter for pieces
// smaller than the simplification tolerance.

#[derive(Clone, Copy)]
struct Ent {
    file: u32,
    off: u32,
    len: u32,
    li: u8,
    lo: u8,
    hi: u8,
    /// x0, y0, x1, y1 on the Web Mercator unit square (y down); x0 > x1 for a feature with no geometry
    bb: [f32; 4],
}

struct Acc {
    kinds: Vec<BTreeMap<String, Kind>>,
    bounds: [f64; 4],
    zooms: Vec<(u8, u8)>,
    count: Vec<u64>,
}

/// bytes of the feature at a position of the mapped tile input
fn ent_bytes<'a>(files: &'a [memmap2::Mmap], e: &Ent) -> &'a [u8] {
    &files[e.file as usize][e.off as usize..(e.off + e.len) as usize]
}

/// unit-square rectangle of tile (z, x, y) grown by `m` tiles on each side
fn tile_rect(z: u8, x: u32, y: u32, m: f64) -> [f64; 4] {
    let n = f64::from(1u32 << z);
    [(f64::from(x) - m) / n, (f64::from(y) - m) / n, (f64::from(x) + 1.0 + m) / n, (f64::from(y) + 1.0 + m) / n]
}

fn meets(bb: &[f32; 4], r: &[f64; 4]) -> bool {
    f64::from(bb[0]) <= r[2] && f64::from(bb[2]) >= r[0] && f64::from(bb[1]) <= r[3] && f64::from(bb[3]) >= r[1]
}

/// tiles at zoom z whose rectangle grown by the clip buffer meets the box
fn cover(bb: &[f32; 4], z: u8, mut f: impl FnMut(u32, u32)) {
    let n = f64::from(1u32 << z);
    let m = BUFFER_TILES;
    let hi = (1u32 << z) - 1;
    let c = |v: f64| (v.floor().max(0.0) as u32).min(hi);
    let (x0, x1) = (c(f64::from(bb[0]) * n - m), c(f64::from(bb[2]) * n + m));
    let (y0, y1) = (c(f64::from(bb[1]) * n - m), c(f64::from(bb[3]) * n + m));
    for x in x0..=x1 {
        for y in y0..=y1 {
            f(x, y);
        }
    }
}

/// the clip buffer (80 of 4096) plus a margin for the f32 bounding boxes
const BUFFER_TILES: f64 = 0.03;

struct Spool {
    w: std::io::BufWriter<std::fs::File>,
    pos: u64,
    index: Vec<(u64, u64, u32)>, // tile id, offset, length
}

fn rss_mb() -> u64 {
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok())
        .map_or(0, |kb| kb / 1024)
}

/// One pass: the tiles of zooms zmin..=zmax inside tile `part` (z, x, y) -> spool. Returns (features read, tiles written).
fn world_pass(
    files: &[memmap2::Mmap],
    ents: &[Ent],
    layer_names: &[&str],
    kinds: &[BTreeMap<String, Kind>],
    part: TileKey,
    zmin: u8,
    zmax: u8,
    spool: &mut Spool,
) -> (usize, usize) {
    let (pz, px, py) = part;
    let rect = tile_rect(pz, px, py, BUFFER_TILES);
    let cand: Vec<usize> = ents
        .par_iter()
        .enumerate()
        .filter(|(_, e)| e.lo <= zmax && e.hi >= zmin && e.bb[0] <= e.bb[2] && (pz == 0 || meets(&e.bb, &rect)))
        .map(|(i, _)| i)
        .collect();
    if cand.is_empty() {
        return (0, 0);
    }
    let parsed: Vec<(Feature, bool)> = cand
        .par_iter()
        .map(|&i| {
            let e = &ents[i];
            let f: Feature = serde_json::from_slice(ent_bytes(files, e)).expect("tile input feature (parsed before)");
            let small = f.geometry.as_ref().is_some_and(|g| tiny(&g.value, e.lo));
            (f, small)
        })
        .collect();
    let mut props: Vec<Vec<Arc<Map<String, Value>>>> = vec![Vec::new(); layer_names.len()];
    let mut groups: BTreeMap<(usize, u8, u8, bool), Vec<Feature>> = BTreeMap::new();
    for (&i, (mut f, small)) in cand.iter().zip(parsed) {
        let e = &ents[i];
        let li = e.li as usize;
        f.foreign_members = None;
        f.id = Some(geojson::feature::Id::Number((props[li].len() as u64).into()));
        props[li].push(Arc::new(f.properties.take().unwrap_or_default()));
        groups.entry((li, e.lo.max(zmin), e.hi.min(zmax), small)).or_default().push(f);
    }
    let sliced: Vec<(usize, Vec<(TileKey, Vec<Piece>)>)> =
        slice_jobs(groups).into_par_iter().map(|(li, lo, hi, small, feats)| (li, slice_group(feats, lo, hi, small, &props[li]))).collect();
    drop(props);
    let mut buckets = gather_tiles(sliced, layer_names.len(), |(z, x, y)| z >= zmin && z <= zmax && (x >> (z - pz), y >> (z - pz)) == (px, py));
    let encoded = encode_buckets(&mut buckets, layer_names, kinds);
    use std::io::Write;
    for ((id, _, _), bytes) in buckets.iter().zip(&encoded).filter(|(_, b)| !b.is_empty()) {
        spool.w.write_all(bytes).expect("write spool");
        spool.index.push((*id, spool.pos, u32::try_from(bytes.len()).expect("tile under 4 GB")));
        spool.pos += bytes.len() as u64;
    }
    (cand.len(), encoded.iter().filter(|b| !b.is_empty()).count())
}

/// A mapped tile input file: its slug, file name and layer index.
type FileInfo = (String, &'static str, usize);

/// The tile input of every slug -> <out>/<name>.pmtiles in memory-bounded passes (see above). `deep` is the zoom whose tiles are the
/// partition of pass C (4..=14; the default 6 is a tile of about 5.6 x 5 degrees at the equator).
pub fn run_world(data: &Path, slugs: &[String], out: &Path, name: &str, deep: u8) {
    assert!((4..=MAX_ZOOM).contains(&deep), "--deep-zoom must be 4..={MAX_ZOOM}");
    let t0 = Instant::now();
    let layer_names = layer_names();
    let (files, file_info) = map_inputs(data, slugs, &layer_names);
    let all = line_offsets(&files);
    check_not_stale(&files, &file_info, &all);
    eprintln!("[{:6.2}s] {} files, {} features indexed by line; parsing for the index", t0.elapsed().as_secs_f64(), files.len(), all.len());
    let (ents, acc) = index_features(&files, &file_info, all, layer_names.len());
    assert!(!ents.is_empty(), "world tile writer {name}: no features read for {} slugs under {}/<slug>/tiles_tmp", slugs.len(), data.display());
    eprintln!("[{:6.2}s] indexed {} features; rss {} MB", t0.elapsed().as_secs_f64(), ents.len(), rss_mb());

    std::fs::create_dir_all(out).expect("create out dir");
    let spool_path = data.join(format!("{name}.spool"));
    let plan = plan_passes(&ents, deep);
    eprintln!("[{:6.2}s] {} passes planned", t0.elapsed().as_secs_f64(), plan.len());
    let spool = run_passes(&files, &ents, &layer_names, &acc.kinds, &plan, &spool_path, t0);
    write_world_pmtiles(out, name, &layer_names, &acc, &spool, &spool_path, t0);
}

/// Open and map every tile input file of every slug (missing or empty files are skipped).
fn map_inputs(data: &Path, slugs: &[String], layer_names: &[&str]) -> (Vec<memmap2::Mmap>, Vec<FileInfo>) {
    let mut files: Vec<memmap2::Mmap> = Vec::new();
    let mut file_info: Vec<FileInfo> = Vec::new();
    for slug in slugs {
        for (layer, file) in TILE_LAYERS {
            let path = data.join(slug).join("tiles_tmp").join(file);
            let f = match std::fs::File::open(&path) {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => panic!("{slug}: cannot read {}: {e}", path.display()),
            };
            if f.metadata().map_or(true, |m| m.len() == 0) {
                continue;
            }
            assert!(f.metadata().unwrap().len() < u64::from(u32::MAX), "{} is over 4 GB", path.display());
            files.push(unsafe { memmap2::Mmap::map(&f) }.unwrap_or_else(|e| panic!("map {}: {e}", path.display())));
            file_info.push((slug.clone(), file, layer_names.iter().position(|l| *l == layer).unwrap()));
        }
    }
    (files, file_info)
}

/// (file index, offset, length) of every non-empty line of every mapped file.
fn line_offsets(files: &[memmap2::Mmap]) -> Vec<(u32, u32, u32)> {
    let lines: Vec<Vec<(u32, u32)>> = files
        .par_iter()
        .map(|m| {
            let mut v = Vec::new();
            let mut start = 0usize;
            for (i, b) in m.iter().enumerate() {
                if *b == b'\n' {
                    if i > start {
                        v.push((start as u32, (i - start) as u32));
                    }
                    start = i + 1;
                }
            }
            if start < m.len() {
                v.push((start as u32, (m.len() - start) as u32));
            }
            v
        })
        .collect();
    lines.iter().enumerate().flat_map(|(fi, v)| v.iter().map(move |&(o, l)| (fi as u32, o, l))).collect()
}

/// Panic on a tile input written before the zoom-range rename (the overview always carries a zoom range).
fn check_not_stale(files: &[memmap2::Mmap], file_info: &[FileInfo], all: &[(u32, u32, u32)]) {
    for (fi, (slug, file, _)) in file_info.iter().enumerate() {
        if *file == "track_overview.geojsonl" {
            let first = all.iter().find(|a| a.0 as usize == fi).map(|a| &files[fi][a.1 as usize..(a.1 + a.2) as usize]);
            assert!(
                first.is_none_or(|l| String::from_utf8_lossy(l).contains("\"zoom\":")),
                "{slug}: stale tile input (no \"zoom\" member); run `rail-extract build {slug}` again"
            );
        }
    }
}

fn empty_acc(nl: usize) -> Acc {
    Acc { kinds: vec![BTreeMap::new(); nl], bounds: [180.0, 90.0, -180.0, -90.0], zooms: vec![(u8::MAX, 0); nl], count: vec![0; nl] }
}

/// Parse every feature once for the index: its byte range, zoom range and bounding box, plus the column types, layer zoom ranges,
/// feature counts and bounds of the whole input (`Acc::count` is the number of features per layer).
fn index_features(files: &[memmap2::Mmap], file_info: &[FileInfo], all: Vec<(u32, u32, u32)>, nl: usize) -> (Vec<Ent>, Acc) {
    let chunks: Vec<(Vec<Ent>, Acc)> = all
        .par_chunks(4096)
        .map(|ch| {
            let mut acc = empty_acc(nl);
            let mut ents = Vec::with_capacity(ch.len());
            for &(fi, off, len) in ch {
                let li = file_info[fi as usize].2;
                let bytes = &files[fi as usize][off as usize..(off + len) as usize];
                let mut f: Feature = serde_json::from_slice(bytes).unwrap_or_else(|e| panic!("{}: bad feature: {e}", file_info[fi as usize].0));
                assert!(
                    f.foreign_members.as_ref().is_none_or(|m| !m.contains_key("tippecanoe")),
                    "{}: stale tile input (\"tippecanoe\" member); run `rail-extract build {}` again",
                    file_info[fi as usize].0,
                    file_info[fi as usize].0
                );
                let tp = f.foreign_members.take().and_then(|m| m.get("zoom").cloned()).unwrap_or(Value::Null);
                let lo = u8::try_from(tp["minzoom"].as_u64().unwrap_or(0)).unwrap_or(u8::MAX).min(MAX_ZOOM);
                let hi = u8::try_from(tp["maxzoom"].as_u64().unwrap_or(u64::from(MAX_ZOOM))).unwrap_or(u8::MAX).min(MAX_ZOOM);
                let mut bb = [180.0f64, 90.0, -180.0, -90.0];
                let mut bb32 = [1.0f32, 1.0, 0.0, 0.0];
                if let Some(g) = &f.geometry {
                    grow(&mut acc.bounds, &g.value);
                    grow(&mut bb, &g.value);
                    if bb[0] <= bb[2] {
                        let (a, b) = (merc(&[bb[0], bb[1]]), merc(&[bb[2], bb[3]]));
                        bb32 = [a.0 as f32, b.1 as f32, b.0 as f32, a.1 as f32];
                        // y grows southwards
                    }
                }
                for (k, v) in f.properties.iter().flatten() {
                    if let Some(kd) = Kind::of(v) {
                        acc.kinds[li].entry(k.clone()).and_modify(|e| *e = e.merge(kd)).or_insert(kd);
                    }
                }
                acc.zooms[li] = (acc.zooms[li].0.min(lo), acc.zooms[li].1.max(hi));
                acc.count[li] += 1;
                ents.push(Ent { file: fi, off, len, li: li as u8, lo, hi, bb: bb32 });
            }
            (ents, acc)
        })
        .collect();
    drop(all);
    let mut ents: Vec<Ent> = Vec::new();
    let mut total = empty_acc(nl);
    for (e, a) in chunks {
        ents.extend(e);
        for li in 0..nl {
            for (k, v) in a.kinds[li].iter() {
                total.kinds[li].entry(k.clone()).and_modify(|x| *x = x.merge(*v)).or_insert(*v);
            }
            total.zooms[li] = (total.zooms[li].0.min(a.zooms[li].0), total.zooms[li].1.max(a.zooms[li].1));
            total.count[li] += a.count[li];
        }
        let b = total.bounds;
        total.bounds = [b[0].min(a.bounds[0]), b[1].min(a.bounds[1]), b[2].max(a.bounds[2]), b[3].max(a.bounds[3])];
    }
    (ents, total)
}

/// The passes: A (world, z0..=3), B (each populated z4 tile, z4..=deep-1), C (each populated tile at `deep`, deep..=14). Populated
/// partition tiles are found by bounding box with the clip buffer, biggest first.
fn plan_passes(ents: &[Ent], deep: u8) -> Vec<(TileKey, u8, u8)> {
    let populated = |pz: u8, zmin: u8, zmax: u8| -> Vec<(u32, u32)> {
        let mut m: HashMap<(u32, u32), u32> = HashMap::new();
        for e in ents.iter().filter(|e| e.lo <= zmax && e.hi >= zmin && e.bb[0] <= e.bb[2]) {
            cover(&e.bb, pz, |x, y| *m.entry((x, y)).or_insert(0) += 1);
        }
        let mut v: Vec<((u32, u32), u32)> = m.into_iter().collect();
        v.sort_by_key(|&(k, n)| (std::cmp::Reverse(n), k));
        v.into_iter().map(|x| x.0).collect()
    };
    let mut plan: Vec<(TileKey, u8, u8)> = vec![((0, 0, 0), 0, 3)];
    if deep > 4 {
        plan.extend(populated(4, 4, deep - 1).into_iter().map(|(x, y)| ((4, x, y), 4, deep - 1)));
    }
    plan.extend(populated(deep, deep, MAX_ZOOM).into_iter().map(|(x, y)| ((deep, x, y), deep, MAX_ZOOM)));
    plan
}

/// Run every planned pass into a spool file at `spool_path`; returns the finished (flushed) spool.
fn run_passes(
    files: &[memmap2::Mmap],
    ents: &[Ent],
    layer_names: &[&str],
    kinds: &[BTreeMap<String, Kind>],
    plan: &[(TileKey, u8, u8)],
    spool_path: &Path,
    t0: Instant,
) -> Spool {
    use std::io::Write;
    let mut spool =
        Spool { w: std::io::BufWriter::with_capacity(1 << 22, std::fs::File::create(spool_path).expect("create spool")), pos: 0, index: Vec::new() };
    let mut peak_cand = 0usize;
    for (i, (part, zmin, zmax)) in plan.iter().enumerate() {
        let t = Instant::now();
        let (n, tiles) = world_pass(files, ents, layer_names, kinds, *part, *zmin, *zmax, &mut spool);
        peak_cand = peak_cand.max(n);
        eprintln!(
            "[{:6.2}s] pass {}/{} z{}..{} in {:?}: {n} features, {tiles} tiles, {:.1}s, rss {} MB",
            t0.elapsed().as_secs_f64(),
            i + 1,
            plan.len(),
            zmin,
            zmax,
            part,
            t.elapsed().as_secs_f64(),
            rss_mb()
        );
    }
    spool.w.flush().expect("flush spool");
    eprintln!(
        "[{:6.2}s] {} tiles spooled ({:.1} MB); biggest pass {peak_cand} features",
        t0.elapsed().as_secs_f64(),
        spool.index.len(),
        spool.pos as f64 / 1e6
    );
    spool
}

/// The spooled tiles in tile-id order -> <out>/<name>.pmtiles; removes the spool file.
fn write_world_pmtiles(out: &Path, name: &str, layer_names: &[&str], acc: &Acc, spool: &Spool, spool_path: &Path, t0: Instant) {
    use std::os::unix::fs::FileExt;
    let mut index = spool.index.clone();
    index.sort_unstable();
    assert!(index.windows(2).all(|w| w[0].0 != w[1].0), "a tile came out of two passes");
    let meta = tile_metadata(name, layer_names, &acc.kinds, &acc.count, &acc.zooms);
    let sp = std::fs::File::open(spool_path).expect("open spool");
    let done = write_pmtiles(out, name, acc.bounds, &meta, |w| {
        let mut buf = Vec::new();
        for &(id, off, len) in &index {
            buf.resize(len as usize, 0);
            sp.read_exact_at(&mut buf, off).expect("read spool");
            w.add_raw_tile(TileCoord::from(TileId::new(id).expect("tile id")), &buf).expect("add tile");
        }
    });
    let _ = std::fs::remove_file(spool_path);
    let mb = std::fs::metadata(&done).map_or(0.0, |m| m.len() as f64 / 1e6);
    eprintln!("[{:6.2}s] {} : {} tiles, {mb:.1} MB", t0.elapsed().as_secs_f64(), done.display(), index.len());
}
