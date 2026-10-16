//! The world rail cache: every rail candidate of the planet with coordinates resolved, in one memory-mapped file.
//!
//! Layout (little-endian): magic, then twelve (offset, length) pairs, then the sections, each aligned to 8 bytes.
//!
//!   WAY_IDX   [WayIdx]   sorted by id: id, record offset, bbox
//!   WAY_DAT   records    tags; n; n x (zz delta node id, zz delta lat, zz delta lon)
//!   NODE_IDX  [NodeIdx]  sorted by id: tagged rail nodes (stations, stop positions, stations being built)
//!   NODE_DAT  records    tags
//!   REL_IDX   [RelIdx]   sorted by id: id, record offset, bbox of the located members (and of member relations)
//!   REL_DAT   records    tags; n; n x (type, zz delta id, role, and for node members zz lat, zz lon)
//!   BWAY_IDX  [BWayIdx]  sorted by id: member ways of admin boundaries (levels 2 and 4)
//!   BWAY_DAT  records    n; n x (zz delta lat, zz delta lon), simplified when the cache is built (no node ids, no tags)
//!   BREL_IDX  [RelIdx]   sorted by id: admin boundary relations, bbox of their ways
//!   BREL_DAT  records    tags; n; n x (zz delta way id, role); way members only
//!   PLACE_IDX [NodeIdx]  sorted by id: place=city and place=town nodes (for the places step)
//!   PLACE_DAT records    tags (`rules::place_key` only: place, name, name:en, int_name, population, capital, wikidata)
//!
//! Tags are: count, then (len, key bytes, len, value bytes) per tag, in OSM order, only keys `rules::cache_key` keeps.
//! Coordinates are 1e-7 degrees; a node missing from the planet has lat = i32::MIN.
//! The fixed-size index tables are read in place from the map; a cut tests the bbox first and decodes only records near its region.

use memmap2::Mmap;
use std::fs::File;
use std::path::Path;

pub const MAGIC: &[u8; 8] = b"RGRAIL03";
pub const NSEC: usize = 12;
pub const HEADER: usize = 8 + NSEC * 16;
pub const MISSING: i32 = i32::MIN;

/// bbox: min lat, min lon, max lat, max lon; EMPTY when nothing is located
pub type BBox = [i32; 4];
pub const EMPTY: BBox = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];

#[inline]
pub fn bbox_add(b: &mut BBox, lat: i32, lon: i32) {
    if lat == MISSING {
        return;
    }
    b[0] = b[0].min(lat);
    b[1] = b[1].min(lon);
    b[2] = b[2].max(lat);
    b[3] = b[3].max(lon);
}
#[inline]
pub fn bbox_union(b: &mut BBox, o: &BBox) {
    b[0] = b[0].min(o[0]);
    b[1] = b[1].min(o[1]);
    b[2] = b[2].max(o[2]);
    b[3] = b[3].max(o[3]);
}
#[inline]
pub fn bbox_hits(a: &BBox, b: &BBox) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WayIdx {
    pub id: i64,
    pub off: u64,
    pub bbox: BBox,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NodeIdx {
    pub id: i64,
    pub off: u64,
    pub lat: i32,
    pub lon: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BWayIdx {
    pub id: i64,
    pub off: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RelIdx {
    pub id: i64,
    pub off: u64,
    pub bbox: BBox,
}

// ---------------------------------------------------------------- varints

#[inline]
pub fn put_var(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}
#[inline]
pub fn put_zz(out: &mut Vec<u8>, v: i64) {
    put_var(out, ((v << 1) ^ (v >> 63)) as u64);
}
#[inline]
pub fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    put_var(out, b.len() as u64);
    out.extend_from_slice(b);
}

pub struct Rd<'a> {
    pub b: &'a [u8],
    pub p: usize,
}

impl<'a> Rd<'a> {
    #[inline(always)]
    pub fn var(&mut self) -> u64 {
        let mut r = 0u64;
        let mut s = 0;
        loop {
            let c = self.b[self.p];
            self.p += 1;
            r |= ((c & 0x7f) as u64) << s;
            if c < 0x80 {
                return r;
            }
            s += 7;
        }
    }
    #[inline(always)]
    pub fn zz(&mut self) -> i64 {
        let n = self.var();
        ((n >> 1) as i64) ^ -((n & 1) as i64)
    }
    #[inline]
    pub fn bytes(&mut self) -> &'a [u8] {
        let l = self.var() as usize;
        let s = &self.b[self.p..self.p + l];
        self.p += l;
        s
    }
    pub fn tags(&mut self) -> Vec<(&'a [u8], &'a [u8])> {
        let n = self.var() as usize;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            let k = self.bytes();
            let val = self.bytes();
            v.push((k, val));
        }
        v
    }
}

