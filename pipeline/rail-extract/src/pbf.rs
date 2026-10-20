//! Minimal OSM PBF reader (https://wiki.openstreetmap.org/wiki/PBF_Format), adapted from rail-atlas `extract/src/pbf.rs`.
//!
//! The file is a sequence of [4-byte big-endian header length][BlobHeader][Blob]. Blobs are zlib-compressed PrimitiveBlocks.
//! The blob headers are indexed once (and the index cached next to the output), so each pass decodes only the blobs of
//! its element type, in parallel. Each blob is one positioned read (no memory map: on a 95 GB file that is a page-fault storm).
//! Read and decompression buffers are reused per thread.

use libdeflater::Decompressor;
use std::cell::RefCell;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::FileExt;
use std::path::Path;

// ---------------------------------------------------------------- protobuf wire format

pub struct Pb<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Pb<'a> {
    #[inline]
    pub fn new(b: &'a [u8]) -> Self {
        Pb { b, p: 0 }
    }
    #[inline]
    pub fn done(&self) -> bool {
        self.p >= self.b.len()
    }
    #[inline(always)]
    pub fn varint(&mut self) -> u64 {
        let b = self.b;
        let mut p = self.p;
        let c = b[p];
        p += 1;
        if c < 0x80 {
            self.p = p;
            return c as u64;
        }
        let mut r = (c & 0x7f) as u64;
        let mut s = 7;
        loop {
            let c = b[p];
            p += 1;
            r |= ((c & 0x7f) as u64) << s;
            if c < 0x80 {
                self.p = p;
                return r;
            }
            s += 7;
        }
    }
    #[inline]
    pub fn key(&mut self) -> (u32, u8) {
        let k = self.varint();
        ((k >> 3) as u32, (k & 7) as u8)
    }
    #[inline]
    pub fn bytes(&mut self) -> &'a [u8] {
        let l = self.varint() as usize;
        let s = &self.b[self.p..self.p + l];
        self.p += l;
        s
    }
    #[inline]
    pub fn skip(&mut self, wt: u8) {
        match wt {
            0 => {
                self.varint();
            }
            1 => self.p += 8,
            2 => {
                let l = self.varint() as usize;
                self.p += l;
            }
            5 => self.p += 4,
            _ => panic!("unsupported protobuf wire type {wt}"),
        }
    }
}

/// zigzag decoding of sint32/sint64
#[inline(always)]
pub fn zz(n: u64) -> i64 {
    ((n >> 1) as i64) ^ -((n & 1) as i64)
}

#[inline]
pub fn push_packed_u32(b: &[u8], out: &mut Vec<u32>) {
    let mut p = Pb::new(b);
    while !p.done() {
        out.push(p.varint() as u32);
    }
}

/// append a packed, delta-coded sint64 field to `out`
#[inline]
pub fn push_delta(b: &[u8], out: &mut Vec<i64>) {
    let mut p = Pb::new(b);
    let mut acc = 0i64;
    while !p.done() {
        acc += zz(p.varint());
        out.push(acc);
    }
}

/// first value of a packed, delta-coded sint64 field
#[inline]
pub fn first_delta(b: &[u8]) -> Option<i64> {
    if b.is_empty() {
        None
    } else {
        Some(zz(Pb::new(b).varint()))
    }
}

// ---------------------------------------------------------------- blob index

#[derive(Clone, Copy, Debug)]
pub struct BlobRef {
    pub off: u64,
    pub len: u32,
    pub data: bool, // OSMData (true) or OSMHeader (false)
}

