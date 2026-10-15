//! The tile writer: reads tiles_tmp/*.geojsonl (tiles.rs) of one or more regions, slices with geojson-vt-rs, writes MLT z0-z14 into a PMTiles v3 file.
//!
//! geojson-vt-rs does the projection, the clip with buffer and the per-zoom simplification; mlt-core encodes; pmtiles writes the
//! archive (directories, dedup of identical tiles, gzip). What is written here: the per-feature minzoom/maxzoom (one slicer per
//! (layer, minzoom, maxzoom) group, tiles kept only inside the zoom range), input order (a sequence number rides in the feature id and
//! is sorted on per tile and layer), --coalesce (consecutive same-attribute pieces of a layer in a tile become one multi-geometry),
//! and the vector_layers metadata.

use mlt_core::encoder::EncoderConfig;
use mlt_core::geo_types::Coord;
use mlt_core::{FeatureWriter, GeometryType, LayerWriter, MltResult, PropKind, PropertyKey};
use geojson::{Feature, FeatureCollection, Value as Gv};
use geojson_vt_rs::{GeoJSONVT, Options, TileOptions};
use pmtiles::{Compression, PmTilesWriter, TileCoord, TileId, TileType};
use rayon::prelude::*;
use serde_json::{json, Map, Value};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::tiles::TILE_LAYERS;

const MAX_ZOOM: u8 = 14;
const EXTENT: u16 = 4096;
const SIMPLIFY: f64 = 2.0;
const CHUNK: usize = 8_000;

/// a tile coordinate in tile units (clip buffer included, so well inside i16); 4 bytes a point instead of 40 as a GeoJSON position
type Pt = [i16; 2];

/// geometry of one piece in tile units; one part is a Point, LineString or Polygon, several are the Multi- type
enum Geom {
    Points(Vec<Pt>),
    Lines(Vec<Vec<Pt>>),
    /// polygons, each a list of rings (exterior first)
    Polys(Vec<Vec<Vec<Pt>>>),
}

fn compact(v: &Gv) -> Option<Geom> {
    let pt = |p: &Vec<f64>| [p[0].round() as i16, p[1].round() as i16];
    let line = |l: &Vec<Vec<f64>>| l.iter().map(pt).collect::<Vec<Pt>>();
    let poly = |rs: &Vec<Vec<Vec<f64>>>| rs.iter().map(line).collect::<Vec<_>>();
    Some(match v {
        Gv::Point(p) => Geom::Points(vec![pt(p)]),
        Gv::MultiPoint(l) => Geom::Points(line(l)),
        Gv::LineString(l) => Geom::Lines(vec![line(l)]),
        Gv::MultiLineString(ls) => Geom::Lines(ls.iter().map(line).collect()),
        Gv::Polygon(rs) => Geom::Polys(vec![poly(rs)]),
        Gv::MultiPolygon(ps) => Geom::Polys(ps.iter().map(poly).collect()),
        _ => return None,
    })
}

/// one feature in a tile: sequence number (input order), properties (shared by all pieces of a source feature), geometry
struct Piece {
    seq: u64,
    props: Arc<Map<String, Value>>,
    geom: Geom,
}

type TileKey = (u8, u32, u32);

fn grow(b: &mut [f64; 4], v: &Gv) {
    fn pos(b: &mut [f64; 4], p: &[f64]) {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    }
    match v {
        Gv::Point(p) => pos(b, p),
        Gv::MultiPoint(l) | Gv::LineString(l) => l.iter().for_each(|p| pos(b, p)),
        Gv::MultiLineString(l) | Gv::Polygon(l) => l.iter().flatten().for_each(|p| pos(b, p)),
        Gv::MultiPolygon(l) => l.iter().flatten().flatten().for_each(|p| pos(b, p)),
        _ => {}
    }
}

/// Web Mercator on the unit square, as geojson-vt projects
fn merc(p: &[f64]) -> (f64, f64) {
    let s = p[1].to_radians().sin();
    (p[0] / 360.0 + 0.5, 0.5 - 0.25 * ((1.0 + s) / (1.0 - s)).ln() / std::f64::consts::PI)
}

