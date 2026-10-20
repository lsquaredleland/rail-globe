//! World rail cache + region outline -> data/<slug>/{features.geojsonl, relations.jsonl, stop_positions.jsonl, qa.json,
//! airports.geojsonl, and the rail types (features.typed.geojsonl, lines.json, classify_report.json)}.
//!
//! Region membership follows osmium extract's "smart" strategy:
//! - a way is in the region when any of its nodes is inside the outline, and keeps its complete geometry;
//! - a relation is in when a member node is inside or a member way is in; then, repeatedly, every relation with a member
//!   relation that is in;
//! - a type=multipolygon relation that is in directly (not only through a member relation) brings all its member ways;
//! - a tagged node is in when it is inside, or is a node of a way that is in, or a node member of such a multipolygon.

use crate::assemble::{self, Fail, Poly, P};
use crate::classify;
use crate::cache::*;
use crate::out::*;
use crate::region::Region;
use crate::rules::{self, is, List, NodeKind, RelKind, Tags};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Instant;

/// what the relations side table gives the features
struct Ctx {
    way_routes: FxHashMap<i64, BTreeSet<String>>, // way id -> route relation ids ("relation/123", in string order)
    route_operator: FxHashMap<String, String>,
    /// way id -> the named infrastructure lines (route=railway) it is a member of: (way members of the line, name, English name); the smallest line names the track
    way_railway: FxHashMap<i64, Vec<(usize, String, Option<String>)>>,
    member_area: FxHashMap<(u8, i64), String>, // (b'n' | b'w' | b'r', id) -> stop_area id; a later stop area wins
}

struct Feat {
    line: Vec<u8>,
    label: String,
    stop_area: Option<String>,
    props_end: usize, // position in `line` of the "}" that closes the properties (classify adds fields there)
}

fn osm_type(k: u8) -> &'static str {
    match k {
        b'n' => "node",
        b'w' => "way",
        _ => "relation",
    }
}

