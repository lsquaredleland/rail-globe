//! Steps 1 to 3b: stations from stop areas and station features, platforms given to stations, duplicates merged.

use super::features::{track_mode, Feats, It};
use super::names::{core_name, decorated, norm_name, operator_tokens};
use super::{groups, max_radius, point_index, Rels, Station};
use crate::geomx::{self, Index, UnionFind, G};
use geo::{Geometry, Point};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::{BTreeMap, BTreeSet};

const SECOND_DOT_M: f64 = 100.0; // same-mode stations this close with the same name are one station
const SECOND_DOT_NAMELESS_M: f64 = 60.0; // ... a nameless one joins the nearest named station this close
const KEY_MERGE_RADIUS: f64 = 1000.0; // stations sharing a wikidata or uic_ref merge only within this distance
const KEY_MERGE_RADIUS_NAMED: f64 = 3000.0; // ... or within this one when their normalised names are equal

/// what the second merge pass compares of a station
struct NameInfo {
    names: Vec<String>,     // own names of its features and stop areas
    deco: BTreeSet<String>, // operator initials, the place its official_name adds in front
    wikidata: BTreeSet<String>,
    plain: bool, // one or two features, no stop area of its own: a polygon or node that repeats a station's name with a decoration
}

/// the final stations and the lookups that were kept in step with the merges
pub(super) struct Grouped {
    pub st: Vec<Station>,
    pub area_to_station: FxHashMap<String, usize>,
    pub plat_station: Vec<(String, usize)>, // (platform id, station), in the order the platforms were given out
}

/// steps 1 to 3b; also gives each platform its modes
pub(super) fn group_stations(feats: &mut Feats, rels: &Rels) -> Grouped {
    let (mut st, by_area_ix) = stations_from_features(feats);
    let s_tree = point_index(&st);
    let mut plat_station = assign_platforms(feats, &mut st, &by_area_ix, &s_tree);
    let (stn, old_to_new) = merge_same_mode(feats, &mut st, &s_tree);
    let (st, old_to_new) = merge_second_dot(feats, rels, stn, old_to_new);
    let area_to_station = by_area_ix.into_iter().map(|(a, i)| (a, old_to_new[i])).collect();
    for x in plat_station.iter_mut() {
        x.1 = old_to_new[x.1];
    }
    Grouped { st, area_to_station, plat_station }
}

/// steps 1 + 2: stations from stop areas, else from station features; also the stop area -> station lookup
fn stations_from_features(feats: &Feats) -> (Vec<Station>, FxHashMap<String, usize>) {
    let station_feats = &feats.station_feats;
    let mut st: Vec<Station> = Vec::new();
    let mut by_area_ix: FxHashMap<String, usize> = FxHashMap::default();
    for (k, it) in station_feats.iter().enumerate() {
        let sa = it.p("_stop_area").filter(|v| !v.is_empty());
        let (modes, stated) = feats.feature_modes(it);
        if let Some(&i) = sa.and_then(|a| by_area_ix.get(a)) {
            st[i].feats.push(k);
            if stated && !st[i].stated {
                st[i].modes.clear(); // a guessed mode gives way to a stated one
            }
            if stated || !st[i].stated {
                st[i].modes.extend(modes);
            }
            st[i].stated |= stated;
            continue;
        }
        st.push(Station {
            feats: vec![k],
            modes,
            areas: sa.map(|a| BTreeSet::from([a.to_string()])).unwrap_or_default(),
            platforms: vec![],
            grouping: if sa.is_some() { "mapped" } else { "inferred" },
            pt: Point::new(0.0, 0.0),
            norm: String::new(),
            merged_from: 1,
            stated,
        });
        if let Some(a) = sa {
            by_area_ix.insert(a.to_string(), st.len() - 1);
        }
    }
    for s_ in st.iter_mut() {
        let pts: Vec<Point<f64>> = s_.feats.iter().filter_map(|&k| if let Geometry::Point(p) = station_feats[k].g { Some(p) } else { None }).collect();
        s_.pt = pts.first().copied().or_else(|| geomx::rep_point(&station_feats[s_.feats[0]].g)).unwrap_or(Point::new(0.0, 0.0));
    }
    (st, by_area_ix)
}

