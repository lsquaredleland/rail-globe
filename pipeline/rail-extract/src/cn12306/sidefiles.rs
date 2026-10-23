//! The side files of the app for the 12306 services: `<out>/<slug>/cn12306/<rev>/` with the station records in hash shards (`<n>.json`),
//! `ids.json`, `stretches.json` and `ways/<cell>.json`; shards.json says which revision is current ("cn12306": rev, count, snapshot,
//! cells). Every file of a revision lives under its folder, so records and stretch data of two builds never mix.

use super::draw;
use super::graph::{Graph, NO_LINE};
use super::records::{self, AppStations, Cities};
use super::stations::TrainRun;
use super::stretch::Stretch;
use super::Side;
use crate::tiles::write_shards_n;
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

fn put(dir: &Path, name: &str, text: &str) -> super::R<()> {
    super::io(std::fs::write(dir.join(name), text), "write", &dir.join(name))
}

pub fn gz_len(bytes: &[u8]) -> usize {
    let mut c = libdeflater::Compressor::new(libdeflater::CompressionLvl::new(6).unwrap());
    let mut out = vec![0u8; c.gzip_compress_bound(bytes.len())];
    c.gzip_compress(bytes, &mut out).unwrap_or(0)
}

/// simplification of the lines below zoom 9: tolerance in degrees, decimals kept
const GEO_TOL: f64 = 0.006;
const GEO_DEC: i32 = 3;
/// degrees of a cell of the way files
const WAY_CELL: f64 = 2.0;
/// degrees from its middle vertex beyond which a way is filed in every cell it touches
const LONG_WAY: f64 = 0.25;
/// record shards: about 10 records each
const SHARDS: u32 = 1024;

/// takes the 12306 services out of a country's side files: the folder and the entry in shards.json
pub fn remove(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir.join("cn12306"));
    let sp = dir.join("shards.json");
    if let Some(mut v) = std::fs::read_to_string(&sp).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        if v.as_object_mut().and_then(|o| o.remove("cn12306")).is_some() {
            let _ = std::fs::write(&sp, serde_json::to_string(&v).unwrap());
        }
    }
}

/// metres, as km, within which a way of the same named line runs alongside a stretch's way (the hop of the canonical redraw)
const TWIN_KM: f64 = 0.03;

/// For each stretch the ways of the SAME named line that run alongside its ways (half of their vertices within 30 m of a SEGMENT of the
/// stretch) and are on no stretch themselves: the other track of a double line, which the canonical redraw left unused. They light with
/// the stretch; a way goes to the stretch it is alongside most.
pub(super) fn twins(g: &Graph, st: &[Stretch]) -> Vec<Vec<u32>> {
    let mut owner = vec![u32::MAX; g.ways.len()];
    for (si, s) in st.iter().enumerate() {
        for &w in &s.ways {
            owner[w as usize] = si as u32;
        }
    }
    let cell = |p: (f64, f64)| ((p.0 * 2000.0).floor() as i32, (p.1 * 2000.0).floor() as i32);
    // the segments of the ways on stretches, in every cell (about 50 m) they pass through
    let mut grid: FxHashMap<(i32, i32), Vec<u32>> = FxHashMap::default();
    for (i, sg) in g.segs.iter().enumerate() {
        if owner[sg.way as usize] == u32::MAX || g.ways[sg.way as usize].line == NO_LINE {
            continue;
        }
        let (a, b) = (g.xy[sg.a as usize], g.xy[sg.b as usize]);
        let steps = (((b.0 - a.0).abs().max((b.1 - a.1).abs()) * 2000.0 * 2.0).ceil() as usize).max(1);
        let mut last = None;
        for k in 0..=steps {
            let f = k as f64 / steps as f64;
            let c = cell((a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f));
            if last != Some(c) {
                grid.entry(c).or_default().push(i as u32);
                last = Some(c);
            }
        }
    }
    // the vertices of the candidate ways
    let mut nodes: Vec<Vec<u32>> = vec![Vec::new(); g.ways.len()];
    for sg in &g.segs {
        let w = &g.ways[sg.way as usize];
        if owner[sg.way as usize] == u32::MAX && !w.service && w.line != NO_LINE {
            nodes[sg.way as usize].extend([sg.a, sg.b]);
        }
    }
    let mut out: Vec<Vec<u32>> = vec![Vec::new(); st.len()];
    for (x, ns) in nodes.iter_mut().enumerate() {
        ns.sort_unstable();
        ns.dedup();
        if ns.is_empty() {
            continue;
        }
        let line = g.ways[x].line;
        let mut hits: BTreeMap<u32, FxHashSet<u32>> = BTreeMap::new(); // stretch -> vertices of x alongside it
        for &n in ns.iter() {
            let p = g.xy[n as usize];
            let (cx, cy) = cell(p);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for &si in grid.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                        let sg = &g.segs[si as usize];
                        if g.ways[sg.way as usize].line == line && super::route::snap_on_seg(p.0, p.1, g.xy[sg.a as usize], g.xy[sg.b as usize]).1 <= TWIN_KM {
                            hits.entry(owner[sg.way as usize]).or_default().insert(n);
                        }
                    }
                }
            }
        }
        // the stretch it is alongside most (the lowest on a tie)
        let best = hits.iter().filter(|(_, h)| h.len() * 2 >= ns.len()).max_by(|a, b| a.1.len().cmp(&b.1.len()).then(b.0.cmp(a.0)));
        if let Some((&si, _)) = best {
            out[si as usize].push(x as u32);
        }
    }
    out
}