// ---------------------------------------------------------------- decoded records

pub struct Way<'a> {
    pub tags: Vec<(&'a [u8], &'a [u8])>,
    pub refs: Vec<i64>,
    pub ll: Vec<(i32, i32)>, // (lat, lon) per ref; MISSING lat when the node is not in the planet
}

pub struct Member<'a> {
    pub t: u8, // 0 node, 1 way, 2 relation
    pub id: i64,
    pub role: &'a [u8],
    pub lat: i32, // node members only
    pub lon: i32,
}

pub struct Rel<'a> {
    pub tags: Vec<(&'a [u8], &'a [u8])>,
    pub mem: Vec<Member<'a>>,
}

// ---------------------------------------------------------------- reader

pub struct Cache {
    map: Mmap,
    sec: [(usize, usize); NSEC],
}

unsafe fn table<T>(b: &[u8]) -> &[T] {
    let n = b.len() / std::mem::size_of::<T>();
    assert!(b.as_ptr() as usize % std::mem::align_of::<T>() == 0, "cache section misaligned");
    std::slice::from_raw_parts(b.as_ptr() as *const T, n)
}

impl Cache {
    pub fn open(path: &Path) -> Result<Cache, String> {
        let f = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
        let map = unsafe { Mmap::map(&f) }.map_err(|e| e.to_string())?;
        if map.len() >= 8 && &map[0..6] == b"RGRAIL" && &map[0..8] != MAGIC {
            return Err(format!(
                "{} is an older cache format ({}, this build reads {}): rebuild it with `rail-extract cache --planet <planet>.osm.pbf` (about a minute)",
                path.display(),
                String::from_utf8_lossy(&map[0..8]),
                String::from_utf8_lossy(MAGIC)
            ));
        }
        if map.len() < HEADER || &map[0..8] != MAGIC {
            return Err(format!("{} is not a rail cache: rebuild it with `rail-extract cache`", path.display()));
        }
        let mut sec = [(0usize, 0usize); NSEC];
        for (i, s) in sec.iter_mut().enumerate() {
            let o = 8 + i * 16;
            let off = u64::from_le_bytes(map[o..o + 8].try_into().unwrap()) as usize;
            let len = u64::from_le_bytes(map[o + 8..o + 16].try_into().unwrap()) as usize;
            if off + len > map.len() {
                return Err("truncated rail cache".into());
            }
            *s = (off, len);
        }
        Ok(Cache { map, sec })
    }
    #[inline]
    fn s(&self, i: usize) -> &[u8] {
        let (o, l) = self.sec[i];
        &self.map[o..o + l]
    }
    pub fn way_idx(&self) -> &[WayIdx] {
        unsafe { table(self.s(0)) }
    }
    pub fn node_idx(&self) -> &[NodeIdx] {
        unsafe { table(self.s(2)) }
    }
    pub fn rel_idx(&self) -> &[RelIdx] {
        unsafe { table(self.s(4)) }
    }
    pub fn place_idx(&self) -> &[NodeIdx] {
        unsafe { table(self.s(10)) }
    }
    pub fn place_tags(&self, i: usize) -> Vec<(&[u8], &[u8])> {
        Rd { b: self.s(11), p: self.place_idx()[i].off as usize }.tags()
    }
    pub fn way(&self, i: usize) -> Way<'_> {
        let mut r = Rd { b: self.s(1), p: self.way_idx()[i].off as usize };
        let tags = r.tags();
        let n = r.var() as usize;
        let (mut refs, mut ll) = (Vec::with_capacity(n), Vec::with_capacity(n));
        let (mut id, mut la, mut lo) = (0i64, 0i64, 0i64);
        for _ in 0..n {
            id += r.zz();
            la += r.zz();
            lo += r.zz();
            refs.push(id);
            ll.push((la as i32, lo as i32));
        }
        Way { tags, refs, ll }
    }
    /// coordinates only (skips the tags and node ids' use); for the region test
    pub fn way_ll(&self, i: usize, ll: &mut Vec<(i32, i32)>) {
        let mut r = Rd { b: self.s(1), p: self.way_idx()[i].off as usize };
        let n = r.var() as usize;
        for _ in 0..n {
            let _ = r.bytes();
            let _ = r.bytes();
        }
        let n = r.var() as usize;
        ll.clear();
        let (mut la, mut lo) = (0i64, 0i64);
        for _ in 0..n {
            r.var();
            la += r.zz();
            lo += r.zz();
            ll.push((la as i32, lo as i32));
        }
    }
    pub fn node_tags(&self, i: usize) -> Vec<(&[u8], &[u8])> {
        Rd { b: self.s(3), p: self.node_idx()[i].off as usize }.tags()
    }
    pub fn rel(&self, i: usize) -> Rel<'_> {
        let mut r = Rd { b: self.s(5), p: self.rel_idx()[i].off as usize };
        let tags = r.tags();
        let n = r.var() as usize;
        let mut mem = Vec::with_capacity(n);
        let mut id = 0i64;
        for _ in 0..n {
            let t = r.b[r.p];
            r.p += 1;
            id += r.zz();
            let role = r.bytes();
            let (lat, lon) = if t == 0 { (r.zz() as i32, r.zz() as i32) } else { (0, 0) };
            mem.push(Member { t, id, role, lat, lon });
        }
        Rel { tags, mem }
    }
    pub fn bway_idx(&self) -> &[BWayIdx] {
        unsafe { table(self.s(6)) }
    }
    pub fn brel_idx(&self) -> &[RelIdx] {
        unsafe { table(self.s(8)) }
    }
    /// simplified geometry of a boundary way, by id
    pub fn bway(&self, id: i64) -> Option<Vec<(i32, i32)>> {
        let i = self.bway_idx().binary_search_by_key(&id, |w| w.id).ok()?;
        let mut r = Rd { b: self.s(7), p: self.bway_idx()[i].off as usize };
        let n = r.var() as usize;
        let mut v = Vec::with_capacity(n);
        let (mut la, mut lo) = (0i64, 0i64);
        for _ in 0..n {
            la += r.zz();
            lo += r.zz();
            v.push((la as i32, lo as i32));
        }
        Some(v)
    }
    /// a boundary relation: tags and (way id, role) members
    pub fn brel(&self, i: usize) -> (Vec<(&[u8], &[u8])>, Vec<(i64, &[u8])>) {
        let mut r = Rd { b: self.s(9), p: self.brel_idx()[i].off as usize };
        let tags = r.tags();
        let n = r.var() as usize;
        let mut mem = Vec::with_capacity(n);
        let mut id = 0i64;
        for _ in 0..n {
            id += r.zz();
            mem.push((id, r.bytes()));
        }
        (tags, mem)
    }
    pub fn find_way(&self, id: i64) -> Option<usize> {
        self.way_idx().binary_search_by_key(&id, |w| w.id).ok()
    }
}

// ---------------------------------------------------------------- writer

pub fn write(path: &Path, secs: [&[u8]; NSEC]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    {
        let mut w = std::io::BufWriter::with_capacity(8 << 20, File::create(&tmp)?);
        let mut off = HEADER;
        let mut head = Vec::with_capacity(HEADER);
        head.extend_from_slice(MAGIC);
        for s in &secs {
            head.extend_from_slice(&(off as u64).to_le_bytes());
            head.extend_from_slice(&(s.len() as u64).to_le_bytes());
            off += (s.len() + 7) / 8 * 8;
        }
        w.write_all(&head)?;
        for s in &secs {
            w.write_all(s)?;
            w.write_all(&[0u8; 8][..(8 - s.len() % 8) % 8])?;
        }
        w.flush()?;
    }
    // written to a temporary name and renamed, so an interrupted build never leaves a half-written cache in place
    std::fs::rename(&tmp, path)
}

/// a slice of plain-old-data structs as bytes, for writing the index tables
pub fn as_bytes<T: Copy>(v: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}