fn txt(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// One GeoJSON feature line
fn feature(t: &List, k: u8, id: i64, geom: &[u8], label: String, ctx: &Ctx) -> Feat {
    let mut o = Vec::with_capacity(256 + geom.len());
    o.extend_from_slice(b"{\"type\":\"Feature\",\"properties\":{");
    let mut first = true;
    let mut key = |o: &mut Vec<u8>, k: &[u8]| {
        if !first {
            o.push(b',');
        }
        first = false;
        js(o, k);
        o.push(b':');
    };
    for kk in rules::KEEP {
        if let Some(v) = t.get(kk.as_bytes()) {
            key(&mut o, kk.as_bytes());
            js(&mut o, v);
        }
    }
    for (kk, v) in &t.0 {
        if kk.starts_with(b"name:") {
            key(&mut o, kk);
            js(&mut o, v);
        }
    }
    key(&mut o, b"osm_id");
    js(&mut o, format!("{}/{id}", osm_type(k)).as_bytes());
    if k == b'w' {
        // the name of the railway (infrastructure line) the way belongs to, for track with no name of its own: the smallest line that names it
        if let Some((_, name, en)) = ctx.way_railway.get(&id).and_then(|v| v.iter().min_by_key(|(n, name, _)| (*n, name.clone()))) {
            key(&mut o, b"_rwy");
            js(&mut o, name.as_bytes());
            if let Some(en) = en.as_ref().filter(|e| *e != name) {
                key(&mut o, b"_rwy_en");
                js(&mut o, en.as_bytes());
            }
        }
        if let Some(rs) = ctx.way_routes.get(&id).filter(|s| !s.is_empty()) {
            key(&mut o, b"_routes");
            o.push(b'[');
            let mut ops = BTreeSet::new();
            for (i, r) in rs.iter().enumerate() {
                if i > 0 {
                    o.push(b',');
                }
                js(&mut o, r.as_bytes());
                if let Some(op) = ctx.route_operator.get(r).filter(|s| !s.is_empty()) {
                    ops.insert(op.as_str());
                }
            }
            o.push(b']');
            if !ops.is_empty() {
                key(&mut o, b"_operators");
                o.push(b'[');
                for (i, op) in ops.iter().enumerate() {
                    if i > 0 {
                        o.push(b',');
                    }
                    js(&mut o, op.as_bytes());
                }
                o.push(b']');
            }
        }
    }
    let mut stop_area = None;
    if let Some(sa) = ctx.member_area.get(&(k, id)) {
        let rw = t.get(b"railway");
        let pt = t.get(b"public_transport");
        if matches!(rw, Some(b"station" | b"halt" | b"platform")) || matches!(pt, Some(b"station" | b"platform")) {
            key(&mut o, b"_stop_area");
            js(&mut o, sa.as_bytes());
            stop_area = Some(sa.clone());
        }
    }
    let props_end = o.len();
    o.extend_from_slice(b"},\"geometry\":");
    o.extend_from_slice(geom);
    o.extend_from_slice(b"}\n");
    Feat { line: o, label, stop_area, props_end }
}

/// Does a station node or area carry passengers? With a passenger list in the region's config (China: the 12306 ticketing list) the list
/// decides, by name; else tags do: `public_transport=station`, `train=yes` or a UIC code. A bare `railway=station` is often a freight yard
/// or a siding stop, and the OSM wiki says stations that handle freight only must not carry `public_transport=station`.
fn passenger_station(t: &List, cfg: Option<&classify::Cfg>) -> bool {
    let tags = is(t, b"public_transport", b"station") || is(t, b"train", b"yes") || t.get(b"uic_ref").is_some() || cfg.map_or(false, |c| c.bare_stations_count());
    match cfg {
        Some(c) => c.passenger_station(t.get(b"name").map(txt).as_deref(), tags),
        None => tags,
    }
}

const STRAY_M: f64 = 100.0; // an unconnected piece of track shorter than this is dropped

fn point_geom(lat: i32, lon: i32) -> Vec<u8> {
    let mut g = b"{\"type\":\"Point\",\"coordinates\":".to_vec();
    pt(&mut g, lat, lon);
    g.push(b'}');
    g
}

/// LineString without repeated points; None when a node has no location or fewer than 2 points remain
fn line_geom(ll: &[(i32, i32)]) -> Option<Vec<u8>> {
    let mut g = b"{\"type\":\"LineString\",\"coordinates\":[".to_vec();
    let mut n = 0;
    let mut prev = (0, 0);
    for &p in ll {
        if p.0 == MISSING {
            return None;
        }
        if n > 0 && p == prev {
            continue;
        }
        if n > 0 {
            g.push(b',');
        }
        pt(&mut g, p.0, p.1);
        prev = p;
        n += 1;
    }
    if n < 2 {
        return None;
    }
    g.extend_from_slice(b"]}");
    Some(g)
}

fn multipolygon_geom(polys: &[Poly], py: bool) -> Vec<u8> {
    let mut g = b"{\"type\":\"MultiPolygon\",\"coordinates\":[".to_vec();
    for (i, poly) in polys.iter().enumerate() {
        if i > 0 {
            g.push(b',');
        }
        g.push(b'[');
        for (j, ring) in poly.iter().enumerate() {
            if j > 0 {
                g.push(b',');
            }
            g.push(b'[');
            for (k, p) in ring.iter().enumerate() {
                if k > 0 {
                    g.push(b',');
                }
                if py {
                    g.push(b'[');
                    pynum(&mut g, p.1);
                    g.push(b',');
                    pynum(&mut g, p.0);
                    g.push(b']');
                } else {
                    pt(&mut g, p.0, p.1);
                }
            }
            g.push(b']');
        }
        g.push(b']');
    }
    g.extend_from_slice(b"]}");
    g
}

enum WayRes {
    Skip,
    Dropped,
    Feat(Feat, bool), // the feature, and whether it is track (a TRACK railway value, drawn as a LineString)
}

/// The feature of a way: track, siding, platform or area (or nothing)
fn way_feature(id: i64, w: &Way, ctx: &Ctx) -> WayRes {
    let t = List(w.tags.clone());
    if t.0.is_empty() || rules::has(&t, b"entrance") {
        return WayRes::Skip;
    }
    let rw = t.get(b"railway").unwrap_or(b"");
    if rw == b"platform_edge" {
        return WayRes::Skip;
    }
    if rules::TRACK.contains(&rw) {
        let svc = t.get(b"service").unwrap_or(b"");
        let Some(g) = line_geom(&w.ll) else { return WayRes::Dropped };
        let label = if rules::SERVICE_KEEP.contains(&svc) { format!("track:{}:{}", txt(rw), txt(svc)) } else { format!("track:{}", txt(rw)) };
        return WayRes::Feat(feature(&t, b'w', id, &g, label, ctx), true);
    }
    if matches!(rw, b"siding" | b"spur" | b"crossover") {
        let Some(g) = line_geom(&w.ll) else { return WayRes::Skip };
        return WayRes::Feat(feature(&t, b'w', id, &g, format!("track:{}", txt(rw)), ctx), false);
    }
    let Some(lab) = rules::classify_poly(&t) else { return WayRes::Skip };
    let closed = w.refs.len() >= 4 && w.refs.first() == w.refs.last();
    if closed && (lab != "area:platform" || is(&t, b"area", b"yes")) && !is(&t, b"area", b"no") {
        if w.ll.iter().any(|p| p.0 == MISSING) {
            return WayRes::Skip;
        }
        let mut g = b"{\"type\":\"Polygon\",\"coordinates\":[[".to_vec();
        for (i, p) in w.ll.iter().enumerate() {
            if i > 0 {
                g.push(b',');
            }
            pt(&mut g, p.0, p.1);
        }
        g.extend_from_slice(b"]]}");
        WayRes::Feat(feature(&t, b'w', id, &g, lab.to_string(), ctx), false)
    } else if lab == "area:platform" {
        let Some(g) = line_geom(&w.ll) else { return WayRes::Skip };
        WayRes::Feat(feature(&t, b'w', id, &g, "platform:line".into(), ctx), false)
    } else {
        WayRes::Skip
    }
}

/// Aerodromes with an IATA or ICAO code, terminals with a name, IATA code or ref
fn airport_feature(t: &List, kind_id: String, polys: &[Poly]) -> Option<Vec<u8>> {
    let truthy = |k: &[u8]| t.get(k).filter(|v| !v.is_empty());
    let kind = t.get(b"aeroway")?;
    match kind {
        b"aerodrome" if truthy(b"iata").is_none() && truthy(b"icao").is_none() => return None,
        b"terminal" if truthy(b"name").is_none() && truthy(b"iata").is_none() && truthy(b"ref").is_none() => return None,
        b"aerodrome" | b"terminal" => {}
        _ => return None,
    }
    let mut o = b"{\"type\":\"Feature\",\"properties\":{\"id\":".to_vec();
    js(&mut o, kind_id.as_bytes());
    o.extend_from_slice(b",\"k\":");
    js(&mut o, kind);
    for (ki, ko) in [("name", "name"), ("iata", "iata"), ("icao", "icao"), ("operator", "operator"), ("aerodrome:type", "atype"), ("military", "military")] {
        if let Some(v) = truthy(ki.as_bytes()) {
            o.push(b',');
            js(&mut o, ko.as_bytes());
            o.push(b':');
            js(&mut o, v);
        }
    }
    if let Some(en) = rules::english_name(t) {
        if Some(en) != t.get(b"name") {
            o.extend_from_slice(b",\"name_en\":");
            js(&mut o, en);
        }
    }
    o.extend_from_slice(b"},\"geometry\":");
    o.extend_from_slice(&multipolygon_geom(polys, true));
    o.extend_from_slice(b"}\n");
    Some(o)
}

pub struct Summary {
    pub line: String,
    /// exemptions that matched fewer features than their expect_min
    pub stale: Vec<String>,
}

/// one relation of the side table (relations.jsonl line plus what the later steps need)
struct Row {
    kind: RelKind,
    id: String,
    name: Option<String>,
    line: Vec<u8>,
    route_ways: Vec<i64>,        // routes: way members with role "" or "route"
    group_members: Vec<String>,  // stop_area_group: members as "type/id"
    tags: Vec<(String, String)>, // the side-table tags, in their order (classify reads them)
    rel_members: Vec<i64>,       // member relations (a route master's routes)
}

enum NodeRes {
    Feat(Feat, Option<(String, Option<String>, bool)>, Option<(i32, i32)>),
    Stop(Vec<u8>, Option<String>),
    Skip,
}

enum AreaRes {
    Feat(Feat),
    Bad,
    Skip,
}

/// everything the feature steps collect, in output order
#[derive(Default)]
struct Emit<'a> {
    counts: BTreeMap<String, i64>,
    used_areas: FxHashSet<String>,
    emitted_ways: FxHashSet<i64>,
    feats: Vec<u8>,
    stops: Vec<u8>,
    stations: Vec<(String, Option<String>, bool)>,
    pax_stations: Vec<(i32, i32)>, // (lat, lon) of passenger stations, nodes and areas
    airports: Vec<u8>,
    n_air: [usize; 2],
    tracks: Vec<classify::TrackIn<'a>>,
}