/// Walk the blob headers with small positioned reads; the blob data itself is not read.
fn index_file(f: &File) -> Result<Vec<BlobRef>, String> {
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    let mut v = Vec::new();
    let mut pos = 0u64;
    let mut hb = Vec::new();
    while pos + 4 <= len {
        let mut l4 = [0u8; 4];
        f.read_exact_at(&mut l4, pos).map_err(|e| e.to_string())?;
        let hl = u32::from_be_bytes(l4) as usize;
        hb.resize(hl, 0);
        f.read_exact_at(&mut hb, pos + 4).map_err(|e| format!("truncated blob header at byte {pos}: {e}"))?;
        let mut pb = Pb::new(&hb);
        let mut is_data = false;
        let mut ds = 0u64;
        while !pb.done() {
            let (fno, wt) = pb.key();
            match (fno, wt) {
                (1, 2) => is_data = pb.bytes() == b"OSMData",
                (3, 0) => ds = pb.varint(),
                _ => pb.skip(wt),
            }
        }
        let off = pos + 4 + hl as u64;
        if off + ds > len {
            return Err(format!("truncated blob at byte {off}"));
        }
        v.push(BlobRef { off, len: ds as u32, data: is_data });
        pos = off + ds;
    }
    Ok(v)
}

/// The blob index, from `cache` when it matches the file's size and modification time, else built and saved there.
/// Indexing the planet is about 53,000 x 2 dependent small reads (~9 s); the cached index loads in milliseconds.
pub fn index(f: &File, cache: &Path) -> Result<(Vec<BlobRef>, bool), String> {
    let md = f.metadata().map_err(|e| e.to_string())?;
    let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let stamp = [md.len(), mtime];
    if let Ok(b) = fs::read(cache) {
        if b.len() >= 16 && (b.len() - 16) % 13 == 0 && u64::from_le_bytes(b[0..8].try_into().unwrap()) == stamp[0]
            && u64::from_le_bytes(b[8..16].try_into().unwrap()) == stamp[1]
        {
            let v = b[16..]
                .as_chunks::<13>().0.iter()
                .map(|c| BlobRef {
                    off: u64::from_le_bytes(c[0..8].try_into().unwrap()),
                    len: u32::from_le_bytes(c[8..12].try_into().unwrap()),
                    data: c[12] != 0,
                })
                .collect();
            return Ok((v, true));
        }
    }
    let v = index_file(f)?;
    let mut out = Vec::with_capacity(16 + v.len() * 13);
    out.extend_from_slice(&stamp[0].to_le_bytes());
    out.extend_from_slice(&stamp[1].to_le_bytes());
    for r in &v {
        out.extend_from_slice(&r.off.to_le_bytes());
        out.extend_from_slice(&r.len.to_le_bytes());
        out.push(r.data as u8);
    }
    if let Some(d) = cache.parent() {
        let _ = fs::create_dir_all(d);
    }
    let tmp = cache.with_extension("tmp");
    if fs::File::create(&tmp).and_then(|mut w| w.write_all(&out)).is_ok() {
        let _ = fs::rename(&tmp, cache);
    }
    Ok((v, false))
}

// ---------------------------------------------------------------- reading and decompressing blobs

struct Tl {
    dec: Decompressor,
    raw: Vec<u8>,
    out: Vec<u8>,
}

thread_local! {
    static TL: RefCell<Tl> = RefCell::new(Tl { dec: Decompressor::new(), raw: Vec::new(), out: Vec::new() });
}

/// Read and decompress one blob into this thread's buffer and call `func` on the PrimitiveBlock bytes.
/// `func` must not call `with_block` itself (the buffer is borrowed while it runs).
pub fn with_block<R>(f: &File, r: &BlobRef, func: impl FnOnce(&[u8]) -> R) -> R {
    TL.with(|tl| {
        let mut tl = tl.borrow_mut();
        let Tl { dec, raw, out } = &mut *tl;
        let len = r.len as usize;
        if raw.len() < len {
            raw.resize(len, 0);
        }
        f.read_exact_at(&mut raw[..len], r.off).expect("read blob");
        let mut pb = Pb::new(&raw[..len]);
        let (mut plain, mut z, mut size) = (None, None, 0usize);
        while !pb.done() {
            let (fno, wt) = pb.key();
            match (fno, wt) {
                (1, 2) => plain = Some(pb.bytes()),
                (2, 0) => size = pb.varint() as usize,
                (3, 2) => z = Some(pb.bytes()),
                (4..=7, 2) => panic!("blob compression other than zlib (lzma, bzip2, lz4, zstd) is not supported"),
                _ => pb.skip(wt),
            }
        }
        if let Some(p) = plain {
            return func(p);
        }
        let z = z.expect("blob without data");
        if out.len() < size {
            out.resize(size, 0);
        }
        let n = dec.zlib_decompress(z, &mut out[..size]).expect("zlib decompression failed");
        func(&out[..n])
    })
}

