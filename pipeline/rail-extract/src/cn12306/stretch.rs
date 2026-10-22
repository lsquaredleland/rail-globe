//! Stretches: the maximal runs of ways that carry exactly the same set of routed legs. A stretch is what a station record points to,
//! so its id must not change when the input order does: it is the first 64 bits of the SHA-1 of the sorted way ids joined with ",".
//! Parallel ways (parallel.rs) join the stretch of the nearest way of the leg's own path.

use rustc_hash::FxHashMap;

pub struct Stretch {
    pub id: u64,
    /// way indexes on a routed path, ascending
    pub ways: Vec<u32>,
    /// way indexes of parallel ways attached to it, ascending
    pub parallel: Vec<u32>,
    /// the legs (indexes into the input) whose paths run over it
    pub legs: Vec<u32>,
}

/// SHA-1 of `data` (RFC 3174), the first eight bytes as a number
fn sha1_64(data: &[u8]) -> u64 {
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&(data.len() as u64 * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            (e, d, c, b, a) = (d, c, b.rotate_left(30), a, t);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    (h[0] as u64) << 32 | h[1] as u64
}

/// the id of a stretch made of these ways
pub fn stretch_id(ids: &mut [&str]) -> u64 {
    ids.sort_unstable();
    sha1_64(ids.join(",").as_bytes())
}

/// stretches of `paths`, the ways (indexes into `way_ids`) of each routed leg in path order. Ordered by id; the build stops if two share one.
pub fn stretches(paths: &[Vec<u32>], way_ids: &[&str]) -> Vec<Stretch> {
    let mut legs_of: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
    for (l, p) in paths.iter().enumerate() {
        for &w in p {
            let v = legs_of.entry(w).or_default();
            if v.last() != Some(&(l as u32)) {
                v.push(l as u32);
            }
        }
    }
    let mut parent: FxHashMap<u32, u32> = legs_of.keys().map(|&w| (w, w)).collect();
    fn find(parent: &mut FxHashMap<u32, u32>, mut x: u32) -> u32 {
        while parent[&x] != x {
            let p = parent[&x];
            let gp = parent[&p];
            parent.insert(x, gp);
            x = gp;
        }
        x
    }
    for p in paths {
        for pair in p.windows(2) {
            if pair[0] != pair[1] && legs_of[&pair[0]] == legs_of[&pair[1]] {
                let (ra, rb) = (find(&mut parent, pair[0]), find(&mut parent, pair[1]));
                parent.insert(ra, rb);
            }
        }
    }
    let mut groups: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
    for &w in legs_of.keys() {
        let r = find(&mut parent, w);
        groups.entry(r).or_default().push(w);
    }
    let mut out: Vec<Stretch> = groups
        .into_values()
        .map(|mut ways| {
            ways.sort_unstable();
            let id = stretch_id(&mut ways.iter().map(|&w| way_ids[w as usize]).collect::<Vec<_>>());
            Stretch { id, legs: legs_of[&ways[0]].clone(), ways, parallel: Vec::new() }
        })
        .collect();
    out.sort_by_key(|s| (s.id, s.ways[0]));
    if let Some(w) = out.windows(2).find(|w| w[0].id == w[1].id) {
        crate::fail(format!(
            "two stretches share the id {:016x} (ways {} and {}): widen the hash",
            w[0].id, way_ids[w[0].ways[0] as usize], way_ids[w[1].ways[0] as usize]
        ));
    }
    out
}

/// attach each parallel way to the stretch of the nearest way of the first leg (in input order) that has it beside its path
/// (`beside[l]`, ascending), nearest by the middle vertex `mid` of the ways. A way that is already in a stretch stays there.
/// Known and kept: a parallel way beside several legs goes with the first of them, not with each.
pub fn attach_parallel(st: &mut [Stretch], paths: &[Vec<u32>], beside: &[Vec<u32>], mid: impl Fn(u32) -> (f64, f64)) {
    let mut owner: FxHashMap<u32, usize> = FxHashMap::default();
    for (i, s) in st.iter().enumerate() {
        for &w in &s.ways {
            owner.insert(w, i);
        }
    }
    for (path, par) in paths.iter().zip(beside) {
        let mut own: Vec<u32> = path.clone();
        own.sort_unstable();
        own.dedup();
        if own.is_empty() {
            continue;
        }
        for &p in par {
            if owner.contains_key(&p) {
                continue;
            }
            let mp = mid(p);
            let d2 = |w: u32| {
                let m = mid(w);
                (m.0 - mp.0).powi(2) + (m.1 - mp.1).powi(2)
            };
            let nearest = own.iter().copied().fold(None::<(f64, u32)>, |b, w| if b.is_none_or(|b| d2(w) < b.0) { Some((d2(w), w)) } else { b }).unwrap().1;
            let i = owner[&nearest];
            owner.insert(p, i);
            st[i].parallel.push(p);
        }
    }
    for s in st {
        s.parallel.sort_unstable();
    }
}