/// the relations kept in relations.jsonl, with what the qa report says about them
struct Kept<'a> {
    rows: Vec<&'a Row>,
    out_rel: Vec<u8>,
    kinds: Vec<(&'static str, i64)>,
    broken: Vec<(String, Option<String>, usize, usize)>, // (id, name, way members, missing)
}

/// Ways with a node inside the outline: state 1 (bbox first, then the points); 0 elsewhere.
fn ways_in_region(c: &Cache, reg: &Region) -> Vec<u8> {
    let wi = c.way_idx();
    let in_way: Vec<u32> = (0..wi.len() as u32)
        .into_par_iter()
        .with_min_len(1 << 12)
        .filter(|&i| {
            let w = &wi[i as usize];
            if !bbox_hits(&w.bbox, &reg.bbox) {
                return false;
            }
            let mut ll = Vec::new();
            c.way_ll(i as usize, &mut ll);
            ll.iter().any(|&(la, lo)| la != MISSING && reg.contains(la, lo))
        })
        .collect();
    let mut way_state = vec![0u8; wi.len()]; // 1 in the region, 2 brought in by a multipolygon
    for &i in &in_way {
        way_state[i as usize] = 1;
    }
    way_state
}

/// Relations by the smart-extract rule: candidates (by bbox) with their in/out flag, and the node ids a multipolygon brings.
/// Ways a direct multipolygon brings are set to state 2 in `way_state`.
fn relations_in_region<'c>(c: &'c Cache, reg: &Region, way_state: &mut [u8]) -> (Vec<(u32, Rel<'c>)>, Vec<bool>, FxHashSet<i64>) {
    let ri = c.rel_idx();
    let cand: Vec<u32> = (0..ri.len() as u32).into_par_iter().filter(|&i| bbox_hits(&ri[i as usize].bbox, &reg.bbox)).collect();
    let rels: Vec<(u32, Rel)> = cand.par_iter().map(|&i| (i, c.rel(i as usize))).collect();
    let mut direct: Vec<bool> = rels
        .par_iter()
        .map(|(_, r)| {
            r.mem.iter().any(|m| match m.t {
                0 => m.lat != MISSING && reg.contains(m.lat, m.lon),
                1 => c.find_way(m.id).map_or(false, |i| way_state[i] == 1),
                _ => false,
            })
        })
        .collect();
    let mut incl = direct.clone();
    let pos: FxHashMap<i64, usize> = rels.iter().enumerate().map(|(k, (i, _))| (ri[*i as usize].id, k)).collect();
    loop {
        let mut changed = false;
        for k in 0..rels.len() {
            if !incl[k] && rels[k].1.mem.iter().any(|m| m.t == 2 && pos.get(&m.id).map_or(false, |&q| incl[q])) {
                incl[k] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // a type=multipolygon relation that is in directly brings all its members
    let mut mp_nodes: FxHashSet<i64> = FxHashSet::default();
    for (k, (_, r)) in rels.iter().enumerate() {
        let t = List(r.tags.clone());
        direct[k] = direct[k] && is(&t, b"type", b"multipolygon");
        if !direct[k] {
            continue;
        }
        for m in &r.mem {
            match m.t {
                0 => {
                    mp_nodes.insert(m.id);
                }
                1 => {
                    if let Some(i) = c.find_way(m.id) {
                        if way_state[i] == 0 {
                            way_state[i] = 2;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    (rels, incl, mp_nodes)
}

/// Tagged nodes of the extract: inside the outline, a node of an extract way, or a node member of a multipolygon.
fn tagged_nodes(c: &Cache, reg: &Region, ways: &[Way], mp_nodes: &FxHashSet<i64>) -> Vec<u32> {
    let ni = c.node_idx();
    let refset: FxHashSet<i64> = ways.iter().flat_map(|w| w.refs.iter().copied()).collect();
    (0..ni.len() as u32)
        .into_par_iter()
        .filter(|&i| {
            let n = &ni[i as usize];
            reg.contains(n.lat, n.lon) || refset.contains(&n.id) || mp_nodes.contains(&n.id)
        })
        .collect()
}

/// The relations side table, in id order: the rows and the lookups the features use.
fn relation_rows(c: &Cache, rels: &[(u32, Rel)], incl: &[bool]) -> (Ctx, Vec<Row>) {
    let ri = c.rel_idx();
    let mut ctx = Ctx { way_routes: FxHashMap::default(), route_operator: FxHashMap::default(), way_railway: FxHashMap::default(), member_area: FxHashMap::default() };
    let mut rows: Vec<Row> = Vec::new();
    for (k, (i, r)) in rels.iter().enumerate() {
        if !incl[k] {
            continue;
        }
        let t = List(r.tags.clone());
        let Some(kind) = rules::rel_kind(&t) else { continue };
        let rid = format!("relation/{}", ri[*i as usize].id);
        let mut o = b"{\"id\":".to_vec();
        js(&mut o, rid.as_bytes());
        o.extend_from_slice(b",\"kind\":\"");
        o.extend_from_slice(kind.name().as_bytes());
        o.extend_from_slice(b"\",\"tags\":{");
        let mut first = true;
        let mut kv: Vec<(String, String)> = Vec::new();
        let mut tag = |o: &mut Vec<u8>, kk: &[u8], v: &[u8]| {
            kv.push((txt(kk), txt(v)));
            if !first {
                o.push(b',');
            }
            first = false;
            js(o, kk);
            o.push(b':');
            js(o, v);
        };
        for kk in rules::REL_TAGS {
            if let Some(v) = t.get(kk.as_bytes()) {
                tag(&mut o, kk.as_bytes(), v);
            }
        }
        for (kk, v) in &t.0 {
            if kk.starts_with(b"name:") {
                tag(&mut o, kk, v);
            }
        }
        o.extend_from_slice(b"},\"members\":[");
        let mut fm = true;
        for m in &r.mem {
            if kind == RelKind::RailwayLine && m.t == 1 {
                continue; // only stations and stops are needed, not the track
            }
            if !fm {
                o.push(b',');
            }
            fm = false;
            o.extend_from_slice(b"{\"type\":\"");
            o.extend_from_slice(osm_type([b'n', b'w', b'r'][m.t.min(2) as usize]).as_bytes());
            o.extend_from_slice(b"\",\"id\":");
            int(&mut o, m.id);
            o.extend_from_slice(b",\"role\":");
            js(&mut o, m.role);
            o.push(b'}');
        }
        o.extend_from_slice(b"]}\n");
        let mut route_ways = Vec::new();
        let mut group_members = Vec::new();
        match kind {
            RelKind::Route => {
                ctx.route_operator.insert(rid.clone(), t.get(b"operator").map(txt).unwrap_or_default());
                for m in &r.mem {
                    if m.t == 1 && (m.role.is_empty() || m.role == b"route") {
                        ctx.way_routes.entry(m.id).or_default().insert(rid.clone());
                        route_ways.push(m.id);
                    }
                }
            }
            RelKind::RailwayLine => {
                if let Some(name) = t.get(b"name").filter(|n| !n.is_empty()).map(txt) {
                    let n_ways = r.mem.iter().filter(|m| m.t == 1).count();
                    let en = crate::rules::english_name(&t).map(txt);
                    for m in r.mem.iter().filter(|m| m.t == 1) {
                        ctx.way_railway.entry(m.id).or_default().push((n_ways, name.clone(), en.clone()));
                    }
                }
            }
            RelKind::StopArea => {
                for m in &r.mem {
                    ctx.member_area.insert(([b'n', b'w', b'r'][m.t.min(2) as usize], m.id), rid.clone());
                }
            }
            RelKind::StopAreaGroup => {
                group_members = r.mem.iter().map(|m| format!("{}/{}", osm_type([b'n', b'w', b'r'][m.t.min(2) as usize]), m.id)).collect();
            }
            _ => {}
        }
        let rel_members = r.mem.iter().filter(|m| m.t == 2).map(|m| m.id).collect();
        rows.push(Row { kind, id: rid, name: t.get(b"name").map(txt), line: o, route_ways, group_members, tags: kv, rel_members });
    }
    (ctx, rows)
}

/// The feature, stop position or nothing for each tagged node (in parallel).
fn node_results(c: &Cache, nodes: &[u32], ctx: &Ctx, cfg: Option<&classify::Cfg>) -> Vec<NodeRes> {
    let ni = c.node_idx();
    nodes
        .par_iter()
        .map(|&i| {
            let n = &ni[i as usize];
            let t = List(c.node_tags(i as usize));
            match rules::node_kind(&t) {
                Some(NodeKind::Construction) => {
                    NodeRes::Feat(feature(&t, b'n', n.id, &point_geom(n.lat, n.lon), "station:construction".into(), ctx), None, None)
                }
                Some(NodeKind::Stop) => {
                    let mut o = b"{\"osm_id\":\"node/".to_vec();
                    int(&mut o, n.id);
                    o.extend_from_slice(b"\",\"lon\":");
                    num(&mut o, n.lon);
                    o.extend_from_slice(b",\"lat\":");
                    num(&mut o, n.lat);
                    o.extend_from_slice(b",\"name\":");
                    js_opt(&mut o, t.get(b"name"));
                    o.extend_from_slice(b",\"modes\":{");
                    let mut f = true;
                    for m in rules::MODE_FLAGS {
                        if let Some(v) = t.get(m) {
                            if !f {
                                o.push(b',');
                            }
                            f = false;
                            js(&mut o, m);
                            o.push(b':');
                            js(&mut o, v);
                        }
                    }
                    o.extend_from_slice(b"},\"ref\":");
                    js_opt(&mut o, t.get(b"ref"));
                    o.extend_from_slice(b"}\n");
                    NodeRes::Stop(o, ctx.member_area.get(&(b'n', n.id)).cloned())
                }
                Some(NodeKind::Station) => {
                    let label = format!("station:{}", t.get(b"railway").map(txt).unwrap_or_else(|| "pt".into()));
                    let f = feature(&t, b'n', n.id, &point_geom(n.lat, n.lon), label, ctx);
                    let has_area = ctx.member_area.contains_key(&(b'n', n.id))
                        && (is(&t, b"railway", b"station") || is(&t, b"railway", b"halt") || is(&t, b"public_transport", b"station"));
                    // a passenger station for the freight test of classify: not one tagged freight only
                    let pax = (is(&t, b"railway", b"station") || is(&t, b"railway", b"halt") || is(&t, b"public_transport", b"station"))
                        && !is(&t, b"railway:traffic_mode", b"freight") && !is(&t, b"station", b"freight") && passenger_station(&t, cfg);
                    NodeRes::Feat(f, Some((format!("node/{}", n.id), t.get(b"name").map(txt), has_area)), pax.then_some((n.lat, n.lon)))
                }
                None => NodeRes::Skip,
            }
        })
        .collect()
}

/// A track way that no other track touches at either end, is tiny (under STRAY_M) and is on no route: a stray piece.
fn is_stray(ll: &[(i32, i32)], track_ends: &FxHashMap<(i32, i32), u32>) -> bool {
    let alone = ll.len() >= 2 && track_ends.get(&ll[0]).copied() == Some(1) && track_ends.get(&ll[ll.len() - 1]).copied() == Some(1);
    alone
        && ll.iter().all(|p| p.0 != i32::MIN)
        && ll
            .windows(2)
            .map(|w| {
                let dy = (w[0].0 - w[1].0) as f64 * 1e-7 * 111_200.0;
                let dx = (w[0].1 - w[1].1) as f64 * 1e-7 * 111_200.0 * ((w[0].0 as f64 * 1e-7).to_radians()).cos();
                (dx * dx + dy * dy).sqrt()
            })
            .sum::<f64>()
            < STRAY_M
}

/// The way of the extract at cache index `idx` (`ext` is sorted).
fn way_at<'a, 'b>(ext: &[u32], ways: &'a [Way<'b>], idx: usize) -> Option<&'a Way<'b>> {
    ext.binary_search(&(idx as u32)).ok().map(|k| &ways[k])
}

/// The assembled polygons of a multipolygon or boundary: every member way must be in the extract.
fn member_ways<'a>(c: &Cache, ext: &[u32], ways: &'a [Way], r: &Rel) -> Option<Vec<&'a [P]>> {
    let mut v = Vec::new();
    for m in r.mem.iter().filter(|m| m.t == 1) {
        let idx = c.find_way(m.id)?;
        v.push(&way_at(ext, ways, idx)?.ll[..]);
    }
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// An airport or terminal as a counted bytes line: closed ways and relation areas both end here.
fn count_airport(e: &mut Emit, a: Vec<u8>) {
    e.n_air[a.windows(8).any(|x| x == b"\"k\":\"aer") as usize] += 1;
    e.airports.extend_from_slice(&a);
}

impl<'a> Emit<'a> {
    fn count(&mut self, label: String) {
        *self.counts.entry(label).or_default() += 1;
    }

    /// Add a feature line with its label count and stop area.
    fn add_feature(&mut self, f: Feat) {
        self.feats.extend_from_slice(&f.line);
        self.count(f.label);
        if let Some(a) = f.stop_area {
            self.used_areas.insert(a);
        }
    }

    /// Nodes: station features, stop positions.
    fn add_nodes(&mut self, node_res: Vec<NodeRes>) {
        for r in node_res {
            match r {
                NodeRes::Feat(f, st, pos) => {
                    self.pax_stations.extend(pos);
                    self.add_feature(f);
                    if let Some(s) = st {
                        self.stations.push(s);
                    }
                }
                NodeRes::Stop(line, area) => {
                    self.stops.extend_from_slice(&line);
                    self.count("stop_position".into());
                    if let Some(a) = area {
                        self.used_areas.insert(a);
                    }
                }
                NodeRes::Skip => {}
            }
        }
    }

    /// Ways: track pieces (kept for classify), platforms and station areas; strays and bad geometry are counted and dropped.
    fn add_ways(&mut self, c: &Cache, ext: &[u32], ways: &[Way<'a>], ctx: &Ctx, cfg: Option<&classify::Cfg>) {
        let wi = c.way_idx();
        let way_res: Vec<WayRes> = ext.par_iter().zip(ways.par_iter()).map(|(&i, w)| way_feature(wi[i as usize].id, w, ctx)).collect();
        // how many track ways end at each point: a way that no other track touches at either end, and is tiny, is a stray piece
        let mut track_ends: FxHashMap<(i32, i32), u32> = FxHashMap::default();
        for (k, r) in way_res.iter().enumerate() {
            if let WayRes::Feat(_, true) = r {
                for e in [ways[k].ll.first(), ways[k].ll.last()].into_iter().flatten() {
                    *track_ends.entry(*e).or_default() += 1;
                }
            }
        }
        for (k, r) in way_res.into_iter().enumerate() {
            match r {
                WayRes::Feat(_, true) if cfg.is_some() && ctx.way_routes.get(&wi[ext[k] as usize].id).map_or(true, |s| s.is_empty()) && is_stray(&ways[k].ll, &track_ends) => {
                    self.count("dropped:stray_track".into()); // under STRAY_M long, touching no other track, on no route
                }
                WayRes::Feat(f, is_track) => {
                    let id = wi[ext[k] as usize].id;
                    let routes: Vec<String> = ctx.way_routes.get(&id).map(|s| s.iter().cloned().collect()).unwrap_or_default();
                    let tl = List(ways[k].tags.clone());
                    if !is_track && (is(&tl, b"railway", b"station") || is(&tl, b"railway", b"halt") || is(&tl, b"public_transport", b"station")) && !is(&tl, b"railway:traffic_mode", b"freight") && passenger_station(&tl, cfg) {
                        // a station mapped as an area or a line: a point in its middle
                        if let Some(&q) = ways[k].ll.get(ways[k].ll.len() / 2) {
                            if q.0 != i32::MIN {
                                self.pax_stations.push(q);
                            }
                        }
                    }
                    if is_track && cfg.is_some() {
                        self.tracks.push(classify::TrackIn {
                            start: self.feats.len(),
                            end: self.feats.len() + f.line.len(),
                            pos: self.feats.len() + f.props_end,
                            props: classify::FeatProps { tags: tl, osm_id: format!("way/{id}") },
                            routes,
                            first: ways[k].ll[0],
                            ll: ways[k].ll.clone(),
                        });
                    }
                    self.add_feature(f);
                    self.emitted_ways.insert(id);
                }
                WayRes::Dropped => self.count("dropped:bad_geometry".into()),
                WayRes::Skip => {}
            }
        }
    }

    /// Relation areas (multipolygon or boundary, every member way in the extract), then airports and terminals.
    fn add_areas(&mut self, c: &Cache, rels: &[(u32, Rel)], incl: &[bool], ext: &[u32], ways: &[Way], ctx: &Ctx) {
        let ri = c.rel_idx();
        let wi = c.way_idx();
        let area_res: Vec<(AreaRes, Option<Vec<u8>>)> = rels
            .par_iter()
            .enumerate()
            .map(|(k, (i, r))| {
                let t = List(r.tags.clone());
                if !incl[k] || !rules::rel_is_area_type(&t) {
                    return (AreaRes::Skip, None);
                }
                let lab = rules::classify_poly(&t);
                let air = rules::aeroway_area(&t);
                if lab.is_none() && !air {
                    return (AreaRes::Skip, None);
                }
                let Some(mw) = member_ways(c, ext, ways, r) else { return (AreaRes::Skip, None) };
                let id = ri[*i as usize].id;
                match assemble::assemble(&mw, true) {
                    Ok(polys) => {
                        let f = lab.map(|l| feature(&t, b'r', id, &multipolygon_geom(&polys, false), l.to_string(), ctx));
                        let a = if air { airport_feature(&t, format!("relation/{id}"), &polys) } else { None };
                        (f.map_or(AreaRes::Skip, AreaRes::Feat), a)
                    }
                    Err(Fail::Geometry) if lab.is_some() => (AreaRes::Bad, None),
                    Err(_) => (AreaRes::Skip, None),
                }
            })
            .collect();

        // aerodromes and terminals mapped as closed ways (libosmium: more than 3 nodes, ends at the same location, not area=no)
        let way_air: Vec<Option<Vec<u8>>> = ext
            .par_iter()
            .zip(ways.par_iter())
            .map(|(&i, w)| {
                let t = List(w.tags.clone());
                if !rules::aeroway_area(&t) || w.refs.len() <= 3 {
                    return None;
                }
                let (a, b) = (w.ll[0], w.ll[w.ll.len() - 1]);
                if a.0 == MISSING || b.0 == MISSING || a != b || is(&t, b"area", b"no") {
                    return None;
                }
                let polys = assemble::assemble(&[&w.ll[..]], true).ok()?;
                airport_feature(&t, format!("way/{}", wi[i as usize].id), &polys)
            })
            .collect();
        for a in way_air.into_iter().flatten() {
            count_airport(self, a);
        }
        for (r, a) in area_res {
            match r {
                AreaRes::Feat(f) => self.add_feature(f),
                AreaRes::Bad => self.count("dropped:bad_multipolygon".into()),
                AreaRes::Skip => {}
            }
            if let Some(a) = a {
                count_airport(self, a);
            }
        }
    }
}

/// Keep stop areas that a kept feature or stop position belongs to, and groups holding a kept stop area.
fn keep_relations<'a>(rows: &'a [Row], e: &Emit) -> Kept<'a> {
    let mut out_rel: Vec<u8> = Vec::new();
    let mut kinds: Vec<(&'static str, i64)> = Vec::new();
    let bump = |kinds: &mut Vec<(&'static str, i64)>, k: &'static str| match kinds.iter_mut().find(|x| x.0 == k) {
        Some(x) => x.1 += 1,
        None => kinds.push((k, 1)),
    };
    let mut kept: Vec<&Row> = rows
        .iter()
        .filter(|r| matches!(r.kind, RelKind::Route | RelKind::RouteMaster | RelKind::RailwayLine) || e.used_areas.contains(&r.id))
        .collect();
    kept.extend(rows.iter().filter(|r| r.kind == RelKind::StopAreaGroup && r.group_members.iter().any(|m| e.used_areas.contains(m))));
    let mut broken: Vec<(String, Option<String>, usize, usize)> = Vec::new();
    for r in &kept {
        out_rel.extend_from_slice(&r.line);
        bump(&mut kinds, r.kind.name());
        if r.kind == RelKind::Route {
            let miss = r.route_ways.iter().filter(|w| !e.emitted_ways.contains(w)).count();
            if r.route_ways.is_empty() || miss > 0 {
                broken.push((r.id.clone(), r.name.clone(), r.route_ways.len(), miss));
            }
        }
    }
    broken.sort_by_key(|b| std::cmp::Reverse(b.3));
    Kept { rows: kept, out_rel, kinds, broken }
}

/// qa.json: feature and relation counts, broken routes, stations without a stop area or a name.
fn qa_json(slug: &str, e: &Emit, k: &Kept, n_routes: i64) -> Vec<u8> {
    let orphans: Vec<&(String, Option<String>, bool)> = e.stations.iter().filter(|s| !s.2).collect();
    let qa = J::obj(vec![
        ("region", J::s(slug.to_string())),
        ("feature_counts", J::Obj(e.counts.iter().map(|(k, v)| (k.clone(), J::Int(*v))).collect())),
        ("relation_counts", J::Obj(k.kinds.iter().map(|(k, v)| (k.to_string(), J::Int(*v))).collect())),
        (
            "broken_routes",
            J::obj(vec![
                ("count", J::Int(k.broken.len() as i64)),
                ("of", J::Int(n_routes)),
                (
                    "worst",
                    J::Arr(
                        k.broken
                            .iter()
                            .take(25)
                            .map(|b| {
                                J::obj(vec![
                                    ("id", J::s(b.0.clone())),
                                    ("name", J::opt(b.1.clone())),
                                    ("ways", J::Int(b.2 as i64)),
                                    ("missing", J::Int(b.3 as i64)),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ]),
        ),
        ("stations_total", J::Int(e.stations.len() as i64)),
        ("stations_without_stop_area", J::Int(orphans.len() as i64)),
        (
            "orphan_station_sample",
            J::Arr(orphans.iter().take(25).map(|o| J::obj(vec![("osm_id", J::s(o.0.clone())), ("name", J::opt(o.1.clone()))])).collect()),
        ),
        ("stations_without_name", J::Int(e.stations.iter().filter(|s| s.1.as_deref().map_or(true, str::is_empty)).count() as i64)),
    ]);
    let mut qa_bytes = Vec::new();
    qa.write_pretty(&mut qa_bytes, 0);
    qa_bytes
}

/// Rail types: features.typed.geojsonl, lines.json, classify_report.json. Returns the summary note and the stale exemptions.
fn classify_step(reg: &Region, cfg: Option<&classify::Cfg>, kept: &Kept, e: &Emit, wr: &dyn Fn(&str, &[u8])) -> (String, Vec<String>) {
    let t_class = Instant::now();
    let Some(cfg) = cfg else { return ("; not classified (no pipeline/config/<slug>.json)".to_string(), Vec::new()) };
    let rel_rows: Vec<classify::RelRow> = kept
        .rows
        .iter()
        .map(|r| classify::RelRow {
            id: r.id.clone(),
            route: r.kind == RelKind::Route,
            master: r.kind == RelKind::RouteMaster,
            tags: classify::Owned(r.tags.clone()),
            rel_members: r.rel_members.clone(),
        })
        .collect();
    let out = classify::classify(&reg.slug, cfg, &rel_rows, &e.feats, &e.tracks, &e.pax_stations);
    wr("features.typed.geojsonl", &out.typed);
    wr("lines.json", &out.lines);
    wr("classify_report.json", &out.report);
    let stale = out.stale.iter().map(|w| format!("{}: {w}", reg.slug)).collect();
    (format!("; classified {} track pieces, {} lines in {:.2}s", e.tracks.len(), out.n_lines, t_class.elapsed().as_secs_f64()), stale)
}

/// Cut one region out of the cache and write its files (features, relations, stop positions, qa, airports, rail types).
pub fn cut(c: &Cache, reg: &Region, outdir: &Path, cfg: Option<&classify::Cfg>) -> Summary {
    let t0 = Instant::now();
    let wi = c.way_idx();

    let mut way_state = ways_in_region(c, reg);
    let (rels, incl, mp_nodes) = relations_in_region(c, reg, &mut way_state);

    // the ways of the extract, decoded once
    let ext: Vec<u32> = (0..wi.len() as u32).into_par_iter().filter(|&i| way_state[i as usize] != 0).collect();
    let ways: Vec<Way> = ext.par_iter().map(|&i| c.way(i as usize)).collect();
    let nodes = tagged_nodes(c, reg, &ways, &mp_nodes);
    let t_member = t0.elapsed().as_secs_f64();

    let (ctx, rows) = relation_rows(c, &rels, &incl);

    // features: nodes, then ways, then relation areas
    let mut e = Emit::default();
    e.add_nodes(node_results(c, &nodes, &ctx, cfg));
    e.add_ways(c, &ext, &ways, &ctx, cfg);
    e.add_areas(c, &rels, &incl, &ext, &ways, &ctx);

    let kept = keep_relations(&rows, &e);
    let n_routes = kept.kinds.iter().find(|k| k.0 == "route").map_or(0, |k| k.1);
    let qa_bytes = qa_json(&reg.slug, &e, &kept, n_routes);

    std::fs::create_dir_all(outdir).expect("create output folder");
    let wr = |name: &str, b: &[u8]| std::fs::write(outdir.join(name), b).unwrap_or_else(|e| panic!("write {name}: {e}"));
    wr("features.geojsonl", &e.feats);
    wr("stop_positions.jsonl", &e.stops);
    wr("relations.jsonl", &kept.out_rel);
    wr("qa.json", &qa_bytes);
    wr("airports.geojsonl", &e.airports);

    let (class_note, stale) = classify_step(reg, cfg, &kept, &e, &wr);

    let nf: i64 = e.counts.iter().filter(|(k, _)| !k.starts_with("dropped") && k.as_str() != "stop_position").map(|(_, v)| v).sum();
    Summary {
        line: format!(
            "{}: {} features, {} stop positions, {} relations ({} routes, {} broken), {} airports + {} terminals; \
             membership {:.2}s, total {:.2}s{}",
            reg.slug,
            nf,
            e.counts.get("stop_position").copied().unwrap_or(0),
            kept.rows.len(),
            n_routes,
            kept.broken.len(),
            e.n_air[1],
            e.n_air[0],
            t_member,
            t0.elapsed().as_secs_f64(),
            class_note
        ),
        stale,
    }
}
