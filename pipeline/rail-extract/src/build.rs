//! Planet -> world rail cache, in one read of the planet.
//!
//! Selection is by tags only; geography is left to the cut. That is what makes one cache serve every region: the planet is read
//! once a month, and a region (a city, a country, the world) is cut from the cache in about a second.
//!
//! Passes, in this order (the planet is sorted nodes, ways, relations; each pass decodes only its own blobs, in parallel):
//!   0. nodes, tags only: the Wikidata ids of the place=city nodes, so that pass 1 can keep the boundary of each city (admin levels 6 to 9).
//!   1. relations: side-table relations (stop areas and groups, rail routes and route masters, route=railway) and area relations
//!      (station, platform, yard, railway land, station building, aerodrome, terminal multipolygons). Their member ways and
//!      member nodes are needed even when untagged (multipolygon outlines; route stops for the smart-extract membership rule).
//!   2. ways: rail candidates by tag, plus the member ways of pass 1. Node refs are decoded only for kept ways.
//!   3. nodes, once: coordinates of every node a kept way or relation needs, the tagged rail nodes, and the place=city / town nodes.
//!
//! Tag-only selection (no roads) needs only one node pass after the tags-only pass 0.

use crate::cache::{self, *};
use crate::pbf::*;
use crate::rules::{self, Tags};
use rayon::prelude::*;
use std::fs::File;
use std::path::Path;
use std::time::Instant;

/// block tags as separate key and value string ids (ways, relations)
struct Sep<'a, 'b> {
    blk: &'b Block<'a>,
    keys: &'b [u32],
    vals: &'b [u32],
}

impl Tags for Sep<'_, '_> {
    #[inline]
    fn get(&self, k: &[u8]) -> Option<&[u8]> {
        for (i, &kk) in self.keys.iter().enumerate() {
            if self.blk.s(kk) == k {
                return self.vals.get(i).map(|&v| self.blk.s(v));
            }
        }
        None
    }
}

/// block tags as interleaved key, value string ids (dense nodes)
struct Kv<'a, 'b> {
    blk: &'b Block<'a>,
    kv: &'b [u32],
}

impl Tags for Kv<'_, '_> {
    #[inline]
    fn get(&self, k: &[u8]) -> Option<&[u8]> {
        self.kv.as_chunks::<2>().0.iter().find(|p| self.blk.s(p[0]) == k).map(|p| self.blk.s(p[1]))
    }
}

/// string-table ids of the given keys in this block (a short list; an element whose keys hit none of them is skipped unread)
fn key_ids(blk: &Block, want: &[&[u8]]) -> Vec<u32> {
    let mut v = Vec::new();
    for (i, s) in blk.strings.iter().enumerate() {
        if want.contains(s) {
            v.push(i as u32);
        }
    }
    v
}

fn ser_tags<'a>(out: &mut Vec<u8>, pairs: impl Iterator<Item = (&'a [u8], &'a [u8])> + Clone) {
    let n = pairs.clone().filter(|(k, _)| rules::cache_key(k)).count();
    put_var(out, n as u64);
    for (k, v) in pairs.filter(|(k, _)| rules::cache_key(k)) {
        put_bytes(out, k);
        put_bytes(out, v);
    }
}

struct RelOut {
    id: i64,
    tags: Vec<u8>,
    mem: Vec<(u8, i64)>,
    roles: Vec<u8>, // one length-prefixed role per member
    bnd: bool,      // an admin boundary (outline), not a rail relation
}

#[derive(Default)]
struct WayBlk {
    ids: Vec<i64>,
    tags: Vec<u8>,
    tag_end: Vec<usize>,
    refs: Vec<i64>,
    ref_end: Vec<usize>,
    // member ways of admin boundaries: node refs only
    bnd_ids: Vec<i64>,
    bnd_refs: Vec<i64>,
    bnd_end: Vec<usize>,
}

#[derive(Default)]
struct NodeBlk {
    found: Vec<(u32, i32, i32)>, // need index, lat, lon
    tagged: Vec<(i64, i32, i32)>,
    tags: Vec<u8>,
    tag_end: Vec<usize>,
    places: Vec<(i64, i32, i32)>, // place=city / town nodes
    ptags: Vec<u8>,
    ptag_end: Vec<usize>,
    villages: usize, // place=village nodes seen (counted, not kept)
}