/// Does the OSMHeader say the file is sorted by type then id (nodes, then ways, then relations)? The planet is.
pub fn header_sorted(f: &File, r: &BlobRef) -> bool {
    with_block(f, r, |raw| {
        let mut pb = Pb::new(raw);
        while !pb.done() {
            let (fno, wt) = pb.key();
            if (fno == 4 || fno == 5) && wt == 2 {
                if pb.bytes() == b"Sort.Type_then_ID" {
                    return true;
                }
            } else {
                pb.skip(wt);
            }
        }
        false
    })
}

// ---------------------------------------------------------------- primitive blocks

pub struct Block<'a> {
    pub strings: Vec<&'a [u8]>,
    pub groups: Vec<&'a [u8]>,
    pub gran: i64,
    pub lat_off: i64,
    pub lon_off: i64,
}

impl<'a> Block<'a> {
    pub fn parse(b: &'a [u8]) -> Self {
        let mut blk = Block { strings: Vec::new(), groups: Vec::new(), gran: 100, lat_off: 0, lon_off: 0 };
        let mut pb = Pb::new(b);
        while !pb.done() {
            let (f, wt) = pb.key();
            match (f, wt) {
                (1, 2) => {
                    let mut st = Pb::new(pb.bytes());
                    while !st.done() {
                        let (f2, wt2) = st.key();
                        if f2 == 1 && wt2 == 2 {
                            blk.strings.push(st.bytes());
                        } else {
                            st.skip(wt2);
                        }
                    }
                }
                (2, 2) => blk.groups.push(pb.bytes()),
                (17, 0) => blk.gran = pb.varint() as i64,
                (19, 0) => blk.lat_off = pb.varint() as i64,
                (20, 0) => blk.lon_off = pb.varint() as i64,
                _ => pb.skip(wt),
            }
        }
        blk
    }
    #[inline]
    pub fn s(&self, i: u32) -> &'a [u8] {
        self.strings.get(i as usize).copied().unwrap_or(b"")
    }
    /// nanodegrees -> 1e-7 degrees (exact for the planet's granularity of 100)
    #[inline]
    pub fn lat7(&self, v: i64) -> i32 {
        ((self.lat_off + self.gran * v) / 100) as i32
    }
    #[inline]
    pub fn lon7(&self, v: i64) -> i32 {
        ((self.lon_off + self.gran * v) / 100) as i32
    }
}

pub const K_NODES: u8 = 0;
pub const K_WAYS: u8 = 1;
pub const K_RELS: u8 = 2;
pub const K_OTHER: u8 = 3;

/// element type of a PrimitiveGroup
pub fn group_kind(g: &[u8]) -> u8 {
    let mut pb = Pb::new(g);
    while !pb.done() {
        let (f, wt) = pb.key();
        match f {
            1 | 2 => return K_NODES,
            3 => return K_WAYS,
            4 => return K_RELS,
            _ => pb.skip(wt),
        }
    }
    K_OTHER
}

/// element type of the first group of a blob (used to find where nodes, ways and relations start in a sorted file)
pub fn blob_kind(f: &File, r: &BlobRef) -> u8 {
    with_block(f, r, |raw| Block::parse(raw).groups.first().map(|g| group_kind(g)).unwrap_or(K_OTHER))
}

