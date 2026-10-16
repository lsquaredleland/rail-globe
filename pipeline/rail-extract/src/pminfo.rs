//! `rail-extract pminfo FILE [--tsv FILE] [--top N]`: reads a PMTiles v3 file written by tilewriter.rs and prints its header, the tiles and
//! bytes per zoom and the biggest tiles; with --tsv also one line per tile (z x y, gzip bytes, hash of the bytes, and per layer the feature
//! count and an order-independent hash of its osm_id values) for comparing two files.

use mlt_core::{Decoder, ParsedLayer, Parser, PropValue};
use pmtiles::{TileCoord, TileId};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

fn varint(b: &[u8], at: &mut usize) -> u64 {
    let (mut v, mut shift) = (0u64, 0);
    loop {
        let x = b[*at];
        *at += 1;
        v |= u64::from(x & 0x7f) << shift;
        if x < 0x80 {
            return v;
        }
        shift += 7;
    }
}

fn gunzip(b: &[u8]) -> Vec<u8> {
    let n = u32::from_le_bytes(b[b.len() - 4..].try_into().unwrap()) as usize;
    let mut out = vec![0u8; n];
    libdeflater::Decompressor::new().gzip_decompress(b, &mut out).expect("gunzip");
    out
}

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf29ce484222325u64, |h, x| (h ^ u64::from(*x)).wrapping_mul(0x100000001b3))
}

/// (first tile id, run length, offset, length) of every entry, leaf directories followed
fn entries(file: &[u8], dir: &[u8], leaf_off: u64, out: &mut Vec<(u64, u64, u64, u32)>) {
    let d = gunzip(dir);
    let mut at = 0;
    let n = varint(&d, &mut at) as usize;
    let mut id = 0u64;
    let ids: Vec<u64> = (0..n).map(|_| { id += varint(&d, &mut at); id }).collect();
    let runs: Vec<u64> = (0..n).map(|_| varint(&d, &mut at)).collect();
    let lens: Vec<u64> = (0..n).map(|_| varint(&d, &mut at)).collect();
    let mut prev_end = 0u64;
    for i in 0..n {
        let o = varint(&d, &mut at);
        let off = if o == 0 { prev_end } else { o - 1 };
        prev_end = off + lens[i];
        if runs[i] == 0 {
            let leaf = &file[(leaf_off + off) as usize..(leaf_off + off + lens[i]) as usize];
            entries(file, leaf, leaf_off, out);
        } else {
            out.push((ids[i], runs[i], off, lens[i] as u32));
        }
    }
}

/// per layer: feature count and xor of the hashes of the osm_id values
fn layers_of(raw: &[u8]) -> BTreeMap<String, (usize, u64)> {
    let mut res = BTreeMap::new();
    let mut dec = Decoder::default();
    for l in Parser::default().parse_layers(raw).expect("parse tile") {
        let ParsedLayer::Tag01(p) = l.decode_all(&mut dec).expect("decode") else { continue };
        let layer = p.into_tile(&mut dec).expect("tile");
        let col = layer.property_names().iter().position(|n| n == "osm_id");
        let mut h = 0u64;
        for f in layer.features() {
            if let Some(PropValue::Str(Some(s))) = col.map(|c| &f.properties()[c]) {
                h ^= fnv(s.as_bytes());
            }
        }
        res.insert(layer.name().to_string(), (layer.features().len(), h));
    }
    res
}

pub fn run(path: &Path, tsv: Option<&Path>, top: usize) {
    let file = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert!(&file[..7] == b"PMTiles" && file[7] == 3, "not a PMTiles v3 file");
    let h = |at| u64_at(&file, at);
    let (root_off, root_len, meta_off, meta_len, leaf_off, data_off) = (h(8), h(16), h(24), h(32), h(40), h(56));
    println!(
        "{}: {:.1} MB, addressed tiles {}, tile entries {}, tile contents {}, zoom {}..{}, type {}, tile compression {}",
        path.display(), file.len() as f64 / 1e6, h(72), h(80), h(88), file[100], file[101], file[99], file[98]
    );
    let meta = gunzip(&file[meta_off as usize..(meta_off + meta_len) as usize]);
    println!("metadata: {}", String::from_utf8_lossy(&meta).chars().take(300).collect::<String>());
    let mut ents = Vec::new();
    entries(&file, &file[root_off as usize..(root_off + root_len) as usize], leaf_off, &mut ents);
    // every tile (a run repeats one entry's bytes for consecutive ids)
    let mut tiles: Vec<(u8, u32, u32, u64, u32)> = Vec::new();
    for (id, run, off, len) in ents {
        for k in 0..run {
            let c = TileCoord::from(TileId::new(id + k).expect("tile id"));
            tiles.push((c.z(), c.x(), c.y(), data_off + off, len));
        }
    }
    let mut per: BTreeMap<u8, (u64, u64, u32)> = BTreeMap::new();
    for t in &tiles {
        let e = per.entry(t.0).or_insert((0, 0, 0));
        e.0 += 1;
        e.1 += u64::from(t.4);
        e.2 = e.2.max(t.4);
    }
    println!("zoom  tiles  MB(gzip)  mean KB  max KB");
    for (z, (n, b, m)) in &per {
        println!("{z:>4} {n:>7} {:>9.2} {:>8.1} {:>7.1}", *b as f64 / 1e6, *b as f64 / *n as f64 / 1e3, f64::from(*m) / 1e3);
    }
    println!("{} tiles in total", tiles.len());
    let mut big: Vec<&(u8, u32, u32, u64, u32)> = tiles.iter().collect();
    big.sort_by_key(|t| std::cmp::Reverse(t.4));
    println!("biggest tiles (z/x/y, KB gzip):");
    for t in big.iter().take(top) {
        println!("  {}/{}/{}  {:.0}", t.0, t.1, t.2, f64::from(t.4) / 1e3);
    }
    if let Some(p) = tsv {
        let mut w = std::io::BufWriter::new(std::fs::File::create(p).expect("create tsv"));
        for t in &tiles {
            let bytes = &file[t.3 as usize..t.3 as usize + t.4 as usize];
            let layers: Vec<String> = layers_of(&gunzip(bytes)).into_iter().map(|(n, (c, h))| format!("{n}:{c}:{h:x}")).collect();
            writeln!(w, "{}\t{}\t{}\t{}\t{:x}\t{}", t.0, t.1, t.2, t.4, fnv(bytes), layers.join(",")).unwrap();
        }
    }
}