/// Wikidata ids of the place=city nodes (a pass over the node blobs, tags only), for the cities' own boundaries
fn city_wikidata(f: &File, nodes: &[BlobRef]) -> std::collections::HashSet<Vec<u8>> {
    nodes
        .par_iter()
        .map(|r| {
            with_block(f, r, |raw| {
                let blk = Block::parse(raw);
                let (place, wd) = (key_ids(&blk, &[b"place"]), key_ids(&blk, &[b"wikidata"]));
                let mut out: Vec<Vec<u8>> = Vec::new();
                if place.is_empty() || wd.is_empty() {
                    return out;
                }
                let mut kvb = Vec::new();
                for g in &blk.groups {
                    if group_kind(g) != K_NODES {
                        continue;
                    }
                    for_each_node(g, true, &mut kvb, |_, _, _, kv| {
                        if kv.is_empty() {
                            return;
                        }
                        let city = kv.as_chunks::<2>().0.iter().any(|p| place.contains(&p[0]) && blk.s(p[1]) == b"city");
                        if city {
                            if let Some(p) = kv.as_chunks::<2>().0.iter().find(|p| wd.contains(&p[0])) {
                                out.push(blk.s(p[1]).to_vec());
                            }
                        }
                    });
                }
                out
            })
        })
        .flatten_iter()
        .collect()
}

/// The kept ways of pass 2 as global arrays: tags and node refs per way, and the boundary ways' node refs.
struct WayData {
    ids: Vec<i64>,
    tags: Vec<u8>,
    tag_end: Vec<usize>,
    refs: Vec<i64>,
    ref_end: Vec<usize>,
    bnd_ids: Vec<i64>,
    bnd_refs: Vec<i64>,
    bnd_end: Vec<usize>,
}

/// Which node a slot (a way ref, a relation member node, a boundary way ref) needs: an index into the sorted `need` list.
struct NodeSlots {
    need: Vec<i64>,
    slot_need: Vec<u32>,
    bslot0: usize, // the first boundary-way slot (they come last)
}

