//! The first type each track gets: overrides, route types, name and evidence, then its own tags.

use super::*;

pub(super) const PASSENGER_NAME_WORDS: [&str; 9] = ["联络线", "直通线", "环线", "高铁", "高速", "客专", "客运专线", "城际", "动车"];

pub(super) struct Decided {
    pub(super) t: String,
    pub(super) s: &'static str,
    pub(super) own: Option<String>,
}

/// The first type each track gets: overrides, tourism, service track, then route types, name and evidence, then its own tags.
/// None for what is not typed track (or proposed track outside the proposed region).
pub(super) fn decide_tracks(tracks: &[TrackIn], cfg: &Cfg, pax: &[(i32, i32)], route_lines: &RouteLines) -> Vec<Option<Decided>> {
    let by_name = freight_ways_by_name(tracks, cfg, route_lines);
    let cand = evidence_candidates(tracks, cfg, route_lines);
    let by_evidence = freight_by_evidence(tracks, &cand, pax);
    let mostly_freight = |name: Option<&str>| name.and_then(|n| by_name.get(n)).is_some_and(|&(f, n)| f >= 5 && f as f64 >= 0.9 * n as f64);
    let mut decided: Vec<Option<Decided>> = Vec::with_capacity(tracks.len());
    for (ti, tr) in tracks.iter().enumerate() {
        let p = &tr.props;
        let rw = p.g("railway").unwrap_or("");
        if !TRACK_RW.contains(&rw) {
            decided.push(None);
            continue;
        }
        if rw == "proposed" && !cfg.proposed.as_ref().is_some_and(|a| a.contains(tr.first.0, tr.first.1)) {
            decided.push(None);
            continue; // proposed track is only kept inside the region's `proposed_region` (Hong Kong for now)
        }
        let mut own: Option<String> = None;
        let (t, s): (String, &str) = if let Some((_, ty)) = cfg.track_type_overrides.iter().find(|(r, _)| r.matches(p)) {
            (ty.clone(), "override")
        } else if p.g("usage") == Some("tourism") {
            ("other".into(), "tags") // tourist and heritage track is always Other, whatever routes or service tags say
        } else if p.g("service").is_some_and(|v| SERVICE_TRACK.contains(&v)) {
            ("yard".into(), "service")
        } else {
            let (o, src) = own_tag_type(p, cfg);
            // A route's High-speed type is the service's (a TGV runs on conventional line too), so it only counts on track that is
            // high-speed itself (highspeed=yes, or maxspeed at the floor); other track of such a route takes its next best route type, else Intercity.
            let route_types: Vec<&str> = tr.routes.iter().filter_map(|r| route_lines.type_of(r)).collect();
            let own_hs = o == "high_speed";
            let best = highest(route_types.iter().copied().filter(|t| own_hs || *t != "high_speed").chain(own_hs.then_some("high_speed")))
                .or_else(|| route_types.contains(&"high_speed").then_some("intercity"))
                .map(str::to_string);
            own = Some(o.clone());
            match best {
                Some(b) => (b, "route"),
                None if matches!(rw, "rail" | "narrow_gauge") && matches!(o.as_str(), "intercity" | "regional") && mostly_freight(p.g("name")) => {
                    ("freight".into(), "name")
                }
                // narrow gauge with no station is as likely a heritage or park railway as a freight one: Other, not Freight
                None if by_evidence.contains_key(&ti) => ((if rw == "narrow_gauge" { "other" } else { "freight" }).into(), by_evidence[&ti]),
                None => (o, src),
            }
        };
        decided.push(Some(Decided { t, s, own }));
    }
    decided
}