/// the modes of platform `k`: its own tags, else the nearest track
fn platform_modes(feats: &Feats, k: usize) -> BTreeSet<String> {
    let (platforms, tracks, track_tree) = (&feats.platforms, &feats.tracks, &feats.track_tree);
    let mut pm: BTreeSet<String> = ["subway", "light_rail", "train"].iter().filter(|m| platforms[k].p(m) == Some("yes")).map(|m| m.to_string()).collect();
    if pm.is_empty() {
        if let Some(m) = feats.nearest_track_mode(&platforms[k].g, 25.0) {
            pm.insert(m.to_string());
        }
        // a surface platform with a heavy-rail track beside it is not a metro platform because a metro tunnel runs under it
        // (Changsha South: line 2 and 4 under the high-speed station): the nearest surface heavy-rail track decides
        let below = platforms[k].p("tunnel").is_some_and(|v| v != "no") || platforms[k].p("layer").is_some_and(|v| v.starts_with('-'));
        if !below && pm.iter().all(|m| m == "subway" || m == "light_rail") {
            let g = &platforms[k].g;
            let hv = track_tree
                .near(g, 25.0)
                .into_iter()
                .filter(|&i| {
                    matches!(track_mode(tracks[i].p("type").unwrap_or("")), "train")
                        && tracks[i].p("tunnel").is_none_or(|v| v == "no")
                        && geomx::dist(&tracks[i].g, g) <= 25.0
                })
                .count();
            let under = track_tree
                .near(g, 25.0)
                .into_iter()
                .filter(|&i| matches!(track_mode(tracks[i].p("type").unwrap_or("")), "subway" | "light_rail") && geomx::dist(&tracks[i].g, g) <= 25.0)
                .all(|i| tracks[i].p("tunnel").is_some_and(|v| v != "no"));
            if hv > 0 && under {
                pm = BTreeSet::from(["train".to_string()]);
            }
        }
    }
    pm
}