/// Pass 1: the side-table and area relations, and the admin boundaries.
fn relations_pass(f: &File, blobs: &[BlobRef], city_wd: &std::collections::HashSet<Vec<u8>>) -> Vec<RelOut> {
    blobs
        .par_iter()
        .map(|r| {
            with_block(f, r, |raw| {
                let blk = Block::parse(raw);
                let ty = key_ids(&blk, &[b"type"]);
                let mut out = Vec::new();
                let mut b = RelBufs::new();
                if ty.is_empty() {
                    return out;
                }
                for g in &blk.groups {
                    if group_kind(g) != K_RELS {
                        continue;
                    }
                    for_each_relation(
                        g,
                        &mut b,
                        |keys, vals| {
                            keys.iter().any(|k| ty.contains(k)) && {
                                let t = Sep { blk: &blk, keys, vals };
                                rules::rel_kind(&t).is_some() || rules::rel_area_wanted(&t) || rules::admin_boundary(&t) || rules::city_boundary(&t, city_wd)
                            }
                        },
                        |id, b| {
                            let sep = Sep { blk: &blk, keys: &b.keys, vals: &b.vals };
                            let bnd = rules::admin_boundary(&sep) || rules::city_boundary(&sep, city_wd);
                            let mut tags = Vec::new();
                            ser_tags(&mut tags, b.keys.iter().zip(b.vals.iter()).map(|(&k, &v)| (blk.s(k), blk.s(v))));
                            let mut roles = Vec::new();
                            let mut mem = Vec::with_capacity(b.memids.len());
                            for i in 0..b.memids.len() {
                                mem.push((b.types.get(i).copied().unwrap_or(0) as u8, b.memids[i]));
                                put_bytes(&mut roles, blk.s(b.roles.get(i).copied().unwrap_or(0)));
                            }
                            out.push(RelOut { id, tags, mem, roles, bnd });
                        },
                    );
                }
                out
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect()
}

/// Sorted, de-duplicated ids of the way members of the relations.
fn member_way_ids(rels: &[RelOut]) -> Vec<i64> {
    let mut v: Vec<i64> = rels.iter().flat_map(|r| r.mem.iter().filter(|m| m.0 == 1).map(|m| m.1)).collect();
    v.par_sort_unstable();
    v.dedup();
    v
}

/// Pass 2: rail candidate ways by tag plus the member ways of pass 1 (`mem_ways`), and the boundary ways (`bnd_ways`); both sorted.
fn ways_pass(f: &File, blobs: &[BlobRef], mem_ways: &[i64], bnd_ways: &[i64]) -> WayData {
    let blks: Vec<WayBlk> = blobs
        .par_iter()
        .map(|r| {
            with_block(f, r, |raw| {
                let blk = Block::parse(raw);
                let quick = key_ids(&blk, &[b"railway", b"landuse", b"building", b"public_transport", b"aeroway"]);
                let mut o = WayBlk::default();
                let (mut keys, mut vals) = (Vec::new(), Vec::new());
                let (mut j, mut jb) = (usize::MAX, usize::MAX);
                for g in &blk.groups {
                    if group_kind(g) != K_WAYS {
                        continue;
                    }
                    for_each_way(g, &mut keys, &mut vals, |id, keys, vals, refs_raw| {
                        // member of a kept relation? (ways and mem_ways are both sorted by id: walk them together)
                        if j == usize::MAX {
                            j = mem_ways.partition_point(|&x| x < id);
                            jb = bnd_ways.partition_point(|&x| x < id);
                        }
                        while j < mem_ways.len() && mem_ways[j] < id {
                            j += 1;
                        }
                        while jb < bnd_ways.len() && bnd_ways[jb] < id {
                            jb += 1;
                        }
                        if jb < bnd_ways.len() && bnd_ways[jb] == id {
                            o.bnd_ids.push(id);
                            push_delta(refs_raw, &mut o.bnd_refs);
                            o.bnd_end.push(o.bnd_refs.len());
                        }
                        let member = j < mem_ways.len() && mem_ways[j] == id;
                        let keep = member || (keys.iter().any(|k| quick.contains(k)) && rules::way_wanted(&Sep { blk: &blk, keys, vals }));
                        if !keep {
                            return;
                        }
                        o.ids.push(id);
                        ser_tags(&mut o.tags, keys.iter().zip(vals.iter()).map(|(&k, &v)| (blk.s(k), blk.s(v))));
                        o.tag_end.push(o.tags.len());
                        push_delta(refs_raw, &mut o.refs);
                        o.ref_end.push(o.refs.len());
                    });
                }
                o
            })
        })
        .collect();
    // flatten into global arrays
    let nw: usize = blks.iter().map(|b| b.ids.len()).sum();
    let nr: usize = blks.iter().map(|b| b.refs.len()).sum();
    let mut w = WayData {
        ids: Vec::with_capacity(nw),
        tags: Vec::with_capacity(blks.iter().map(|b| b.tags.len()).sum()),
        tag_end: Vec::with_capacity(nw),
        refs: Vec::with_capacity(nr),
        ref_end: Vec::with_capacity(nw),
        bnd_ids: Vec::new(),
        bnd_refs: Vec::new(),
        bnd_end: Vec::new(),
    };
    for b in blks {
        let b0 = w.bnd_refs.len();
        w.bnd_ids.extend_from_slice(&b.bnd_ids);
        w.bnd_refs.extend_from_slice(&b.bnd_refs);
        w.bnd_end.extend(b.bnd_end.iter().map(|e| e + b0));
        let (t0, r0_) = (w.tags.len(), w.refs.len());
        w.ids.extend_from_slice(&b.ids);
        w.tags.extend_from_slice(&b.tags);
        w.refs.extend_from_slice(&b.refs);
        w.tag_end.extend(b.tag_end.iter().map(|e| e + t0));
        w.ref_end.extend(b.ref_end.iter().map(|e| e + r0_));
    }
    w
}

/// The nodes we need: every way ref, then every relation member node, then the boundary way refs. Each slot maps to an index in
/// the sorted, de-duplicated `need` list, so after the node pass every coordinate is found by index, with no searching.
fn node_slots(w: &WayData, rels: &[RelOut]) -> NodeSlots {
    let nr = w.refs.len();
    let mut slots: Vec<(i64, u32)> = w.refs.iter().copied().enumerate().map(|(i, id)| (id, i as u32)).collect();
    let mut mslot = nr;
    for r in rels {
        for &(t, id) in &r.mem {
            if t == 0 {
                slots.push((id, mslot as u32));
                mslot += 1;
            }
        }
    }
    let bslot0 = mslot; // boundary way refs come last
    for &id in &w.bnd_refs {
        slots.push((id, mslot as u32));
        mslot += 1;
    }
    assert!(mslot < u32::MAX as usize, "too many node slots");
    slots.par_sort_unstable();
    let mut need: Vec<i64> = Vec::with_capacity(slots.len());
    let mut slot_need = vec![0u32; mslot];
    for &(id, s) in &slots {
        if need.last() != Some(&id) {
            need.push(id);
        }
        slot_need[s as usize] = (need.len() - 1) as u32;
    }
    NodeSlots { need, slot_need, bslot0 }
}

/// Pass 3: coordinates of the needed nodes, the tagged rail nodes, and the place nodes.
fn nodes_pass(f: &File, blobs: &[BlobRef], need: &[i64]) -> Vec<NodeBlk> {
    blobs
        .par_iter()
        .map(|r| {
            with_block(f, r, |raw| {
                let blk = Block::parse(raw);
                let quick = key_ids(&blk, &[b"railway", b"public_transport", b"construction:railway", b"construction:public_transport"]);
                let mut o = NodeBlk::default();
                let place_k = key_ids(&blk, &[b"place"]);
                let mut kvb = Vec::new();
                for g in &blk.groups {
                    if group_kind(g) != K_NODES {
                        continue;
                    }
                    let Some(first) = first_node_id(g) else { continue };
                    let mut j = need.partition_point(|&x| x < first);
                    for_each_node(g, !quick.is_empty() || !place_k.is_empty(), &mut kvb, |id, la, lo, kv| {
                        while j < need.len() && need[j] < id {
                            j += 1;
                        }
                        let needed = j < need.len() && need[j] == id;
                        let tagged =
                            !kv.is_empty() && kv.as_chunks::<2>().0.iter().any(|p| quick.contains(&p[0])) && rules::node_kind(&Kv { blk: &blk, kv }).is_some();
                        let place = !kv.is_empty() && kv.as_chunks::<2>().0.iter().any(|p| place_k.contains(&p[0]) && matches!(blk.s(p[1]), b"city" | b"town"));
                        if !place && !kv.is_empty() && kv.as_chunks::<2>().0.iter().any(|p| place_k.contains(&p[0]) && blk.s(p[1]) == b"village") {
                            o.villages += 1;
                        }
                        if !needed && !tagged && !place {
                            return;
                        }
                        let (lat, lon) = (blk.lat7(la), blk.lon7(lo));
                        if place {
                            o.places.push((id, lat, lon));
                            let pairs = kv.as_chunks::<2>().0.iter().map(|p| (blk.s(p[0]), blk.s(p[1]))).filter(|(k, _)| rules::place_key(k));
                            put_var(&mut o.ptags, pairs.clone().count() as u64);
                            for (k, v) in pairs {
                                put_bytes(&mut o.ptags, k);
                                put_bytes(&mut o.ptags, v);
                            }
                            o.ptag_end.push(o.ptags.len());
                        }
                        if needed {
                            o.found.push((j as u32, lat, lon));
                        }
                        if tagged {
                            o.tagged.push((id, lat, lon));
                            ser_tags(&mut o.tags, kv.as_chunks::<2>().0.iter().map(|p| (blk.s(p[0]), blk.s(p[1]))));
                            o.tag_end.push(o.tags.len());
                        }
                    });
                }
                o
            })
        })
        .collect()
}

/// Way records: tags, then the refs with their coordinates, delta coded; and the index with each way's bbox.
fn way_records(w: &WayData, slots: &NodeSlots, need_ll: &[(i32, i32)]) -> (Vec<WayIdx>, Vec<u8>) {
    let nw = w.ids.len();
    let chunk = 1 << 14;
    let parts: Vec<(Vec<WayIdx>, Vec<u8>)> = (0..nw)
        .collect::<Vec<_>>()
        .par_chunks(chunk)
        .map(|ix| {
            let mut idx = Vec::with_capacity(ix.len());
            let mut dat = Vec::new();
            for &i in ix {
                let off = dat.len() as u64;
                let ts = if i == 0 { 0 } else { w.tag_end[i - 1] };
                dat.extend_from_slice(&w.tags[ts..w.tag_end[i]]);
                let rs = if i == 0 { 0 } else { w.ref_end[i - 1] };
                let re = w.ref_end[i];
                put_var(&mut dat, (re - rs) as u64);
                let (mut pid, mut pla, mut plo) = (0i64, 0i64, 0i64);
                let mut bb = EMPTY;
                for s in rs..re {
                    let id = w.refs[s];
                    let (la, lo) = need_ll[slots.slot_need[s] as usize];
                    put_zz(&mut dat, id - pid);
                    put_zz(&mut dat, la as i64 - pla);
                    put_zz(&mut dat, lo as i64 - plo);
                    pid = id;
                    pla = la as i64;
                    plo = lo as i64;
                    bbox_add(&mut bb, la, lo);
                }
                idx.push(WayIdx { id: w.ids[i], off, bbox: bb });
            }
            (idx, dat)
        })
        .collect();
    let mut way_idx = Vec::with_capacity(nw);
    let mut way_dat = Vec::new();
    for (idx, dat) in parts {
        let base = way_dat.len() as u64;
        way_idx.extend(idx.into_iter().map(|mut w| {
            w.off += base;
            w
        }));
        way_dat.extend_from_slice(&dat);
    }
    (way_idx, way_dat)
}

/// Relation records, and their bboxes (member nodes, member ways, then member relations, three levels deep).
fn relation_records(rels: &[RelOut], nr: usize, slots: &NodeSlots, need_ll: &[(i32, i32)], way_idx: &[WayIdx]) -> (Vec<RelIdx>, Vec<u8>) {
    let mut rel_idx = Vec::with_capacity(rels.len());
    let mut rel_dat = Vec::new();
    let mut ms = nr;
    for r in rels {
        let off = rel_dat.len() as u64;
        rel_dat.extend_from_slice(&r.tags);
        put_var(&mut rel_dat, r.mem.len() as u64);
        let mut rd = Rd { b: &r.roles, p: 0 };
        let mut pid = 0i64;
        let mut bb = EMPTY;
        for &(t, id) in &r.mem {
            rel_dat.push(t);
            put_zz(&mut rel_dat, id - pid);
            pid = id;
            put_bytes(&mut rel_dat, rd.bytes());
            match t {
                0 => {
                    let (la, lo) = need_ll[slots.slot_need[ms] as usize];
                    ms += 1;
                    put_zz(&mut rel_dat, la as i64);
                    put_zz(&mut rel_dat, lo as i64);
                    bbox_add(&mut bb, la, lo);
                }
                1 => {
                    if let Ok(i) = way_idx.binary_search_by_key(&id, |w: &WayIdx| w.id) {
                        bbox_union(&mut bb, &way_idx[i].bbox);
                    }
                }
                _ => {}
            }
        }
        rel_idx.push(RelIdx { id: r.id, off, bbox: bb });
    }
    for _ in 0..3 {
        let snapshot: Vec<BBox> = rel_idx.iter().map(|r| r.bbox).collect();
        for (k, r) in rels.iter().enumerate() {
            for &(t, id) in &r.mem {
                if t == 2 {
                    if let Ok(i) = rel_idx.binary_search_by_key(&id, |x: &RelIdx| x.id) {
                        let o = snapshot[i];
                        bbox_union(&mut rel_idx[k].bbox, &o);
                    }
                }
            }
        }
    }
    (rel_idx, rel_dat)
}

/// Admin boundary ways, simplified once here (shared borders stay shared): index, data, points before and after.
fn boundary_way_records(w: &WayData, slots: &NodeSlots, need_ll: &[(i32, i32)]) -> (Vec<BWayIdx>, Vec<u8>, usize, usize) {
    let bparts: Vec<(Vec<BWayIdx>, Vec<u8>, usize, usize)> = (0..w.bnd_ids.len())
        .collect::<Vec<_>>()
        .par_chunks(1 << 12)
        .map(|ix| {
            let (mut idx, mut dat, mut before, mut after) = (Vec::new(), Vec::new(), 0, 0);
            let mut pts = Vec::new();
            for &i in ix {
                let s0 = if i == 0 { 0 } else { w.bnd_end[i - 1] };
                pts.clear();
                pts.extend((s0..w.bnd_end[i]).map(|s| need_ll[slots.slot_need[slots.bslot0 + s] as usize]));
                if pts.iter().any(|p| p.0 == MISSING) {
                    continue; // incomplete way: its boundaries will not assemble
                }
                let keep = simplify(&pts, SIMPLIFY_DEG);
                before += pts.len();
                after += keep.len();
                idx.push(BWayIdx { id: w.bnd_ids[i], off: dat.len() as u64 });
                put_var(&mut dat, keep.len() as u64);
                let (mut pla, mut plo) = (0i64, 0i64);
                for p in keep {
                    put_zz(&mut dat, p.0 as i64 - pla);
                    put_zz(&mut dat, p.1 as i64 - plo);
                    pla = p.0 as i64;
                    plo = p.1 as i64;
                }
            }
            (idx, dat, before, after)
        })
        .collect();
    let (mut bway_idx, mut bway_dat, mut pts_before, mut pts_after) = (Vec::new(), Vec::new(), 0usize, 0usize);
    for (idx, dat, b, a) in bparts {
        let base = bway_dat.len() as u64;
        bway_idx.extend(idx.into_iter().map(|mut w| {
            w.off += base;
            w
        }));
        bway_dat.extend_from_slice(&dat);
        pts_before += b;
        pts_after += a;
    }
    (bway_idx, bway_dat, pts_before, pts_after)
}

/// Admin boundary relations: their way members and bboxes.
fn boundary_relation_records(bnd_rels: &[RelOut], w: &WayData, slots: &NodeSlots, need_ll: &[(i32, i32)]) -> (Vec<RelIdx>, Vec<u8>) {
    let mut brel_idx = Vec::with_capacity(bnd_rels.len());
    let mut brel_dat = Vec::new();
    for r in bnd_rels {
        let off = brel_dat.len() as u64;
        brel_dat.extend_from_slice(&r.tags);
        let ways: Vec<(i64, &[u8])> = {
            let mut rd = Rd { b: &r.roles, p: 0 };
            r.mem.iter().map(|&(t, id)| (t, id, rd.bytes())).filter(|m| m.0 == 1).map(|m| (m.1, m.2)).collect()
        };
        put_var(&mut brel_dat, ways.len() as u64);
        let mut pid = 0i64;
        let mut bb = EMPTY;
        for (id, role) in ways {
            put_zz(&mut brel_dat, id - pid);
            pid = id;
            put_bytes(&mut brel_dat, role);
            if let Ok(i) = w.bnd_ids.binary_search(&id) {
                let s0 = if i == 0 { 0 } else { w.bnd_end[i - 1] };
                for s in s0..w.bnd_end[i] {
                    let (la, lo) = need_ll[slots.slot_need[slots.bslot0 + s] as usize];
                    bbox_add(&mut bb, la, lo);
                }
            }
        }
        brel_idx.push(RelIdx { id: r.id, off, bbox: bb });
    }
    (brel_idx, brel_dat)
}

/// Tagged rail node records (index and tags), in block order.
fn tagged_node_records(nblks: &[NodeBlk]) -> (Vec<NodeIdx>, Vec<u8>) {
    let mut node_idx = Vec::new();
    let mut node_dat = Vec::new();
    for b in nblks {
        let mut s = 0;
        for (k, &(id, lat, lon)) in b.tagged.iter().enumerate() {
            node_idx.push(NodeIdx { id, off: node_dat.len() as u64, lat, lon });
            node_dat.extend_from_slice(&b.tags[s..b.tag_end[k]]);
            s = b.tag_end[k];
        }
    }
    (node_idx, node_dat)
}

/// Place node records (city and town, with their place tags), in block order.
fn place_records(nblks: &[NodeBlk]) -> (Vec<NodeIdx>, Vec<u8>) {
    let mut place_idx = Vec::new();
    let mut place_dat = Vec::new();
    for b in nblks {
        let mut s = 0;
        for (k, &(id, lat, lon)) in b.places.iter().enumerate() {
            place_idx.push(NodeIdx { id, off: place_dat.len() as u64, lat, lon });
            place_dat.extend_from_slice(&b.ptags[s..b.ptag_end[k]]);
            s = b.ptag_end[k];
        }
    }
    (place_idx, place_dat)
}

/// Read the planet in the four passes and write the world rail cache to `out`.
pub fn build(planet: &Path, out: &Path) {
    let t0 = Instant::now();
    let lap = |what: &str| eprintln!("[{:7.1}s] {what}", t0.elapsed().as_secs_f64());
    let file = File::open(planet).unwrap_or_else(|e| panic!("open {}: {e}", planet.display()));
    let f = &file;

    // ---- index the blobs (cached beside the output) and find where nodes, ways and relations start
    let (blobs, from_cache) = index(f, &out.with_extension("blobidx")).expect("PBF index");
    let sorted = blobs.first().is_some_and(|b| !b.data && header_sorted(f, b));
    assert!(sorted, "the input is not marked Sort.Type_then_ID (sort it with `osmium sort` first)");
    let data: Vec<BlobRef> = blobs.into_iter().filter(|b| b.data).collect();
    let n = data.len();
    let w0 = first_of(f, &data, K_WAYS);
    let r0 = first_of(f, &data, K_RELS);
    lap(&format!("indexed {n} blobs{} (nodes 0..{w0}, ways {w0}..{r0}, relations {r0}..{n})", if from_cache { " from the cached index" } else { "" }));

    // ---- pass 0, place nodes: whose boundary is wanted
    let city_wd = city_wikidata(f, &data[..w0]);
    lap(&format!("pass 0 (nodes, tags only): {} Wikidata ids of place=city nodes", city_wd.len()));

    // ---- pass 1, relations
    let all_rels = relations_pass(f, &data[r0..], &city_wd);
    let (bnd_rels, rels): (Vec<RelOut>, Vec<RelOut>) = all_rels.into_iter().partition(|r| r.bnd);
    let bnd_ways = member_way_ids(&bnd_rels);
    let mem_ways = member_way_ids(&rels);
    lap(&format!(
        "pass 1 (relations): {} kept, {} member ways; {} admin boundaries (levels 2, 4, 5 and the cities' 6 to 9), {} boundary ways",
        rels.len(),
        mem_ways.len(),
        bnd_rels.len(),
        bnd_ways.len()
    ));

    // ---- pass 2, ways
    let mut w = ways_pass(f, &data[w0..r0], &mem_ways, &bnd_ways);
    let nw = w.ids.len();
    let nr = w.refs.len();
    lap(&format!("pass 2 (ways): {nw} kept, {nr} node refs; {} boundary ways, {} node refs", w.bnd_ids.len(), w.bnd_refs.len()));

    let slots = node_slots(&w, &rels);
    lap(&format!("{} distinct nodes needed", slots.need.len()));

    // ---- pass 3, nodes: coordinates of the needed nodes, and the tagged rail nodes
    let nblks = nodes_pass(f, &data[..w0], &slots.need);
    let mut need_ll = vec![(MISSING, MISSING); slots.need.len()];
    let mut found = 0usize;
    for b in &nblks {
        for &(j, la, lo) in &b.found {
            need_ll[j as usize] = (la, lo);
        }
        found += b.found.len();
    }
    let ntagged: usize = nblks.iter().map(|b| b.tagged.len()).sum();
    let nplaces: usize = nblks.iter().map(|b| b.places.len()).sum();
    let nvillages: usize = nblks.iter().map(|b| b.villages).sum();
    lap(&format!(
        "pass 3 (nodes): {found} of {} needed nodes located, {ntagged} tagged rail nodes, {nplaces} place nodes (city, town; {nvillages} villages not kept)",
        slots.need.len()
    ));

    // ---- records
    let (way_idx, way_dat) = way_records(&w, &slots, &need_ll);
    (w.tags, w.refs, w.tag_end, w.ref_end) = (Vec::new(), Vec::new(), Vec::new(), Vec::new()); // the records hold them now
    let (rel_idx, rel_dat) = relation_records(&rels, nr, &slots, &need_ll, &way_idx);
    let (bway_idx, bway_dat, pts_before, pts_after) = boundary_way_records(&w, &slots, &need_ll);
    let (brel_idx, brel_dat) = boundary_relation_records(&bnd_rels, &w, &slots, &need_ll);
    lap(&format!(
        "admin boundaries: {} relations, {} ways, {} points simplified to {} ({:.0}%)",
        brel_idx.len(),
        bway_idx.len(),
        pts_before,
        pts_after,
        100.0 * pts_after as f64 / pts_before.max(1) as f64
    ));
    drop((w, slots, need_ll));
    let (node_idx, node_dat) = tagged_node_records(&nblks);
    let (place_idx, place_dat) = place_records(&nblks);
    lap("assembled way, relation and node records");

    cache::write(
        out,
        [
            as_bytes(&way_idx),
            &way_dat,
            as_bytes(&node_idx),
            &node_dat,
            as_bytes(&rel_idx),
            &rel_dat,
            as_bytes(&bway_idx),
            &bway_dat,
            as_bytes(&brel_idx),
            &brel_dat,
            as_bytes(&place_idx),
            &place_dat,
        ],
    )
    .unwrap_or_else(|e| panic!("write {}: {e}", out.display()));
    let size = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    lap(&format!(
        "wrote {} ({:.0} MB): {} ways, {} tagged nodes, {} relations",
        out.display(),
        size as f64 / 1e6,
        way_idx.len(),
        node_idx.len(),
        rel_idx.len()
    ));
}

/// Boundary simplification tolerance: about 20 m (0.0002 degrees of latitude). The old Nominatim outlines used 0.0003 for cities.
const SIMPLIFY_DEG: f64 = 0.0002;

/// Douglas-Peucker on one way, keeping both ends (so ways shared by two boundaries stay identical in both). Distances use
/// longitude scaled by cos(latitude), so the tolerance is about the same in metres everywhere.
fn simplify(p: &[(i32, i32)], tol_deg: f64) -> Vec<(i32, i32)> {
    if p.len() <= 2 {
        return p.to_vec();
    }
    let k = (p[0].0 as f64 * 1e-7).to_radians().cos();
    let xy = |q: (i32, i32)| (q.1 as f64 * 1e-7 * k, q.0 as f64 * 1e-7);
    let tol2 = tol_deg * tol_deg;
    let mut keep = vec![false; p.len()];
    keep[0] = true;
    keep[p.len() - 1] = true;
    let mut stack = vec![(0usize, p.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (ax, ay) = xy(p[a]);
        let (bx, by) = xy(p[b]);
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let (mut best, mut bi) = (-1.0, a);
        #[allow(clippy::needless_range_loop)] // `i` is also the result index
        for i in a + 1..b {
            let (px, py) = xy(p[i]);
            let d2 = if len2 == 0.0 {
                (px - ax).powi(2) + (py - ay).powi(2)
            } else {
                let t = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
                (px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)
            };
            if d2 > best {
                best = d2;
                bi = i;
            }
        }
        if best > tol2 {
            keep[bi] = true;
            stack.push((a, bi));
            stack.push((bi, b));
        }
    }
    p.iter().zip(keep).filter(|(_, k)| *k).map(|(q, _)| *q).collect()
}