/// the first 32 bits (8 hex digits) of the SHA-1 of every file under `dir`: path, length and bytes, in path order
fn content_rev(dir: &Path) -> super::R<String> {
    fn walk(d: &Path, rel: &str, out: &mut Vec<(String, std::path::PathBuf)>) -> super::R<()> {
        let mut es: Vec<_> = super::io(std::fs::read_dir(d), "read", d)?.flatten().collect();
        es.sort_by_key(|e| e.file_name());
        for e in es {
            let name = format!("{rel}{}", e.file_name().to_string_lossy());
            if e.path().is_dir() {
                walk(&e.path(), &format!("{name}/"), out)?;
            } else {
                out.push((name, e.path()));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(dir, "", &mut files)?;
    files.sort();
    let mut all: Vec<u8> = Vec::new();
    for (name, path) in files {
        let bytes = super::io(std::fs::read(&path), "read", &path)?;
        all.extend(name.as_bytes());
        all.extend((bytes.len() as u64).to_le_bytes());
        all.extend(bytes);
    }
    Ok(format!("{:016x}", super::stretch::sha1_64(&all))[..8].to_string())
}

pub fn write(
    side: &Side,
    snapshot: &Value,
    g: &Graph,
    st: &[Stretch],
    runs: &[TrainRun],
    english: &FxHashMap<String, String>,
    gazetteer: &FxHashMap<String, String>,
) -> super::R<Value> {
    remove(side.dir);

    // the stretches of every train of the timetable, ascending
    let mut train_stretches: Vec<Vec<u32>> = vec![Vec::new(); runs.len()];
    for (si, s) in st.iter().enumerate() {
        for &t in &s.trains {
            train_stretches[t as usize].push(si as u32);
        }
    }

    let twin = twins(g, st);
    let n_twins: usize = twin.iter().map(|t| t.len()).sum();
    let app = AppStations::load(side.cut)?;
    let cities = Cities::load(side.regions, gazetteer)?;
    let out = records::build(runs, &train_stretches, &app, &cities, english, gazetteer);

    // written under a temporary name; the revision is a hash of the content of every file, so any change of data is a new revision
    let dir = side.dir.join("cn12306").join("building");
    let ways_dir = dir.join("ways");
    super::io(std::fs::create_dir_all(&ways_dir), "create", &ways_dir)?;
    write_shards_n(&dir, &out.records, SHARDS);
    put(&dir, "ids.json", &serde_json::to_string(&out.ids).unwrap_or_default())?;

    // stretch drawing data below zoom 9: lines as integers in 1/q degrees (the first point, then steps), and the rail type
    let wanted: FxHashSet<&str> = st.iter().flat_map(|s| s.ways.iter()).chain(twin.iter().flatten()).map(|&w| g.ways[w as usize].id.as_str()).collect();
    let geo = draw::read_ways(&side.cut.join("features.typed.geojsonl"), &wanted)?;
    let mut geo_list: Vec<Value> = Vec::with_capacity(st.len());
    // from zoom 9 the ways themselves: by the WAY_CELL degree cell of the way's middle vertex, stretch number -> way numbers
    let mut cells: BTreeMap<String, BTreeMap<usize, Vec<u64>>> = BTreeMap::new();
    for (si, s) in st.iter().enumerate() {
        let ids: Vec<&String> = s.ways.iter().map(|&w| &g.ways[w as usize].id).collect();
        let members: Vec<&draw::WayGeo> = ids.iter().filter_map(|id| geo.get(id.as_str())).collect();
        let lines: Vec<&[(f64, f64)]> = members.iter().map(|w| w.coords.as_slice()).collect();
        let parts: Vec<Value> = draw::simplify(&lines, GEO_TOL, GEO_DEC)
            .iter()
            .map(|ln| {
                let mut flat: Vec<i64> = Vec::with_capacity(ln.len() * 2);
                let mut prev = (0, 0);
                for &p in ln {
                    flat.extend([p.0 - prev.0, p.1 - prev.1]);
                    prev = p;
                }
                json!(flat)
            })
            .collect();
        geo_list.push(json!({ "t": draw::stretch_type(members.into_iter()), "c": parts }));
        for &w in s.ways.iter().chain(&twin[si]) {
            let way = &g.ways[w as usize];
            let Some(num) = way.id.strip_prefix("way/").and_then(|x| x.parse::<u64>().ok()) else { continue };
            let cell = |lon: f64, lat: f64| ((lon / WAY_CELL).floor() as i64, (lat / WAY_CELL).floor() as i64);
            // a way reaching far from its middle vertex is filed in every cell its box touches, so a view finds it from any of them
            let (x0, y0, x1, y1) = match geo.get(way.id.as_str()) {
                Some(w) if w.coords.iter().any(|c| (c.0 - way.mid.0).abs().max((c.1 - way.mid.1).abs()) > LONG_WAY) => {
                    let (mut lo, mut hi) = ((f64::INFINITY, f64::INFINITY), (f64::NEG_INFINITY, f64::NEG_INFINITY));
                    for c in &w.coords {
                        lo = (lo.0.min(c.0), lo.1.min(c.1));
                        hi = (hi.0.max(c.0), hi.1.max(c.1));
                    }
                    let (lo, hi) = (cell(lo.0, lo.1), cell(hi.0, hi.1));
                    (lo.0, lo.1, hi.0, hi.1)
                }
                _ => {
                    let c = cell(way.mid.0, way.mid.1);
                    (c.0, c.1, c.0, c.1)
                }
            };
            for cx in x0..=x1 {
                for cy in y0..=y1 {
                    cells.entry(format!("{cx}_{cy}")).or_default().entry(si).or_default().push(num);
                }
            }
        }
    }
    let geo_text = json!({ "q": 10i64.pow(GEO_DEC as u32), "s": geo_list }).to_string();
    put(&dir, "stretches.json", &geo_text)?;
    let (mut ways_raw, mut ways_gz, mut biggest) = (0, 0, 0);
    for (key, per) in &cells {
        let obj: serde_json::Map<String, Value> = per
            .iter()
            .map(|(si, ws)| {
                let mut ws = ws.clone();
                ws.sort_unstable();
                (si.to_string(), json!(ws))
            })
            .collect();
        let text = Value::Object(obj).to_string();
        ways_raw += text.len();
        let gz = gz_len(text.as_bytes());
        ways_gz += gz;
        biggest = biggest.max(gz);
        put(&ways_dir, &format!("{key}.json"), &text)?;
    }

    let rev = content_rev(&dir)?;
    let fin = side.dir.join("cn12306").join(&rev);
    super::io(std::fs::rename(&dir, &fin), "rename", &dir)?;
    let dir = fin;

    // shards.json learns the new kind
    let sp = side.dir.join("shards.json");
    let text = super::io(std::fs::read_to_string(&sp), "read", &sp)?;
    let mut shards: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", sp.display()))?;
    if !shards.is_object() {
        return Err(format!("{} is not a JSON object", sp.display()));
    }
    shards["cn12306"] = json!({ "rev": rev, "count": SHARDS, "snapshot": snapshot, "cells": cells.keys().collect::<Vec<_>>() });
    super::io(std::fs::write(&sp, serde_json::to_string(&shards).unwrap_or_default()), "write", &sp)?;

    let mut report = out.report;
    let sizes: Vec<usize> = out.records.iter().map(|r| r.1.len()).collect();
    let mut shard_sizes: Vec<u64> = (0..SHARDS).filter_map(|i| std::fs::metadata(dir.join(format!("{i}.json"))).ok().map(|m| m.len())).collect();
    shard_sizes.sort_unstable();
    report["rev"] = json!(rev);
    report["twin_ways"] = json!(n_twins);
    report["shards"] = json!({ "count": SHARDS, "median_bytes": shard_sizes.get(shard_sizes.len() / 2), "max_bytes": shard_sizes.last() });
    report["sizes"] = json!({
        "records_raw": sizes.iter().sum::<usize>(),
        "stretches": [geo_text.len(), gz_len(geo_text.as_bytes())],
        "ways_cells": cells.len(), "ways_raw": ways_raw, "ways_gz": ways_gz, "ways_biggest_cell_gz": biggest,
    });
    Ok(report)
}