/// first blob index whose first group kind is >= `kind` (the file is sorted by type)
pub fn first_of(f: &File, data: &[BlobRef], kind: u8) -> usize {
    let (mut lo, mut hi) = (0usize, data.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        let k = blob_kind(f, &data[mid]);
        if k < kind || k == K_OTHER && kind != K_OTHER {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

// ---------------------------------------------------------------- element iteration

/// Every node of a group (dense or plain): f(id, lat_raw, lon_raw, interleaved key/value string ids).
/// `want_tags` false skips the key/value stream entirely (an empty slice is passed).
/// Coordinates are raw block units; convert with `Block::lat7` / `lon7`.
pub fn for_each_node<F: FnMut(i64, i64, i64, &[u32])>(g: &[u8], want_tags: bool, kvbuf: &mut Vec<u32>, mut f: F) {
    let mut pb = Pb::new(g);
    while !pb.done() {
        let (fno, wt) = pb.key();
        match (fno, wt) {
            (2, 2) => {
                let d = pb.bytes();
                let mut q = Pb::new(d);
                let (mut ids, mut lats, mut lons, mut kv): (&[u8], &[u8], &[u8], &[u8]) = (b"", b"", b"", b"");
                while !q.done() {
                    let (f2, wt2) = q.key();
                    match (f2, wt2) {
                        (1, 2) => ids = q.bytes(),
                        (8, 2) => lats = q.bytes(),
                        (9, 2) => lons = q.bytes(),
                        (10, 2) => kv = q.bytes(),
                        _ => q.skip(wt2),
                    }
                }
                let (mut pi, mut pla, mut plo) = (Pb::new(ids), Pb::new(lats), Pb::new(lons));
                let (mut id, mut la, mut lo) = (0i64, 0i64, 0i64);
                if want_tags && !kv.is_empty() {
                    let mut pk = Pb::new(kv);
                    while !pi.done() {
                        id += zz(pi.varint());
                        la += zz(pla.varint());
                        lo += zz(plo.varint());
                        kvbuf.clear();
                        while !pk.done() {
                            let k = pk.varint() as u32;
                            if k == 0 {
                                break;
                            }
                            kvbuf.push(k);
                            kvbuf.push(pk.varint() as u32);
                        }
                        f(id, la, lo, kvbuf);
                    }
                } else {
                    while !pi.done() {
                        id += zz(pi.varint());
                        la += zz(pla.varint());
                        lo += zz(plo.varint());
                        f(id, la, lo, &[]);
                    }
                }
            }
            (1, 2) => {
                // a plain Node (rare in planet files)
                let m = pb.bytes();
                let mut q = Pb::new(m);
                let (mut id, mut la, mut lo) = (0i64, 0i64, 0i64);
                let (mut keys, mut vals) = (Vec::new(), Vec::new());
                while !q.done() {
                    let (f2, wt2) = q.key();
                    match (f2, wt2) {
                        (1, 0) => id = zz(q.varint()),
                        (2, 2) => push_packed_u32(q.bytes(), &mut keys),
                        (3, 2) => push_packed_u32(q.bytes(), &mut vals),
                        (8, 0) => la = zz(q.varint()),
                        (9, 0) => lo = zz(q.varint()),
                        _ => q.skip(wt2),
                    }
                }
                kvbuf.clear();
                if want_tags {
                    for (k, v) in keys.iter().zip(vals.iter()) {
                        kvbuf.push(*k);
                        kvbuf.push(*v);
                    }
                }
                f(id, la, lo, kvbuf);
            }
            _ => pb.skip(wt),
        }
    }
}

/// first node id of a group, without decoding the rest
pub fn first_node_id(g: &[u8]) -> Option<i64> {
    let mut pb = Pb::new(g);
    while !pb.done() {
        let (fno, wt) = pb.key();
        match (fno, wt) {
            (2, 2) => {
                let mut q = Pb::new(pb.bytes());
                while !q.done() {
                    let (f2, wt2) = q.key();
                    if (f2, wt2) == (1, 2) {
                        return first_delta(q.bytes());
                    }
                    q.skip(wt2);
                }
            }
            (1, 2) => {
                let mut q = Pb::new(pb.bytes());
                while !q.done() {
                    let (f2, wt2) = q.key();
                    if (f2, wt2) == (1, 0) {
                        return Some(zz(q.varint()));
                    }
                    q.skip(wt2);
                }
            }
            _ => pb.skip(wt),
        }
    }
    None
}

/// Every way of a group: f(id, key ids, value ids, raw packed refs). Refs are decoded by the caller only when needed.
pub fn for_each_way<'a, F: FnMut(i64, &[u32], &[u32], &'a [u8])>(g: &'a [u8], keys: &mut Vec<u32>, vals: &mut Vec<u32>, mut f: F) {
    let mut pb = Pb::new(g);
    while !pb.done() {
        let (fno, wt) = pb.key();
        if fno == 3 && wt == 2 {
            let m = pb.bytes();
            let mut w = Pb::new(m);
            let mut id = 0i64;
            let mut refs: &'a [u8] = b"";
            keys.clear();
            vals.clear();
            while !w.done() {
                let (f2, wt2) = w.key();
                match (f2, wt2) {
                    (1, 0) => id = w.varint() as i64,
                    (2, 2) => push_packed_u32(w.bytes(), keys),
                    (2, 0) => keys.push(w.varint() as u32),
                    (3, 2) => push_packed_u32(w.bytes(), vals),
                    (3, 0) => vals.push(w.varint() as u32),
                    (8, 2) => refs = w.bytes(),
                    _ => w.skip(wt2),
                }
            }
            f(id, &keys[..], &vals[..], refs);
        } else {
            pb.skip(wt);
        }
    }
}

pub struct RelBufs {
    pub keys: Vec<u32>,
    pub vals: Vec<u32>,
    pub roles: Vec<u32>,
    pub types: Vec<u32>,
    pub memids: Vec<i64>,
}

impl RelBufs {
    pub fn new() -> Self {
        RelBufs { keys: Vec::new(), vals: Vec::new(), roles: Vec::new(), types: Vec::new(), memids: Vec::new() }
    }
}

/// Every relation of a group: f(id, bufs). Members are decoded only for relations whose tags pass `want`.
pub fn for_each_relation<W: FnMut(&[u32], &[u32]) -> bool, F: FnMut(i64, &RelBufs)>(g: &[u8], b: &mut RelBufs, mut want: W, mut f: F) {
    let mut pb = Pb::new(g);
    while !pb.done() {
        let (fno, wt) = pb.key();
        if fno == 4 && wt == 2 {
            let m = pb.bytes();
            let mut r = Pb::new(m);
            let mut id = 0i64;
            b.keys.clear();
            b.vals.clear();
            b.roles.clear();
            b.types.clear();
            b.memids.clear();
            let (mut roles, mut memraw, mut types): (&[u8], &[u8], &[u8]) = (b"", b"", b"");
            while !r.done() {
                let (f2, wt2) = r.key();
                match (f2, wt2) {
                    (1, 0) => id = r.varint() as i64,
                    (2, 2) => push_packed_u32(r.bytes(), &mut b.keys),
                    (3, 2) => push_packed_u32(r.bytes(), &mut b.vals),
                    (8, 2) => roles = r.bytes(),
                    (9, 2) => memraw = r.bytes(),
                    (10, 2) => types = r.bytes(),
                    _ => r.skip(wt2),
                }
            }
            if !want(&b.keys, &b.vals) {
                continue;
            }
            push_packed_u32(roles, &mut b.roles);
            push_packed_u32(types, &mut b.types);
            push_delta(memraw, &mut b.memids);
            f(id, b);
        } else {
            pb.skip(wt);
        }
    }
}