/// Would geojson-vt drop a part of this feature at tile zoom `lo`? It drops a line shorter than the tolerance and a ring smaller than
/// tolerance squared (tolerance = 2 / (4096 * 2^lo) of the world). Such features are sliced with a tiny
/// tolerance instead (see `slice_group`), so every id survives at every zoom of its range.
fn tiny(g: &Gv, lo: u8) -> bool {
    let tol = SIMPLIFY / (EXTENT as f64 * f64::from(1u32 << lo));
    let line = |l: &[Vec<f64>]| l.windows(2).map(|w| { let (a, b) = (merc(&w[0]), merc(&w[1])); (b.0 - a.0).hypot(b.1 - a.1) }).sum::<f64>() <= tol;
    let ring = |r: &[Vec<f64>]| {
        let pts: Vec<(f64, f64)> = r.iter().map(|p| merc(p)).collect();
        let twice: f64 = pts.windows(2).map(|w| w[0].0 * w[1].1 - w[1].0 * w[0].1).sum();
        (twice / 2.0).abs() <= tol * tol
    };
    // cheap exit for the great majority of lines: a bounding box wider than three tolerances (degrees, y stretched by 1/cos) is longer
    if let Gv::LineString(_) = g {
        let mut b = [180.0f64, 90.0, -180.0, -90.0];
        grow(&mut b, g);
        let k = ((b[1] + b[3]) / 2.0).to_radians().cos().max(1e-6);
        if ((b[2] - b[0]) / 360.0).max((b[3] - b[1]) / 360.0 / k) > 3.0 * tol {
            return false;
        }
    }
    match g {
        Gv::LineString(l) => line(l),
        Gv::MultiLineString(ls) => ls.iter().any(|l| line(l)),
        Gv::Polygon(rs) => rs.first().is_some_and(|r| ring(r)),
        Gv::MultiPolygon(ps) => ps.iter().any(|rs| rs.first().is_some_and(|r| ring(r))),
        _ => false,
    }
}

/// slice one group of features into tiles z in [lo, hi], descending only through non-empty tiles; `tiny` groups are sliced
/// without simplification (they are smaller than the tolerance at their first zoom anyway)
fn slice_group(features: Vec<Feature>, lo: u8, hi: u8, tiny: bool, props: &[Arc<Map<String, Value>>]) -> Vec<(TileKey, Vec<Piece>)> {
    let opts = Options {
        max_zoom: MAX_ZOOM,
        index_max_zoom: 5,
        index_max_points: 100_000,
        generate_id: false,
        tile: TileOptions { tolerance: if tiny { 0.01 } else { SIMPLIFY }, extent: EXTENT, buffer: 80, line_metrics: false },
    };
    let mut vt = GeoJSONVT::new(&FeatureCollection { bbox: None, features, foreign_members: None }, &opts);
    let mut out = Vec::new();
    let mut stack = vec![(0u8, 0u32, 0u32)];
    while let Some((z, x, y)) = stack.pop() {
        let tile = vt.get_tile(z, x, y);
        // a tile can be drawn empty (every piece smaller than the tolerance) and still have features below it
        if tile.num_points == 0 {
            continue;
        }
        if z >= lo && !tile.features.features.is_empty() {
            let pieces: Vec<Piece> = tile
                .features
                .features
                .iter()
                .filter_map(|f| {
                    let seq = match f.id.as_ref()? {
                        geojson::feature::Id::Number(n) => n.as_u64()?,
                        _ => return None,
                    };
                    Some(Piece { seq, props: Arc::clone(&props[seq as usize]), geom: compact(&f.geometry.as_ref()?.value)? })
                })
                .collect();
            out.push(((z, x, y), pieces));
        }
        if z < hi {
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                stack.push((z + 1, x * 2 + dx, y * 2 + dy));
            }
        }
    }
    out
}

/// lines that meet end to start become one line (without it the z0-8 overview has three times the parts)
fn join_lines(parts: Vec<Vec<Pt>>) -> Vec<Vec<Pt>> {
    let mut by_start: HashMap<Pt, Vec<usize>> = HashMap::new();
    for (i, l) in parts.iter().enumerate().rev() {
        if let Some(&first) = l.first() {
            by_start.entry(first).or_default().push(i);
        }
    }
    let mut parts: Vec<Option<Vec<Pt>>> = parts.into_iter().map(Some).collect();
    let mut out = Vec::new();
    for i in 0..parts.len() {
        let Some(mut cur) = parts[i].take() else { continue };
        while let Some(&end) = cur.last() {
            let next = by_start.get_mut(&end).and_then(|c| {
                while let Some(j) = c.pop() {
                    if parts[j].is_some() {
                        return Some(j);
                    }
                }
                None
            });
            let Some(j) = next else { break };
            cur.extend(parts[j].take().into_iter().flatten().skip(1));
        }
        out.push(cur);
    }
    out
}