/// platforms: mapped via stop_area first; the rest go to the station whose footprint (point + mapped platforms) is nearest and
/// within its radius, else the station whose railway land holds them
fn assign_platforms(feats: &mut Feats, st: &mut [Station], by_area_ix: &FxHashMap<String, usize>, s_tree: &Index) -> Vec<(String, usize)> {
    let mut plat_station: Vec<(String, usize)> = Vec::new();
    let mut given: FxHashSet<String> = FxHashSet::default();
    for k in 0..feats.platforms.len() {
        feats.platforms[k].modes = platform_modes(feats, k);
        let platforms = &feats.platforms;
        if let Some(&i) = platforms[k].p("_stop_area").and_then(|a| by_area_ix.get(a)) {
            st[i].platforms.push(k);
            given.insert(platforms[k].id().to_string());
            plat_station.push((platforms[k].id().to_string(), i));
        }
    }
    let (platforms, lands) = (&feats.platforms, &feats.lands);
    // a station whose mode is only a guess (nearest track, often the tram in front of Basel SBB) takes the modes of its own mapped
    // platforms: a main-line station with train platforms is a train station, not a tram stop
    for s_ in st.iter_mut().filter(|s_| !s_.stated) {
        let pm: BTreeSet<String> =
            s_.platforms.iter().flat_map(|&k| platforms[k].modes.iter().cloned()).filter(|m| matches!(m.as_str(), "train" | "subway" | "light_rail")).collect();
        if !pm.is_empty() {
            s_.modes = pm;
        }
    }
    let mut anchors: Vec<G> = Vec::new();
    let mut anchor_owner: Vec<usize> = Vec::new();
    for (i, s_) in st.iter().enumerate() {
        anchors.push(Geometry::Point(s_.pt));
        anchor_owner.push(i);
        for &x in &s_.platforms {
            anchors.push(platforms[x].g.clone());
            anchor_owner.push(i);
        }
    }
    let anchor_tree = Index::new(anchors.iter().map(geomx::bbox));
    #[allow(clippy::needless_range_loop)] // `k` is also stored in the result
    for k in 0..platforms.len() {
        if given.contains(platforms[k].id()) {
            continue;
        }
        let pm = &platforms[k].modes;
        let Some(c) = geomx::rep_point(&platforms[k].g) else { continue };
        let cg = Geometry::Point(c);
        let mut best: Option<(usize, f64)> = None;
        for ai in anchor_tree.near(&cg, 300.0) {
            let i = anchor_owner[ai];
            if !pm.is_empty() && pm.is_disjoint(&st[i].modes) {
                continue;
            }
            let d = geomx::dist(&anchors[ai], &cg);
            if d <= max_radius(&st[i].modes) * if pm.is_empty() { 0.5 } else { 1.0 } && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        let mut best = best.map(|b| b.0);
        if best.is_none() {
            for li in feats.land_tree.at(&c) {
                if geomx::contains_pt(&lands[li].g, &c) {
                    let near: Vec<usize> = s_tree
                        .near(&cg, 200.0)
                        .into_iter()
                        .filter(|&i| geomx::contains_pt(&lands[li].g, &st[i].pt) && (pm.is_empty() || !pm.is_disjoint(&st[i].modes)))
                        .collect();
                    if !near.is_empty() {
                        best = near
                            .into_iter()
                            .map(|i| (i, geomx::dist_pt(&cg, &st[i].pt)))
                            .fold(None, |a: Option<(usize, f64)>, (i, d)| if a.is_none_or(|(_, bd)| d < bd) { Some((i, d)) } else { a })
                            .map(|x| x.0);
                        break;
                    }
                }
            }
        }
        if let Some(b) = best {
            st[b].platforms.push(k);
            given.insert(platforms[k].id().to_string());
            plat_station.push((platforms[k].id().to_string(), b));
        }
    }
    plat_station
}

/// a group of stations becomes one: the member with the most platforms gives the point and the name (the first such, as Python's max)
fn combine(platforms: &[It], ms: &[&Station]) -> Station {
    let all_mapped = ms.iter().all(|m| m.grouping == "mapped");
    let any_mapped = ms.iter().any(|m| m.grouping == "mapped");
    let grouping = if all_mapped && ms.len() == 1 {
        "mapped"
    } else if ms.len() > 1 && !any_mapped {
        "inferred"
    } else {
        "mapped"
    };
    let mut seen = FxHashSet::default();
    let plats: Vec<usize> = ms.iter().flat_map(|m| m.platforms.iter().copied()).filter(|&x| seen.insert(platforms[x].id().to_string())).collect();
    let lead = ms.iter().fold(None, |a: Option<&&Station>, m| if a.is_none_or(|b| m.platforms.len() > b.platforms.len()) { Some(m) } else { a }).unwrap();
    Station {
        feats: ms.iter().flat_map(|m| m.feats.iter().copied()).collect(),
        modes: ms.iter().flat_map(|m| m.modes.iter().cloned()).collect(),
        areas: ms.iter().flat_map(|m| m.areas.iter().cloned()).collect(),
        platforms: plats,
        grouping,
        pt: lead.pt,
        norm: lead.norm.clone(),
        merged_from: ms.iter().map(|m| m.merged_from).sum(),
        stated: ms.iter().any(|m| m.stated),
    }
}

/// build the merged stations of a union-find's groups, and the old index -> new index map
fn combine_groups(platforms: &[It], st: &[Station], uf: &mut UnionFind) -> (Vec<Station>, Vec<usize>) {
    let mut out: Vec<Station> = Vec::new();
    let mut new_of = vec![0usize; st.len()];
    for members in groups(uf, st.len()) {
        let ms: Vec<&Station> = members.iter().map(|&i| &st[i]).collect();
        for &i in &members {
            new_of[i] = out.len();
        }
        out.push(combine(platforms, &ms));
    }
    (out, new_of)
}

/// step 3: merge duplicates of the same mode (a shared wikidata or uic_ref, the same name close by, a polygon beside its own node)
fn merge_same_mode(feats: &Feats, st: &mut [Station], s_tree: &Index) -> (Vec<Station>, Vec<usize>) {
    let station_feats = &feats.station_feats;
    let mut uf = UnionFind::new(st.len());
    let mut keys_of: Vec<BTreeSet<(String, String)>> = Vec::with_capacity(st.len());
    for s_ in st.iter_mut() {
        let name = s_.feats.iter().find_map(|&k| station_feats[k].p("name").filter(|n| !n.is_empty())).unwrap_or("");
        s_.norm = norm_name(name);
        let mut ks = BTreeSet::new();
        for &k in &s_.feats {
            // not railway:ref: it is a per-network abbreviation ("Ch" is Chociszew, Ciechanow, Chybie...), not an identifier
            for key in ["wikidata", "uic_ref"] {
                if let Some(v) = station_feats[k].p(key).filter(|v| !v.is_empty()) {
                    ks.insert((key.to_string(), v.to_string()));
                }
            }
        }
        keys_of.push(ks);
    }
    let mut key_index: BTreeMap<&(String, String), Vec<usize>> = BTreeMap::new();
    for (i, ks) in keys_of.iter().enumerate() {
        for k in ks {
            key_index.entry(k).or_default().push(i);
        }
    }
    // a shared id only merges stations that are also close: ids are copied between far-apart objects (uic_ref in Russia, wikidata in
    // Poland), and an unlimited union fused up to 600 stations into one
    for idxs in key_index.values() {
        for (n, &a) in idxs.iter().enumerate() {
            for &b in &idxs[n + 1..] {
                if st[a].modes.is_disjoint(&st[b].modes) {
                    continue;
                }
                let d = geomx::dist_pt(&Geometry::Point(st[a].pt), &st[b].pt);
                if d <= KEY_MERGE_RADIUS || (d <= KEY_MERGE_RADIUS_NAMED && !st[a].norm.is_empty() && st[a].norm == st[b].norm) {
                    uf.union(a, b);
                }
            }
        }
    }
    for i in 0..st.len() {
        if st[i].norm.is_empty() {
            continue;
        }
        let r = max_radius(&st[i].modes);
        for j in s_tree.near(&Geometry::Point(st[i].pt), r) {
            if j <= i || st[j].norm != st[i].norm || st[j].modes.is_disjoint(&st[i].modes) {
                continue;
            }
            if geomx::dist_pt(&Geometry::Point(st[i].pt), &st[j].pt) <= r {
                uf.union(i, j);
            }
        }
    }
    // a station area with no platforms, stop area or second feature (a building also tagged railway=station: 三次駅 beside the node 三次,
    // New York "Pennsylvania Station") joins the nearest same-mode station within 100 m of it, preferring one with platforms, when the
    // names agree once normalised or the area has no name; else (named differently) the one station with platforms inside the area
    for i in 0..st.len() {
        if !st[i].areas.is_empty() || st[i].feats.len() != 1 {
            continue;
        }
        let g = &station_feats[st[i].feats[0]].g;
        if !matches!(g, Geometry::Polygon(_) | Geometry::MultiPolygon(_)) {
            continue;
        }
        let mut cands: Vec<(bool, f64, usize)> = Vec::new();
        for j in s_tree.near(g, 100.0) {
            if j == i || st[j].modes.is_disjoint(&st[i].modes) {
                continue;
            }
            let d = geomx::dist(g, &Geometry::Point(st[j].pt));
            if d <= 100.0 {
                cands.push((st[j].platforms.is_empty(), d, j));
            }
        }
        cands.sort_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).unwrap());
        let inside: Vec<usize> = cands.iter().filter(|c| !c.0 && geomx::contains_pt(g, &st[c.2].pt)).map(|c| c.2).collect();
        let pick = cands
            .iter()
            .find(|c| (st[i].norm.is_empty() && (st[i].platforms.is_empty() || st[c.2].platforms.is_empty())) || st[c.2].norm == st[i].norm)
            .map(|c| c.2)
            .or(if inside.len() == 1 && st[i].platforms.is_empty() { Some(inside[0]) } else { None });
        if let Some(j) = pick {
            uf.union(i, j);
        }
    }
    combine_groups(&feats.platforms, st, &mut uf)
}

