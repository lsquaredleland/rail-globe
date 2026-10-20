//! Route and line typing: the type of a route relation, high-speed share by track, lines.json.

use super::*;

/// type of one route relation: (type, source); first matching rule wins
pub(super) fn route_type(t: &Owned, cfg: &Cfg, track_hs: HsTrack) -> (String, &'static str) {
    for (r, ty) in &cfg.type_overrides {
        if r.matches(t) {
            return (ty.clone(), "override");
        }
    }
    if let Some(m) = t.g("route").and_then(route_mode_type) {
        return (m.into(), "mode");
    }
    let net = t.g("network").unwrap_or("");
    let svc = t.g("service");
    let hs_net = cfg.hs_networks.iter().any(|r| r.is_match(net));
    let national = svc.is_some_and(|s| s.split(';').any(|v| matches!(v.trim(), "national" | "international")));
    if let Some(s) = svc.and_then(service_type) {
        if s == "intercity" && national && hs_net {
            return ("high_speed".into(), "service+network"); // service=international;long_distance on a high-speed network
        }
        return (s.into(), "service");
    }
    if national {
        return ((if hs_net { "high_speed" } else { "intercity" }).into(), "service+network");
    }
    if hs_net {
        return ("high_speed".into(), "network");
    }
    if cfg.china {
        let r = t.g("ref").unwrap_or("");
        let mut ch = r.chars();
        if ch.next() == Some('C') && ch.next().is_some_and(|c| !c.is_whitespace() && speed_kmh(Some(&c.to_string())).is_some()) {
            return ("intercity".into(), "train_number");
        }
    }
    match track_hs {
        HsTrack::Majority => return ("high_speed".into(), "track"),
        HsTrack::LongPartial => return ("intercity".into(), "track"),
        HsTrack::No => {}
    }
    ("regional".into(), "default")
}

/// A route is High-speed "by track" (no service or network rule decided) only when at least this share of its length is high-speed track.
pub(super) const HS_TRACK_SHARE: f64 = 0.5;
/// A route with some high-speed track, but under HS_TRACK_SHARE of it, is Intercity when it is at least this long (a GWR or CrossCountry
/// service that runs part of its way on a fast line), else Regional (a commuter service that crosses a high-speed link).
pub(super) const LONG_ROUTE_KM: f64 = 150.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HsTrack {
    No,
    LongPartial,
    Majority,
}

/// Route id -> how much of its track is high-speed (routes with no high-speed track are left out).
pub(super) fn high_speed_routes(tracks: &[TrackIn], cfg: &Cfg) -> FxHashMap<String, HsTrack> {
    // route -> (km of track, km of high-speed track: highspeed=yes, or high-speed by its own tags); track under construction counts for neither
    let mut len: FxHashMap<&str, (f64, f64)> = FxHashMap::default();
    for tr in tracks.iter().filter(|tr| !tr.routes.is_empty() && !matches!(tr.props.g("railway"), Some("construction" | "proposed"))) {
        let km = ll_km(&tr.ll);
        let hs = tr.props.g("highspeed") == Some("yes") || own_tag_type(&tr.props, cfg).0 == "high_speed";
        for r in &tr.routes {
            let e = len.entry(r.as_str()).or_default();
            e.0 += km;
            if hs {
                e.1 += km;
            }
        }
    }
    len.into_iter()
        .filter(|(_, (_, hs))| *hs > 0.0)
        .map(|(r, (all, hs))| {
            (
                r.to_string(),
                if hs >= HS_TRACK_SHARE * all {
                    HsTrack::Majority
                } else if all >= LONG_ROUTE_KM {
                    HsTrack::LongPartial
                } else {
                    HsTrack::No
                },
            )
        })
        .collect()
}

/// The lines of the region: their lines.json text, ids in first-seen order, types, and the counts for the report.
pub(super) struct Lines<'a> {
    pub(super) ids: Vec<&'a str>,
    pub(super) out: Vec<u8>,
    pub(super) line_type: FxHashMap<&'a str, String>,
    pub(super) by_type: Vec<(String, i64)>,
    pub(super) by_type_src: Vec<(String, i64)>,
}

/// The type of the line a route is on (its route master, else the route itself).
pub(super) struct RouteLines<'a, 'b> {
    pub(super) route_pos: &'b FxHashMap<&'a str, usize>,
    pub(super) route_master: &'b FxHashMap<String, &'a str>,
    pub(super) line_type: &'b FxHashMap<&'a str, String>,
}

impl RouteLines<'_, '_> {
    /// None when `rid` is not a running route.
    pub(super) fn type_of(&self, rid: &str) -> Option<&str> {
        self.route_pos.get(rid)?;
        self.line_type.get(self.route_master.get(rid).copied().unwrap_or(rid)).map(String::as_str)
    }
}

pub(super) fn py_opt(o: &mut Vec<u8>, v: Option<&str>) {
    match v {
        Some(s) => py_str(o, s),
        None => o.extend_from_slice(b"null"),
    }
}