/// --coalesce: consecutive pieces with equal properties and the same geometry kind become one multi-geometry
fn coalesce(pieces: Vec<Piece>) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    for p in pieces {
        if let Some(last) = out.last_mut() {
            if last.props == p.props && std::mem::discriminant(&last.geom) == std::mem::discriminant(&p.geom) {
                match (&mut last.geom, p.geom) {
                    (Geom::Points(a), Geom::Points(b)) => a.extend(b),
                    (Geom::Lines(a), Geom::Lines(b)) => a.extend(b),
                    (Geom::Polys(a), Geom::Polys(b)) => a.extend(b),
                    _ => unreachable!("same kind"),
                }
                continue;
            }
        }
        out.push(p);
    }
    for p in &mut out {
        if let Geom::Lines(l) = &mut p.geom {
            if l.len() > 1 {
                *l = join_lines(std::mem::take(l));
            }
        }
    }
    out
}

/// the one type a property has in a layer (MLT allows one per column): Int is i32, Float is f64; mixed kinds widen to Str
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Bool,
    Int,
    Int64,
    Float,
    Str,
}

impl Kind {
    fn of(v: &Value) -> Option<Kind> {
        match v {
            Value::Null => None,
            Value::Bool(_) => Some(Kind::Bool),
            Value::Number(n) => Some(match n.as_i64() {
                Some(i) if i32::try_from(i).is_ok() => Kind::Int,
                Some(_) => Kind::Int64,
                None => Kind::Float,
            }),
            _ => Some(Kind::Str),
        }
    }

    /// the narrowest kind that holds both: ints widen to floats, anything else mixed becomes a string
    fn merge(self, o: Kind) -> Kind {
        use Kind::*;
        match (self, o) {
            (a, b) if a == b => a,
            (Int | Int64 | Float, Int | Int64 | Float) => if self == Float || o == Float { Float } else { Int64 },
            _ => Str,
        }
    }

    /// i64 beyond i32 is stored as f64 (exact to 2^53; MapLibre's JS decoder returns BigInt for 64-bit integer columns)
    fn stored(self) -> Kind {
        if self == Kind::Int64 { Kind::Float } else { self }
    }

    fn field_type(self) -> &'static str {
        match self.stored() {
            Kind::Bool => "Boolean",
            Kind::Int | Kind::Int64 | Kind::Float => "Number",
            Kind::Str => "String",
        }
    }
}

/// one property value as stored in its column
enum Cell<'a> {
    Bool(bool),
    Int(i32),
    Float(f64),
    Str(Cow<'a, str>),
}

fn cell<'a>(v: &'a Value, kind: Kind) -> Option<Cell<'a>> {
    if v.is_null() {
        return None;
    }
    Some(match (kind.stored(), v) {
        (Kind::Bool, Value::Bool(b)) => Cell::Bool(*b),
        (Kind::Int, Value::Number(n)) => Cell::Int(n.as_i64().and_then(|i| i32::try_from(i).ok()).expect("int fits i32")),
        (Kind::Float, Value::Number(n)) => Cell::Float(n.as_f64().expect("number")),
        (Kind::Str, Value::String(s)) => Cell::Str(Cow::Borrowed(s)),
        (Kind::Str, other) => Cell::Str(Cow::Owned(other.to_string())),
        (k, other) => unreachable!("property {other} does not fit its layer type {k:?}"),
    })
}

fn tc(p: &Pt) -> Coord<i32> {
    Coord { x: i32::from(p[0]), y: i32::from(p[1]) }
}

