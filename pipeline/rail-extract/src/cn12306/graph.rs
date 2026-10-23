//! The China track as a graph: every `railway=rail` way of the world cut's `cn` slug, service track included (it costs 3x in the
//! router). Nodes are OSM nodes: two ways connect only where they share a node id (way_nodes.jsonl, written by `cut` beside the features);
//! a gap between ways that do not share a node is repaired by the router's stitches (rgraph.rs), not here. One edge per way segment; every way keeps its `osm_id`, so ids match the world tiles. Each way gets a speed for the minimum run time: its
//! `maxspeed` tag, else the median of the neighbouring ways of the same railway, else 250 km/h for a high-speed line and 120 for
//! the rest (40 for service track).

use rayon::prelude::*;
use rustc_hash::FxHashMap;
use serde::Deserialize;
use std::path::Path;

/// great-circle distance in km (the same formula as the Python prototype and `tiles::haversine_len`, for two points)
pub fn hav(lo1: f64, la1: f64, lo2: f64, la2: f64) -> f64 {
    let (p1, p2) = (la1.to_radians(), la2.to_radians());
    let (dp, dl) = (p2 - p1, (lo2 - lo1).to_radians());
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    12742.0 * a.sqrt().min(1.0).asin()
}

/// the search grid: cells of 0.02 degrees (about 2 km)
pub fn cell(lon: f64, lat: f64) -> (i32, i32) {
    ((lon * 50.0) as i32, (lat * 50.0) as i32)
}

/// one segment between two consecutive vertices of a way
pub struct Seg {
    pub a: u32,
    pub b: u32,
    pub way: u32,
    pub km: f64,
}

pub struct Way {
    pub id: String,
    pub service: bool,
    /// length of its segments in km
    pub km: f64,
    /// km/h used for the minimum run time
    pub speed: f64,
    /// where the speed came from: 0 the way's own maxspeed tag, 1 inferred from neighbours, 2 a default
    pub speed_src: u8,
    /// the middle vertex, where the way is filed in the way files
    pub mid: (f64, f64),
    /// `highspeed=yes`
    pub hsr: bool,
    /// the speed of an untagged way by its `usage` (the corridor router's default), km/h
    pub usage_kmh: f64,
    /// the index of the way's name in `Graph::lines`, `NO_LINE` for none
    pub line: u32,
}

/// a way without a name
pub const NO_LINE: u32 = u32::MAX;

pub struct Graph {
    pub xy: Vec<(f64, f64)>,
    pub segs: Vec<Seg>,
    adj_start: Vec<u32>,
    adj: Vec<u32>,
    pub ways: Vec<Way>,
    /// node ids by 0.02 degree cell (nodes with an edge only)
    pub grid: FxHashMap<(i32, i32), Vec<u32>>,
    /// segment ids by every 0.02 degree cell they cross, so a long straight segment is found from any cell beside it
    pub seg_grid: FxHashMap<(i32, i32), Vec<u32>>,
    /// connected component of every node (-1: no edge)
    pub comp: Vec<i32>,
    /// the names of the ways, in the order of their first way
    pub lines: Vec<String>,
    /// ways whose node list did not match their vertices
    pub ways_without_node_ids: usize,
    /// repaired duplicate nodes: the two way ids and the point
    pub dup_joins: Vec<(String, String, (f64, f64))>,
}

impl Graph {
    /// indexes of the segments touching node `u`, in the order they were read
    pub fn seg_ids_at(&self, u: u32) -> &[u32] {
        &self.adj[self.adj_start[u as usize] as usize..self.adj_start[u as usize + 1] as usize]
    }

    pub fn segs_at(&self, u: u32) -> impl Iterator<Item = &Seg> {
        self.seg_ids_at(u).iter().map(move |&s| &self.segs[s as usize])
    }

    pub fn n_nodes(&self) -> usize {
        self.xy.len()
    }
}

#[derive(Deserialize)]
struct RawFeature {
    properties: RawProps,
    geometry: RawGeom,
}

