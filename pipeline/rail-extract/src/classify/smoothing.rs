//! Neighbour passes: short pieces and chains of track take the type of the track they join.

use super::*;

pub(super) const MAX_CHAIN_KM: f64 = 40.0; // a chain of such pieces (a loop, a bypass, an old alignment) longer than this is a line of its own
pub(super) const SNIPPET_KM: f64 = 2.0; // a piece of track shorter than this that differs from all its neighbours takes their type

pub(super) fn ll_km(ll: &[(i32, i32)]) -> f64 {
    let pts: Vec<(f64, f64)> = ll.iter().filter(|p| p.0 != i32::MIN).map(|p| (p.1 as f64 / 1e7, p.0 as f64 / 1e7)).collect();
    crate::tiles::haversine_len(&pts)
}

/// Types the neighbour passes may change.
pub(super) fn smoothable(d: &Decided) -> bool {
    matches!(d.s, "tags" | "default" | "name" | "no-station" | "short-spur" | "no-evidence") && matches!(d.t.as_str(), "intercity" | "regional" | "freight")
}

/// A short piece of track whose type differs from all the track it joins (sidings and yard track do not count), when those agree, takes their type: a few metres of
/// "Regional" in the middle of a freight line is a tagging gap, not a service.
pub(super) fn smooth_short_pieces(tracks: &[TrackIn], at_end: &FxHashMap<(i32, i32), Vec<usize>>, decided: &mut [Option<Decided>]) {
    for _ in 0..2 {
        let mut changes: Vec<(usize, String)> = Vec::new();
        for (i, tr) in tracks.iter().enumerate() {
            let Some(d) = decided[i].as_ref().filter(|d| smoothable(d)) else { continue };
            if tr.props.g("railway") != Some("rail") || tr.ll.len() < 2 || ll_km(&tr.ll) >= SNIPPET_KM {
                continue;
            }
            let mut around: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
            let mut both_ends = true;
            for e in [tr.ll.first().unwrap(), tr.ll.last().unwrap()] {
                let n: Vec<&str> = at_end
                    .get(e)
                    .into_iter()
                    .flatten()
                    .filter(|&&j| j != i)
                    .filter_map(|&j| decided[j].as_ref().filter(|x| x.t != "yard").map(|x| x.t.as_str()))
                    .collect(); // a siding or yard track at a junction is no vote
                both_ends &= !n.is_empty();
                around.extend(n);
            }
            if both_ends && around.len() == 1 {
                let u = *around.iter().next().unwrap();
                if u != d.t && matches!(u, "intercity" | "regional" | "freight" | "high_speed") {
                    changes.push((i, u.to_string()));
                }
            }
        }
        if changes.is_empty() {
            break;
        }
        for (i, u) in changes {
            if let Some(d) = decided[i].as_mut() {
                d.t = u;
                d.s = "neighbour";
            }
        }
    }
}

/// The same for a chain of Intercity pieces: unrouted track typed Intercity from its tags (`usage=main`) that meets the rest of the network
/// only through Regional track is part of that Regional line (a loop, a bypass or an old alignment beside a routed line that no route
/// relation lists). Pieces joined end to end, under MAX_CHAIN_KM in all, whose every contact with other track (sidings and yard track do
/// not count) is Regional, become Regional. Only this direction: a chain of Freight or Regional pieces touching passenger track is a branch.
pub(super) fn smooth_chains(tracks: &[TrackIn], at_end: &FxHashMap<(i32, i32), Vec<usize>>, decided: &mut [Option<Decided>]) {
    let elig: Vec<usize> = (0..tracks.len())
        .filter(|&i| tracks[i].props.g("railway") == Some("rail") && tracks[i].ll.len() >= 2 && decided[i].as_ref().is_some_and(smoothable))
        .collect();
    let mut uf = UnionFind::new(tracks.len());
    let is_elig: FxHashSet<usize> = elig.iter().copied().collect();
    for &i in &elig {
        for e in [tracks[i].ll.first().unwrap(), tracks[i].ll.last().unwrap()] {
            for &j in at_end.get(e).into_iter().flatten().filter(|&&j| j != i && is_elig.contains(&j)) {
                // only pieces of one type are joined: the chain is one line
                if decided[i].as_ref().map(|d| &d.t) == decided[j].as_ref().map(|d| &d.t) {
                    uf.link(i, j);
                }
            }
        }
    }
    let mut comps: FxHashMap<usize, Vec<usize>> = FxHashMap::default();
    for &i in &elig {
        let r = uf.find(i);
        comps.entry(r).or_default().push(i);
    }
    let mut changes: Vec<(usize, String)> = Vec::new();
    for (_, members) in comps {
        let km: f64 = members.iter().map(|&i| ll_km(&tracks[i].ll)).sum();
        let t = decided[members[0]].as_ref().unwrap().t.clone();
        if members.len() < 2 || km >= MAX_CHAIN_KM {
            continue; // a single piece is the pass above
        }
        let inside: FxHashSet<usize> = members.iter().copied().collect();
        let mut around: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        let mut attach = 0;
        for &i in &members {
            for e in [tracks[i].ll.first().unwrap(), tracks[i].ll.last().unwrap()] {
                let n: Vec<&str> = at_end
                    .get(e)
                    .into_iter()
                    .flatten()
                    .filter(|&&j| !inside.contains(&j))
                    .filter_map(|&j| decided[j].as_ref().filter(|x| x.t != "yard").map(|x| x.t.as_str()))
                    .collect();
                if !n.is_empty() {
                    attach += 1;
                    around.extend(n);
                }
            }
        }
        if attach >= 1 && around.len() == 1 {
            let u = *around.iter().next().unwrap();
            if t == "intercity" && u == "regional" {
                changes.extend(members.iter().map(|&i| (i, u.to_string())));
            }
        }
    }
    for (i, u) in changes {
        if let Some(d) = decided[i].as_mut() {
            d.t = u;
            d.s = "neighbour";
        }
    }
}