/// MLT layers of one tile (`layers`: name, property types of the layer, pieces in draw order), concatenated
fn encode(layers: Vec<(&str, &BTreeMap<String, Kind>, Vec<Piece>)>) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, kinds, pieces) in &layers {
        // only the columns this tile uses
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for p in pieces {
            seen.extend(p.props.iter().filter(|(_, v)| !v.is_null()).map(|(k, _)| k.as_str()));
        }
        let cols: Vec<(&str, Kind)> = kinds.iter().filter(|(k, _)| seen.contains(k.as_str())).map(|(k, v)| (k.as_str(), *v)).collect();
        let cells: Vec<Vec<Option<Cell>>> =
            pieces.iter().map(|p| cols.iter().map(|(k, kind)| p.props.get(*k).and_then(|v| cell(v, *kind))).collect()).collect();
        let mut layer = LayerWriter::new(name, EXTENT as u32).expect("layer");
        let keys: Vec<PropertyKey> = cols
            .iter()
            .map(|(k, kind)| {
                let pk = match kind.stored() {
                    Kind::Bool => PropKind::Bool,
                    Kind::Int => PropKind::I32,
                    Kind::Float | Kind::Int64 => PropKind::F64,
                    Kind::Str => PropKind::Str,
                };
                layer.add_property(k, pk).expect("property")
            })
            .collect();
        for (p, row) in pieces.iter().zip(&cells) {
            let gt = match &p.geom {
                Geom::Points(l) if l.len() == 1 => GeometryType::Point,
                Geom::Points(_) => GeometryType::MultiPoint,
                Geom::Lines(l) if l.len() == 1 => GeometryType::LineString,
                Geom::Lines(_) => GeometryType::MultiLineString,
                Geom::Polys(l) if l.len() == 1 => GeometryType::Polygon,
                Geom::Polys(_) => GeometryType::MultiPolygon,
            };
            let mut f = layer.feature(gt);
            for (key, c) in keys.iter().zip(row) {
                match c {
                    Some(Cell::Bool(b)) => f.property(*key, *b),
                    Some(Cell::Int(i)) => f.property(*key, *i),
                    Some(Cell::Float(x)) => f.property(*key, *x),
                    Some(Cell::Str(s)) => f.property(*key, s.as_ref()),
                    None => continue,
                }
                .expect("property value");
            }
            match &p.geom {
                Geom::Points(l) => f.points(l.iter().map(tc)),
                Geom::Lines(ls) if ls.len() == 1 => f.line(ls[0].iter().map(tc)),
                Geom::Lines(ls) => ls.iter().try_for_each(|l| f.line(l.iter().map(tc)).map(|_| ())).map(|_| &mut f),
                Geom::Polys(ps) => ps.iter().try_for_each(|rs| add_polygon(&mut f, rs).map(|_| ())).map(|_| &mut f),
            }
            .expect("geometry");
            f.finish().expect("feature");
        }
        out.extend(layer.encode(encoder_config()).expect("encode layer"));
    }
    out
}

/// Tiles are stored gzipped. FastPFOR and FSST already compress the streams, which gzip then cannot shrink further: turning both off
/// made 349 dense China city tiles (z6-14) 13.6% smaller gzipped and china.pmtiles 1.9% smaller; JS decode time is unchanged (+-3%).
fn encoder_config() -> EncoderConfig {
    EncoderConfig::default().with_fastpfor(false).with_fsst(false)
}

/// exterior ring first, then the holes
fn add_polygon<'w, 'a, 'f>(f: &'f mut FeatureWriter<'w, 'a>, rings: &[Vec<Pt>]) -> MltResult<&'f mut FeatureWriter<'w, 'a>> {
    for (i, r) in rings.iter().enumerate() {
        let coords = r.iter().map(tc);
        if i == 0 {
            f.exterior_ring(coords)?;
        } else {
            f.hole(coords)?;
        }
    }
    Ok(f)
}