/// One lines.json record, written with ", " and ": " separators.
pub(super) fn write_line<'a>(o: &mut Vec<u8>, lid: &str, first: &Owned, tags: &Owned, t: &str, ts: &str, route_ids: impl Iterator<Item = &'a str>) {
    o.extend_from_slice(b"{\"id\": ");
    py_str(o, lid);
    o.extend_from_slice(b", \"name\": ");
    py_opt(o, or(tags.g("name"), first.g("name")));
    o.extend_from_slice(b", \"ref\": ");
    py_opt(o, or(tags.g("ref"), first.g("ref")));
    o.extend_from_slice(b", \"name_en\": ");
    py_opt(o, english(tags).or_else(|| english(first)));
    o.extend_from_slice(b", \"tags\": {");
    for (i, (k, v)) in tags.0.iter().enumerate() {
        if i > 0 {
            o.extend_from_slice(b", ");
        }
        py_str(o, k);
        o.extend_from_slice(b": ");
        py_str(o, v);
    }
    o.extend_from_slice(b"}, \"colour\": ");
    py_opt(o, or(tags.g("colour"), first.g("colour")));
    o.extend_from_slice(b", \"type\": ");
    py_str(o, t);
    o.extend_from_slice(b", \"type_source\": ");
    py_str(o, ts);
    o.extend_from_slice(b", \"operator\": ");
    py_opt(o, or(tags.g("operator"), first.g("operator")));
    o.extend_from_slice(b", \"operator_wd\": ");
    py_opt(o, or(or(tags.g("operator:wikidata"), first.g("operator:wikidata")), or(tags.g("brand:wikidata"), first.g("brand:wikidata"))));
    o.extend_from_slice(b", \"network\": ");
    py_opt(o, or(tags.g("network"), first.g("network")));
    o.extend_from_slice(b", \"from\": ");
    py_opt(o, first.g("from"));
    o.extend_from_slice(b", \"to\": ");
    py_opt(o, first.g("to"));
    o.extend_from_slice(b", \"routes\": [");
    for (i, id) in route_ids.enumerate() {
        if i > 0 {
            o.extend_from_slice(b", ");
        }
        py_str(o, id);
    }
    o.extend_from_slice(b"]}");
}

/// Lines: route_master where one exists, else the route; the master's type is the most prominent of its routes.
pub(super) fn build_lines<'a>(
    cfg: &Cfg,
    rows: &'a [RelRow],
    routes: &[&'a RelRow],
    route_master: &FxHashMap<String, &'a str>,
    rtype: &[(String, &'static str)],
    ex_stats: &mut [ExStats],
) -> Lines<'a> {
    let masters: FxHashMap<&str, &RelRow> = rows.iter().filter(|r| r.master).map(|r| (r.id.as_str(), r)).collect();
    let mut line_ids: Vec<&str> = Vec::new();
    let mut line_routes: FxHashMap<&str, Vec<usize>> = FxHashMap::default();
    for (i, r) in routes.iter().enumerate() {
        let lid = route_master.get(&r.id).copied().unwrap_or(r.id.as_str());
        line_routes.entry(lid).or_insert_with(|| {
            line_ids.push(lid);
            Vec::new()
        });
        line_routes.get_mut(lid).unwrap().push(i);
    }
    let mut out = b"[".to_vec();
    let mut line_type: FxHashMap<&str, String> = FxHashMap::default();
    let (mut by_type, mut by_type_src): (Vec<(String, i64)>, Vec<(String, i64)>) = (Vec::new(), Vec::new());
    for (n, lid) in line_ids.iter().enumerate() {
        let rids = &line_routes[lid];
        let first_route = routes[rids[0]];
        let src = masters.get(lid).copied().unwrap_or(first_route);
        let mut t = highest(rids.iter().map(|&i| rtype[i].0.as_str())).unwrap_or("").to_string();
        // a master-level override (e.g. BART) beats the member routes
        let merged = Merged(&first_route.tags, &src.tags);
        if let Some((_, ty)) = cfg.type_overrides.iter().find(|(r, _)| r.matches(&merged)) {
            t = ty.clone();
        }
        let mut ts = rtype[rids[0]].1;
        // exemptions beat every rule above (a later one beats an earlier one)
        for (xi, x) in cfg.exemptions.iter().enumerate() {
            if let Action::Type(ty) = &x.action {
                if x.was_source.is_none() && x.was_type.as_deref().is_none_or(|w| w == t) && x.rule.matches(&LineTags(&merged, lid)) {
                    t = ty.clone();
                    ts = "exemption";
                    ex_stats[xi].lines += 1;
                }
            }
        }
        bump(&mut by_type, &t);
        bump(&mut by_type_src, &format!("{t}/{ts}"));
        if n > 0 {
            out.extend_from_slice(b", ");
        }
        write_line(&mut out, lid, &first_route.tags, &src.tags, &t, ts, rids.iter().map(|&ri| routes[ri].id.as_str()));
        line_type.insert(lid, t);
    }
    out.push(b']');
    Lines { ids: line_ids, out, line_type, by_type, by_type_src }
}