#[derive(Deserialize)]
struct RawProps {
    railway: Option<String>,
    osm_id: Option<String>,
    service: Option<String>,
    maxspeed: Option<String>,
    highspeed: Option<String>,
    usage: Option<String>,
    bridge: Option<String>,
    tunnel: Option<String>,
    layer: Option<String>,
    name: Option<String>,
    #[serde(rename = "name:zh")]
    name_zh: Option<String>,
    #[serde(rename = "_rwy")]
    rwy: Option<String>,
}

#[derive(Deserialize)]
struct RawGeom {
    coordinates: Vec<[f64; 2]>,
}

/// what the speed rules read from a way's tags
struct Tags {
    maxspeed: Option<f64>,
    high_speed: bool,
    /// name, else name:zh
    name: String,
    /// the railway the way belongs to (`_rwy`), else its name: ways of one railway share a speed
    key: String,
}

/// the highest number of a `maxspeed` tag in km/h: "120", "50;80", "30 mph"; None for "none", "signals" and anything over 400
pub fn maxspeed_kmh(s: &str) -> Option<f64> {
    let mut best: Option<f64> = None;
    for part in s.split(';') {
        let t = part.trim_start();
        let digits = t.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(t.len());
        let num = &t[..digits];
        // digits, optionally one point followed by digits ("100.", ".5" and "1.2.3" are not numbers)
        let (int, frac) = num.split_once('.').map_or((num, None), |(i, f)| (i, Some(f)));
        if int.is_empty() || frac.is_some_and(|f| f.is_empty() || f.contains('.')) {
            continue;
        }
        let Ok(mut v) = num.parse::<f64>() else { continue };
        if t[digits..].trim_start().starts_with("mph") {
            v *= 1.609;
        }
        if v > 0.0 && v <= 400.0 {
            best = Some(best.map_or(v, |b| b.max(v)));
        }
    }
    best
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// a way's speed from the rules in the header; `neigh[w]` lists the ways that share a node with `w` (only filled for ways that need an inferred speed)
fn speeds(tags: &[Tags], service: &[bool], neigh: &[Vec<u32>]) -> Vec<(f64, u8)> {
    let n = tags.len();
    let mut spd: Vec<Option<f64>> = tags.iter().map(|t| t.maxspeed).collect();
    let mut src: Vec<u8> = spd.iter().map(|s| if s.is_some() { 0 } else { 2 }).collect();
    for _ in 0..40 {
        // one round reads the speeds of the round before: all updates are applied after the scan
        let upd: Vec<(usize, f64)> = (0..n)
            .filter(|&w| spd[w].is_none() && !service[w] && !tags[w].key.is_empty())
            .filter_map(|w| {
                let mut vals: Vec<f64> =
                    neigh[w].iter().filter(|&&x| !service[x as usize] && tags[x as usize].key == tags[w].key).filter_map(|&x| spd[x as usize]).collect();
                (!vals.is_empty()).then(|| (w, median(&mut vals)))
            })
            .collect();
        if upd.is_empty() {
            break;
        }
        for (w, v) in upd {
            spd[w] = Some(v);
            src[w] = 1;
        }
    }
    (0..n)
        .map(|w| {
            let v = spd[w].unwrap_or_else(|| {
                let t = &tags[w];
                let nm = format!("{}{}", t.name, t.key);
                if service[w] {
                    40.0
                } else if t.high_speed || nm.contains("高速") || nm.contains("客专") || nm.contains("客运专线") {
                    250.0
                } else {
                    120.0
                }
            });
            (v, src[w])
        })
        .collect()
}

/// read <features> (a cut's features.geojsonl) and build the graph
pub fn load(features: &Path) -> super::R<Graph> {
    let text = super::io(std::fs::read_to_string(features), "read", features)?;
    let lines: Vec<&str> = text.lines().filter(|l| l.contains("\"railway\":\"rail\"") && l.contains("\"type\":\"LineString\"")).collect();
    let raws: Vec<RawFeature> = lines
        .par_iter()
        .map(|l| serde_json::from_str::<RawFeature>(l).map_err(|e| format!("{}: {e}: {}", features.display(), l.chars().take(200).collect::<String>())))
        .collect::<super::R<Vec<_>>>()?
        .into_iter()
        .filter(|f| f.properties.railway.as_deref() == Some("rail"))
        .collect();

    // the OSM node ids of every way (way_nodes.jsonl beside the features): two ways connect only where they share a node id
    let wn_path = features.with_file_name("way_nodes.jsonl");
    let wn_text =
        std::fs::read_to_string(&wn_path).map_err(|e| format!("read {}: {e} (written by `rail-extract cut`; cut the region again)", wn_path.display()))?;
    let way_nodes: FxHashMap<i64, Vec<i64>> = wn_text.par_lines().filter_map(crate::site::decode_way_nodes).collect();
    drop(wn_text);

    let mut xy: Vec<(f64, f64)> = Vec::new();
    let mut index: FxHashMap<i64, u32> = FxHashMap::default();
    let mut node = |id: i64, c: [f64; 2]| -> (u32, (f64, f64)) {
        let n = *index.entry(id).or_insert_with(|| {
            xy.push((c[0], c[1]));
            xy.len() as u32 - 1
        });
        (n, xy[n as usize])
    };
    let mut without_ids = 0usize;

    let mut ways: Vec<Way> = Vec::with_capacity(raws.len());
    let mut tags: Vec<Tags> = Vec::with_capacity(raws.len());
    let mut segs: Vec<Seg> = Vec::new();
    // every (node, way, the vertex is an end of the way) and the level of every way, for the repair of duplicate nodes below
    let mut inc: Vec<(u32, u32, bool)> = Vec::new();
    let mut level: Vec<u32> = Vec::new();
    let mut level_of: FxHashMap<(bool, bool, i32), u32> = FxHashMap::default();
    let mut lines: Vec<String> = Vec::new();
    let mut line_of: FxHashMap<String, u32> = FxHashMap::default();
    for f in raws.iter().filter(|f| !f.geometry.coordinates.is_empty()) {
        let w = ways.len();
        let p = &f.properties;
        let Some(osm_id) = p.osm_id.clone().filter(|s| !s.is_empty()) else {
            return Err(format!("{}: a railway=rail line has no osm_id", features.display()));
        };
        let service = p.service.as_deref().is_some_and(|s| !s.is_empty());
        let name = p.name.clone().filter(|s| !s.is_empty()).or_else(|| p.name_zh.clone()).unwrap_or_default();
        let rwy = p.rwy.clone().unwrap_or_default();
        let key = if rwy.is_empty() { name.clone() } else { rwy };
        tags.push(Tags { maxspeed: p.maxspeed.as_deref().and_then(maxspeed_kmh), high_speed: p.highspeed.as_deref() == Some("yes"), name, key });
        let cs = &f.geometry.coordinates;
        // a way whose node list does not match its vertices (a node the cut had no point for) cannot be aligned: its vertices get an id
        // from their place, so they join only at the same place
        let ids: Vec<i64> = match osm_id.strip_prefix("way/").and_then(|x| x.parse::<i64>().ok()).and_then(|id| way_nodes.get(&id)) {
            Some(v) if v.len() == cs.len() => v.clone(),
            Some(_) => {
                without_ids += 1;
                cs.iter().map(|c| -1 - (((c[0] * 1e5).round() as i64) << 32 | ((c[1] * 1e5).round() as i64 & 0xffff_ffff))).collect()
            }
            None => return Err(format!("{}: no node list for {osm_id}", wn_path.display())),
        };
        let lv = (
            p.bridge.as_deref().is_some_and(|b| !b.is_empty() && b != "no"),
            p.tunnel.as_deref().is_some_and(|b| !b.is_empty() && b != "no"),
            p.layer.as_deref().and_then(|l| l.trim().parse::<i32>().ok()).unwrap_or(0),
        );
        let next = level_of.len() as u32;
        level.push(*level_of.entry(lv).or_insert(next));
        let mut km = 0.0;
        let mut prev: Option<(u32, (f64, f64))> = None;
        for (vi, (&c, &id)) in cs.iter().zip(&ids).enumerate() {
            let (n, p) = node(id, c);
            inc.push((n, w as u32, vi == 0 || vi + 1 == cs.len()));
            if let Some((pv, a)) = prev.filter(|&(pv, _)| pv != n) {
                let d = hav(a.0, a.1, p.0, p.1);
                segs.push(Seg { a: pv, b: n, way: w as u32, km: d });
                km += d;
            }
            prev = Some((n, p));
        }
        let m = cs[cs.len() / 2];
        let usage_kmh = match p.usage.as_deref() {
            Some("main") => 120.0,
            Some("branch") => 80.0,
            Some("industrial" | "military" | "tourism" | "test") => 40.0,
            _ => 100.0,
        };
        let line = if tags[w].name.is_empty() {
            NO_LINE
        } else {
            *line_of.entry(tags[w].name.clone()).or_insert_with(|| {
                lines.push(tags[w].name.clone());
                (lines.len() - 1) as u32
            })
        };
        ways.push(Way { id: osm_id, service, km, speed: 0.0, speed_src: 0, mid: (m[0], m[1]), hsr: tags[w].high_speed, usage_kmh, line });
    }

    if segs.is_empty() {
        return Err(format!("{}: no railway=rail way with two distinct vertices: not a China cut?", features.display()));
    }

    // duplicate nodes: OSM mapping errors where one point is two nodes (bit-identical coordinates, different ids). They are joined when at
    // least one is the end of its way and both ways are on the same level (bridge, tunnel, layer; missing is ground, 0). A crossing in the
    // middle of two ways, or on different levels, stays unjoined.
    let mut dup_joins: Vec<(String, String, (f64, f64))> = Vec::new();
    {
        let mut at: FxHashMap<(u64, u64), Vec<u32>> = FxHashMap::default();
        for (i, p) in xy.iter().enumerate() {
            at.entry((p.0.to_bits(), p.1.to_bits())).or_default().push(i as u32);
        }
        let mut groups: Vec<Vec<u32>> = at.into_values().filter(|v| v.len() > 1).collect();
        groups.sort();
        inc.sort_unstable();
        let incident = |n: u32| &inc[inc.partition_point(|x| x.0 < n)..inc.partition_point(|x| x.0 <= n)];
        // union-find over the nodes, the smallest index the root
        let mut canon: Vec<u32> = (0..xy.len() as u32).collect();
        fn find(p: &mut [u32], mut x: u32) -> u32 {
            while p[x as usize] != x {
                p[x as usize] = p[p[x as usize] as usize];
                x = p[x as usize];
            }
            x
        }
        for grp in groups {
            for (i, &a) in grp.iter().enumerate() {
                for &b in &grp[i + 1..] {
                    let hit = incident(a)
                        .iter()
                        .flat_map(|x| incident(b).iter().map(move |y| (x, y)))
                        .find(|(x, y)| (x.2 || y.2) && level[x.1 as usize] == level[y.1 as usize]);
                    if let Some((x, y)) = hit {
                        let (ra, rb) = (find(&mut canon, a), find(&mut canon, b));
                        canon[ra.max(rb) as usize] = ra.min(rb);
                        dup_joins.push((ways[x.1 as usize].id.clone(), ways[y.1 as usize].id.clone(), xy[a as usize]));
                    }
                }
            }
        }
        if !dup_joins.is_empty() {
            for s in segs.iter_mut() {
                s.a = find(&mut canon, s.a);
                s.b = find(&mut canon, s.b);
            }
            segs.retain(|s| s.a != s.b);
        }
        dup_joins.sort_by(|p, q| p.2 .0.total_cmp(&q.2 .0).then(p.2 .1.total_cmp(&q.2 .1)).then(p.0.cmp(&q.0)).then(p.1.cmp(&q.1)));
    }

    // adjacency in compressed form: the segments of a node in the order they were read
    let n = xy.len();
    let mut adj_start = vec![0u32; n + 1];
    for s in &segs {
        adj_start[s.a as usize + 1] += 1;
        adj_start[s.b as usize + 1] += 1;
    }
    for i in 0..n {
        adj_start[i + 1] += adj_start[i];
    }
    let mut fill = adj_start.clone();
    let mut adj = vec![0u32; segs.len() * 2];
    for (i, s) in segs.iter().enumerate() {
        for u in [s.a, s.b] {
            adj[fill[u as usize] as usize] = i as u32;
            fill[u as usize] += 1;
        }
    }

    // the ways that share a node with a way that has no maxspeed tag
    let service: Vec<bool> = ways.iter().map(|w| w.service).collect();
    let mut way_segs: Vec<Vec<u32>> = vec![Vec::new(); ways.len()];
    for (i, s) in segs.iter().enumerate() {
        way_segs[s.way as usize].push(i as u32);
    }
    let neigh: Vec<Vec<u32>> = (0..ways.len())
        .into_par_iter()
        .map(|w| {
            if tags[w].maxspeed.is_some() || service[w] || tags[w].key.is_empty() {
                return Vec::new();
            }
            let mut v: Vec<u32> = Vec::new();
            for &si in &way_segs[w] {
                for u in [segs[si as usize].a, segs[si as usize].b] {
                    for &sj in &adj[adj_start[u as usize] as usize..adj_start[u as usize + 1] as usize] {
                        let x = segs[sj as usize].way;
                        if x as usize != w {
                            v.push(x);
                        }
                    }
                }
            }
            v.sort_unstable();
            v.dedup();
            v
        })
        .collect();
    for (w, (s, src)) in speeds(&tags, &service, &neigh).into_iter().enumerate() {
        ways[w].speed = s;
        ways[w].speed_src = src;
    }

    let mut grid: FxHashMap<(i32, i32), Vec<u32>> = FxHashMap::default();
    for (i, &(lo, la)) in xy.iter().enumerate() {
        if adj_start[i + 1] > adj_start[i] {
            grid.entry(cell(lo, la)).or_default().push(i as u32);
        }
    }

    // every cell a segment crosses: sampled every quarter cell along it
    let mut seg_grid: FxHashMap<(i32, i32), Vec<u32>> = FxHashMap::default();
    for (i, s) in segs.iter().enumerate() {
        let (a, b) = (xy[s.a as usize], xy[s.b as usize]);
        let steps = (((b.0 - a.0).abs().max((b.1 - a.1).abs()) * 50.0 * 4.0).ceil() as usize).max(1);
        let mut last = None;
        for k in 0..=steps {
            let f = k as f64 / steps as f64;
            let c = cell(a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f);
            if last != Some(c) {
                let v = seg_grid.entry(c).or_default();
                if v.last() != Some(&(i as u32)) {
                    v.push(i as u32);
                }
                last = Some(c);
            }
        }
    }

    let mut comp = vec![-1i32; n];
    let mut nc = 0;
    for s in 0..n {
        if comp[s] >= 0 || adj_start[s + 1] == adj_start[s] {
            continue;
        }
        comp[s] = nc;
        let mut stack = vec![s as u32];
        while let Some(u) = stack.pop() {
            for &si in &adj[adj_start[u as usize] as usize..adj_start[u as usize + 1] as usize] {
                let sg = &segs[si as usize];
                let v = if sg.a == u { sg.b } else { sg.a };
                if comp[v as usize] < 0 {
                    comp[v as usize] = nc;
                    stack.push(v);
                }
            }
        }
        nc += 1;
    }
    Ok(Graph { xy, segs, adj_start, adj, ways, grid, seg_grid, comp, lines, ways_without_node_ids: without_ids, dup_joins })
}
