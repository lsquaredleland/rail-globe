//! Stretches: the maximal runs of ways that carry exactly the same set of trains. A stretch is what a station record points to, so its
//! id must not change when the input order does: it is the first 64 bits of the SHA-1 of the sorted way ids joined with ",".

use rustc_hash::FxHashMap;

pub struct Stretch {
    pub id: u64,
    /// way indexes on a routed path, ascending
    pub ways: Vec<u32>,
    /// the trains (indexes into the input) whose paths run over it
    pub trains: Vec<u32>,
}

/// SHA-1 of `data` (RFC 3174), the first eight bytes as a number
pub fn sha1_64(data: &[u8]) -> u64 {
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

/// stretches of `paths`, the ways (indexes into `way_ids`) of each routed leg in path order, `owner[l]` the train of leg `l` (the legs of
/// one train follow each other). Ordered by id; the build stops if two share one.
pub fn stretches(paths: &[Vec<u32>], owner: &[u32], way_ids: &[&str]) -> super::R<Vec<Stretch>> {
    let mut legs_of: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
    for (l, p) in paths.iter().enumerate() {
        for &w in p {
            let v = legs_of.entry(w).or_default();
            if v.last() != Some(&owner[l]) {
                v.push(owner[l]);
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
            Stretch { id, trains: legs_of[&ways[0]].clone(), ways }
        })
        .collect();
    out.sort_by_key(|s| (s.id, s.ways[0]));
    if let Some(w) = out.windows(2).find(|w| w[0].id == w[1].id) {
        return Err(format!(
            "two stretches share the id {:016x} (ways {} and {}): widen the hash",
            w[0].id, way_ids[w[0].ways[0] as usize], way_ids[w[1].ways[0] as usize]
        ));
    }
    Ok(out)
}