/// what the second merge pass compares, for each station
fn name_infos(feats: &Feats, rels: &Rels, stn: &[Station]) -> Vec<NameInfo> {
    let station_feats = &feats.station_feats;
    stn.iter()
        .map(|x| {
            let mut names: Vec<String> = x.feats.iter().filter_map(|&k| station_feats[k].p("name").filter(|n| !n.is_empty()).map(String::from)).collect();
            let mut deco = BTreeSet::new();
            let mut wikidata = BTreeSet::new();
            for &k in &x.feats {
                for key in ["operator", "operator:short", "brand", "network"] {
                    if let Some(v) = station_feats[k].p(key) {
                        operator_tokens(v, &mut deco);
                    }
                }
                if let Some(w) = station_feats[k].p("wikidata").filter(|w| !w.is_empty()) {
                    wikidata.insert(w.to_string());
                }
            }
            for a in &x.areas {
                if let Some(n) = rels.area_name(a).filter(|n| !n.is_empty()) {
                    names.push(n.to_string());
                }
            }
            // "Berlin Jungfernheide" as official_name of "Jungfernheide": Berlin is a place, not part of the name
            for &k in &x.feats {
                let toks = |t: &str| -> Vec<String> { norm_name(t).split_whitespace().map(String::from).collect() };
                if let Some(of) = station_feats[k].p("official_name") {
                    let ot = toks(of);
                    for n in &names {
                        let nt = toks(n);
                        if !nt.is_empty() && ot.len() > nt.len() && ot.ends_with(&nt) {
                            deco.extend(ot[..ot.len() - nt.len()].iter().cloned());
                        }
                    }
                }
            }
            let plain = x.feats.len() <= 2 && x.areas.is_empty();
            NameInfo { names, deco, wikidata, plain }
        })
        .collect()
}