/// The tile input (<data>/<slug>/tiles_tmp) of every slug -> <out>/<name>.pmtiles, written to <name>.tmp.pmtiles and renamed on success.
/// Slices one region at a time (peak memory is the biggest region's, about 5 GB for China; do not run several in parallel).
pub fn run(data: &Path, slugs: &[String], out: &Path, name: &str) {
    let t0 = Instant::now();
    // layer names in first-seen order (TILE_LAYERS order)
    let mut layer_names: Vec<&str> = Vec::new();
    for (l, _) in TILE_LAYERS {
        if !layer_names.contains(&l) {
            layer_names.push(l);
        }
    }
    // ---- one region at a time: read (layer index, minzoom, maxzoom, tiny) -> features in input order (seq in the id), slice, drop the
    // input. Only the biggest region's input and slicers are in memory at once; the pieces of all regions are kept until the encode.
    let mut seq = vec![0u64; layer_names.len()];
    let mut bounds = [180.0f64, 90.0, -180.0, -90.0];
    let mut kinds: Vec<BTreeMap<String, Kind>> = vec![BTreeMap::new(); layer_names.len()];
    // properties of every source feature of a layer, by seq: the slicer gets bare geometry, the pieces share these
    let mut props: Vec<Vec<Arc<Map<String, Value>>>> = vec![Vec::new(); layer_names.len()];
    let mut zooms: Vec<(u8, u8)> = vec![(u8::MAX, 0); layer_names.len()];
    let mut sliced: Vec<(usize, Vec<(TileKey, Vec<Piece>)>)> = Vec::new();
    let mut bad_zoom_warned: BTreeSet<(usize, u64, u64)> = BTreeSet::new();
    for slug in slugs {
        let mut groups: BTreeMap<(usize, u8, u8, bool), Vec<Feature>> = BTreeMap::new();
        for (layer, file) in TILE_LAYERS {
            let li = layer_names.iter().position(|l| *l == layer).unwrap();
            let path = data.join(slug).join("tiles_tmp").join(file);
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => panic!("{slug}: cannot read {}: {e}", path.display()),
            };
            let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
            // the overview (track_overview) always carries a zoom range: without one the file is from before the rename, rebuild the region
            assert!(file != "track_overview.geojsonl" || lines.first().map_or(true, |l| l.contains("\"zoom\":")), "{slug}: stale tile input (no \"zoom\" member); run `rail-extract build {slug}` again");
            let feats: Vec<Feature> = lines
                .par_iter()
                .enumerate()
                .map(|(i, l)| serde_json::from_str::<Feature>(l).unwrap_or_else(|e| panic!("{slug}: {file} line {}: bad feature: {e}", i + 1)))
                .collect();
            for mut f in feats {
                assert!(f.foreign_members.as_ref().map_or(true, |m| !m.contains_key("tippecanoe")), "{slug}: stale tile input ({file} has a \"tippecanoe\" member); run `rail-extract build {slug}` again");
                let tp = f.foreign_members.as_ref().and_then(|m| m.get("zoom")).cloned().unwrap_or(Value::Null);
                let (raw_lo, raw_hi) = (tp["minzoom"].as_u64().unwrap_or(0), tp["maxzoom"].as_u64().unwrap_or(MAX_ZOOM as u64));
                let lo = u8::try_from(raw_lo).unwrap_or(u8::MAX).min(MAX_ZOOM);
                let hi = u8::try_from(raw_hi).unwrap_or(u8::MAX).min(MAX_ZOOM);
                if (raw_lo > raw_hi || raw_lo > u64::from(MAX_ZOOM)) && bad_zoom_warned.insert((li, raw_lo, raw_hi)) {
                    eprintln!("warning: {slug}: layer {layer} has features with zoom range {raw_lo}..{raw_hi} (max zoom {MAX_ZOOM}); clamped to {lo}..{hi}");
                }
                f.foreign_members = None;
                f.id = Some(geojson::feature::Id::Number(seq[li].into()));
                seq[li] += 1;
                if let Some(g) = &f.geometry {
                    grow(&mut bounds, &g.value);
                }
                let fp = f.properties.take().unwrap_or_default();
                for (k, v) in &fp {
                    if let Some(kd) = Kind::of(v) {
                        kinds[li]
                            .entry(k.clone())
                            .and_modify(|e| {
                                let m = e.merge(kd);
                                if m == Kind::Str && *e != Kind::Str {
                                    eprintln!("warning: {slug}: layer {} key {k:?} has mixed value types ({e:?} and {kd:?}); stored as String", layer_names[li]);
                                }
                                *e = m;
                            })
                            .or_insert(kd);
                    }
                }
                props[li].push(Arc::new(fp));
                zooms[li] = (zooms[li].0.min(lo), zooms[li].1.max(hi.min(MAX_ZOOM)));
                let small = f.geometry.as_ref().is_some_and(|g| tiny(&g.value, lo));
                groups.entry((li, lo, hi.min(MAX_ZOOM), small)).or_default().push(f);
            }
        }
        eprintln!("[{:6.2}s] read {slug}: {} groups", t0.elapsed().as_secs_f64(), groups.len());

        // slice every group in parallel; a big group is cut into runs of CHUNK features first (features are sliced independently, and the
        // pieces are put back in input order by seq), or China's one 245k-feature track group would be a single 20 s thread
        let mut jobs: Vec<(usize, u8, u8, bool, Vec<Feature>)> = Vec::new();
        for ((li, lo, hi, small), mut feats) in groups {
            while feats.len() > CHUNK {
                let rest = feats.split_off(CHUNK);
                jobs.push((li, lo, hi, small, std::mem::replace(&mut feats, rest)));
            }
            jobs.push((li, lo, hi, small, feats));
        }
        let part: Vec<(usize, Vec<(TileKey, Vec<Piece>)>)> =
            jobs.into_par_iter().map(|(li, lo, hi, small, feats)| (li, slice_group(feats, lo, hi, small, &props[li]))).collect();
        sliced.extend(part);
        eprintln!("[{:6.2}s] sliced {slug}", t0.elapsed().as_secs_f64());
    }

    assert!(
        seq.iter().any(|&n| n > 0),
        "tile writer {name}: no features read for {slugs:?} under {}/<slug>/tiles_tmp (wrong slug, or run `rail-extract build <slug>` first)",
        data.display()
    );

    // ---- gather per tile and layer, in tile-id order
    let mut tiles: HashMap<TileKey, Vec<Vec<Piece>>> = HashMap::new();
    for (li, g) in sliced {
        for (key, pieces) in g {
            tiles.entry(key).or_insert_with(|| (0..layer_names.len()).map(|_| Vec::new()).collect())[li].extend(pieces);
        }
    }
    let mut order: Vec<(u64, TileKey)> =
        tiles.keys().map(|&(z, x, y)| (TileId::from(TileCoord::new(z, x, y).expect("coord")).value(), (z, x, y))).collect();
    order.sort_unstable();
    let mut buckets: Vec<(TileKey, Vec<Vec<Piece>>)> = order.iter().map(|(_, k)| (*k, tiles.remove(k).unwrap())).collect();
    eprintln!("[{:6.2}s] {} tiles gathered", t0.elapsed().as_secs_f64(), buckets.len());

    // ---- encode and gzip in parallel (order kept), then write
    let encoded: Vec<Vec<u8>> = buckets
        .par_iter_mut()
        .map_init(|| libdeflater::Compressor::new(libdeflater::CompressionLvl::default()), |gz, (_, layers)| {
            let layers: Vec<(&str, &BTreeMap<String, Kind>, Vec<Piece>)> = layers
                .iter_mut()
                .enumerate()
                .filter(|(_, p)| !p.is_empty())
                .map(|(li, p)| {
                    let mut p = std::mem::take(p);
                    p.sort_by_key(|x| x.seq);
                    (layer_names[li], &kinds[li], coalesce(p))
                })
                .collect();
            // gzip here, in parallel: the pmtiles writer would compress every tile one after the other
            let raw = encode(layers);
            let mut out = vec![0u8; gz.gzip_compress_bound(raw.len())];
            let n = gz.gzip_compress(&raw, &mut out).expect("gzip tile");
            out.truncate(n);
            out
        })
        .collect();
    eprintln!("[{:6.2}s] encoded", t0.elapsed().as_secs_f64());

    let fields: Vec<BTreeMap<&str, &str>> =
        kinds.iter().map(|m| m.iter().map(|(k, v)| (k.as_str(), v.field_type())).collect()).collect();
    let vector_layers: Vec<Value> = (0..layer_names.len())
        .filter(|&li| seq[li] > 0)
        .map(|li| json!({"id": layer_names[li], "fields": fields[li], "minzoom": zooms[li].0, "maxzoom": zooms[li].1}))
        .collect();
    let meta = json!({"name": name, "format": "mlt", "minzoom": "0", "maxzoom": MAX_ZOOM.to_string(), "vector_layers": vector_layers,
        "generator": "rail-extract"});
    std::fs::create_dir_all(out).expect("create out dir");
    let path = out.join(format!("{name}.tmp.pmtiles"));
    let mut w = PmTilesWriter::new(TileType::Mlt)
        .tile_compression(Compression::Gzip)
        .min_zoom(0)
        .max_zoom(MAX_ZOOM)
        .bounds(bounds[0], bounds[1], bounds[2], bounds[3])
        .center((bounds[0] + bounds[2]) / 2.0, (bounds[1] + bounds[3]) / 2.0)
        .metadata(&meta.to_string())
        .create(std::fs::File::create(&path).expect("create pmtiles"))
        .expect("pmtiles writer");
    for ((z, x, y), bytes) in buckets.iter().map(|b| b.0).zip(&encoded) {
        w.add_raw_tile(TileCoord::new(z, x, y).expect("coord"), bytes).expect("add tile");
    }
    w.finalize().expect("finalize pmtiles");
    let path = {
        let done = out.join(format!("{name}.pmtiles"));
        std::fs::rename(&path, &done).expect("rename pmtiles");
        done
    };
    let mb = std::fs::metadata(&path).map_or(0.0, |m| m.len() as f64 / 1e6);
    eprintln!("[{:6.2}s] {} : {} tiles, {mb:.1} MB", t0.elapsed().as_secs_f64(), path.display(), encoded.len());
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlt_core::encoder::wound_geometry;
    use mlt_core::geo_types::{Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
    use mlt_core::{Decoder, ParsedLayer, Parser, PropValue};

    fn ring(pts: &[(i32, i32)]) -> Vec<Vec<f64>> {
        pts.iter().map(|&(x, y)| vec![x as f64, y as f64]).collect()
    }

    fn gring(pts: &[(i32, i32)]) -> LineString<i32> {
        LineString::from(pts.iter().map(|&(x, y)| Coord { x, y }).collect::<Vec<_>>())
    }

    fn props(v: Value) -> Arc<Map<String, Value>> {
        Arc::new(v.as_object().expect("object").clone())
    }

    fn geom(g: Gv) -> Geom {
        compact(&g).expect("geometry")
    }

    fn kinds_of(pieces: &[Piece]) -> BTreeMap<String, Kind> {
        let mut m: BTreeMap<String, Kind> = BTreeMap::new();
        for p in pieces {
            for (k, v) in p.props.iter() {
                if let Some(kd) = Kind::of(v) {
                    m.entry(k.clone()).and_modify(|e| *e = e.merge(kd)).or_insert(kd);
                }
            }
        }
        m
    }

    #[test]
    fn mlt_round_trip() {
        let outer = [(0, 0), (100, 0), (100, 100), (0, 100), (0, 0)];
        let hole = [(20, 20), (20, 80), (80, 80), (80, 20), (20, 20)];
        let outer2 = [(200, 200), (300, 200), (300, 300), (200, 200)];
        let pieces = vec![
            Piece { seq: 0, props: props(json!({"name": "a", "n": 1, "ok": true, "mix": 5})), geom: geom(Gv::Point(vec![10.0, 20.0])) },
            Piece {
                seq: 1,
                props: props(json!({"name": "b", "n": 2.5, "mix": "x"})),
                geom: geom(Gv::MultiPoint(vec![vec![1.0, 2.0], vec![3.0, 4.0]])),
            },
            Piece {
                seq: 2,
                props: props(json!({"name": "c", "big": 5_000_000_000i64, "mix": true})),
                geom: geom(Gv::LineString(ring(&[(0, 0), (50, 60), (70, 10)]))),
            },
            Piece {
                seq: 3,
                props: props(json!({"ok": false})),
                geom: geom(Gv::MultiLineString(vec![ring(&[(0, 0), (5, 5)]), ring(&[(10, 10), (20, 30), (40, 30)])])),
            },
            Piece { seq: 4, props: props(json!({"name": "e"})), geom: geom(Gv::Polygon(vec![ring(&outer), ring(&hole)])) },
            Piece {
                seq: 5,
                props: props(json!({"name": "f", "n": -3})),
                geom: geom(Gv::MultiPolygon(vec![vec![ring(&outer), ring(&hole)], vec![ring(&outer2)]])),
            },
        ];
        let kinds = kinds_of(&pieces);
        assert_eq!(kinds["name"], Kind::Str);
        assert_eq!(kinds["n"], Kind::Float);
        assert_eq!(kinds["mix"], Kind::Str);
        assert_eq!(kinds["big"].stored(), Kind::Float);
        assert_eq!(kinds["ok"], Kind::Bool);

        let bytes = encode(vec![("test", &kinds, pieces)]);

        let layers = Parser::default().parse_layers(&bytes).expect("parse");
        assert_eq!(layers.len(), 1);
        let mut dec = Decoder::default();
        let ParsedLayer::Tag01(parsed) = layers.into_iter().next().unwrap().decode_all(&mut dec).expect("decode") else {
            panic!("expected a v1 layer");
        };
        let layer = parsed.into_tile(&mut dec).expect("tile");
        assert_eq!(layer.name(), "test");
        let names: Vec<&str> = layer.property_names().iter().map(String::as_str).collect();
        let col = |n: &str| names.iter().position(|x| *x == n).unwrap_or_else(|| panic!("column {n}"));

        let want_geoms: Vec<Geometry<i32>> = vec![
            Geometry::Point(Point::new(10, 20)),
            Geometry::MultiPoint(MultiPoint::new(vec![Point::new(1, 2), Point::new(3, 4)])),
            Geometry::LineString(gring(&[(0, 0), (50, 60), (70, 10)])),
            Geometry::MultiLineString(MultiLineString::new(vec![gring(&[(0, 0), (5, 5)]), gring(&[(10, 10), (20, 30), (40, 30)])])),
            Geometry::Polygon(Polygon::new(gring(&outer), vec![gring(&hole)])),
            Geometry::MultiPolygon(MultiPolygon::new(vec![
                Polygon::new(gring(&outer), vec![gring(&hole)]),
                Polygon::new(gring(&outer2), vec![]),
            ])),
        ];
        let feats = layer.features();
        assert_eq!(feats.len(), want_geoms.len());
        for (i, (f, w)) in feats.iter().zip(&want_geoms).enumerate() {
            assert_eq!(f.geometry(), &wound_geometry(w), "geometry of feature {i}");
        }

        let s = |f: usize, n: &str| match &feats[f].properties()[col(n)] {
            PropValue::Str(v) => v.clone(),
            other => panic!("{n} is {other:?}"),
        };
        let fl = |f: usize, n: &str| match &feats[f].properties()[col(n)] {
            PropValue::F64(v) => *v,
            other => panic!("{n} is {other:?}"),
        };
        let bo = |f: usize, n: &str| match &feats[f].properties()[col(n)] {
            PropValue::Bool(v) => *v,
            other => panic!("{n} is {other:?}"),
        };
        // strings; a key absent from a feature is null
        assert_eq!(s(0, "name").as_deref(), Some("a"));
        assert_eq!(s(3, "name"), None);
        assert_eq!(s(5, "name").as_deref(), Some("f"));
        // mixed number/string/bool key is a string everywhere
        assert_eq!(s(0, "mix").as_deref(), Some("5"));
        assert_eq!(s(1, "mix").as_deref(), Some("x"));
        assert_eq!(s(2, "mix").as_deref(), Some("true"));
        assert_eq!(s(4, "mix"), None);
        // int and float mixed is a float
        assert_eq!(fl(0, "n"), Some(1.0));
        assert_eq!(fl(1, "n"), Some(2.5));
        assert_eq!(fl(5, "n"), Some(-3.0));
        assert_eq!(fl(2, "big"), Some(5e9));
        assert_eq!(bo(0, "ok"), Some(true));
        assert_eq!(bo(3, "ok"), Some(false));
        assert_eq!(bo(1, "ok"), None);
    }

    /// signed area of a ring in tile units (y down): positive is clockwise on screen, the MLT/MVT exterior winding
    fn area2(r: &LineString<i32>) -> i64 {
        r.0.windows(2).map(|w| w[0].x as i64 * w[1].y as i64 - w[1].x as i64 * w[0].y as i64).sum()
    }

    #[test]
    fn exterior_ring_is_wound_clockwise() {
        // exterior given counter-clockwise on screen and the hole clockwise: the decoded exterior must come back clockwise (positive), the hole opposite
        let outer = [(0, 0), (0, 100), (100, 100), (100, 0), (0, 0)];
        let hole = [(20, 20), (80, 20), (80, 80), (20, 80), (20, 20)];
        assert!(area2(&gring(&outer)) < 0 && area2(&gring(&hole)) > 0, "fixture windings");
        let piece = Piece { seq: 0, geom: geom(Gv::Polygon(vec![ring(&outer), ring(&hole)])), props: props(json!({})) };
        let kinds = BTreeMap::new();
        let bytes = encode(vec![("areas", &kinds, vec![piece])]);
        let mut dec = Decoder::default();
        let layers = Parser::default().parse_layers(&bytes).expect("parse");
        let ParsedLayer::Tag01(p) = layers.into_iter().next().expect("layer").decode_all(&mut dec).expect("decode") else { panic!("tag") };
        let tile = p.into_tile(&mut dec).expect("tile");
        let Geometry::Polygon(poly) = tile.features()[0].geometry().clone() else { panic!("polygon") };
        assert!(area2(poly.exterior()) > 0, "exterior ring winding");
        assert!(poly.interiors().iter().all(|h| area2(h) < 0), "hole winding");
    }

    #[test]
    fn tiny_lines_survive_slicing() {
        // 5 m of track near San Francisco: shorter than the tolerance at z9 (about 38 m), so geojson-vt alone would drop it
        let short = Gv::LineString(vec![vec![-122.4, 37.78], vec![-122.4, 37.78004]]);
        let long = Gv::LineString(vec![vec![-122.4, 37.78], vec![-122.39, 37.79]]);
        assert!(tiny(&short, 9) && !tiny(&long, 9));
        let feat = |g: Gv, n: u64| Feature {
            id: Some(geojson::feature::Id::Number(n.into())),
            geometry: Some(geojson::Geometry::new(g)),
            properties: None,
            bbox: None,
            foreign_members: None,
        };
        let seqs = |small: bool| {
            let tiles = slice_group(vec![feat(short.clone(), 0)], 9, 10, small, &[Arc::new(Map::new())]);
            tiles.iter().filter(|(k, _)| k.0 == 9).map(|(_, p)| p.len()).sum::<usize>()
        };
        assert_eq!(seqs(false), 0);
        assert_eq!(seqs(true), 1);
    }
}