/// step 3b: the second dot of a station. Merged stations of one mode within SECOND_DOT_M that carry the same name once the
/// decoration is off (operator initials "Neckarsulm AVG", "Gilroy Transit Center", a place before a hyphen, a bilingual gloss, an
/// official_name's place), or a plain polygon/node that repeats a name with a Japanese operator in front or a Chinese station type
/// or exit behind ("阪急西院駅" beside "西院"), or a nameless one within SECOND_DOT_NAMELESS_M of a named one. Two stations whose
/// wikidata differ stay two when one name only carries a decoration or is missing (a different wikidata beside the same name is common: DB and S-Bahn Jungfernheide); names that merely sit close (Herald Square, PATH 33rd Street) are not the same name.
fn merge_second_dot(feats: &Feats, rels: &Rels, stn: Vec<Station>, old_to_new: Vec<usize>) -> (Vec<Station>, Vec<usize>) {
    let infos = name_infos(feats, rels, &stn);
    let tree = point_index(&stn);
    let mut uf2 = UnionFind::new(stn.len());
    let mut merged_any = false;
    for i in 0..stn.len() {
        let pi = Geometry::Point(stn[i].pt);
        let mut near_named: Option<(f64, usize)> = None; // for a nameless station: the nearest named one
        for j in tree.near(&pi, SECOND_DOT_M) {
            if j == i || stn[j].modes.is_disjoint(&stn[i].modes) {
                continue;
            }
            let d = geomx::dist_pt(&pi, &stn[j].pt);
            if d > SECOND_DOT_M {
                continue;
            }
            let (a, b) = (&infos[i], &infos[j]);
            let other_wd = !a.wikidata.is_empty() && !b.wikidata.is_empty() && a.wikidata.is_disjoint(&b.wikidata);
            if a.names.is_empty() && !b.names.is_empty() {
                if other_wd {
                    continue;
                }
                if d <= SECOND_DOT_NAMELESS_M && near_named.is_none_or(|(bd, _)| d < bd) {
                    near_named = Some((d, j));
                }
                continue;
            }
            if j < i || a.names.is_empty() {
                continue;
            }
            let deco: BTreeSet<String> = a.deco.union(&b.deco).cloned().collect();
            let (ca, cb): (Vec<String>, Vec<String>) =
                (a.names.iter().map(|n| core_name(n, &deco)).collect(), b.names.iter().map(|n| core_name(n, &deco)).collect());
            let same = ca.iter().any(|x| !x.is_empty() && cb.contains(x));
            let weak = |long: &NameInfo, short: &NameInfo| {
                !other_wd && long.plain && long.names.iter().any(|l| short.names.iter().any(|s| decorated(&norm_name(l), &norm_name(s))))
            };
            if same || weak(a, b) || weak(b, a) {
                uf2.union(i, j);
                merged_any = true;
            }
        }
        if let Some((_, j)) = near_named {
            uf2.union(i, j);
            merged_any = true;
        }
    }
    if !merged_any {
        return (stn, old_to_new);
    }
    let (out, new_of) = combine_groups(&feats.platforms, &stn, &mut uf2);
    (out, old_to_new.iter().map(|&k| new_of[k]).collect())
}
