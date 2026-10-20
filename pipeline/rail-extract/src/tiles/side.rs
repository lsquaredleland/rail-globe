//! Step 4 of `run`: the JSON side files the app loads on demand (`<out>/<slug>/...`): the lines index, named lines, railways, and the sharded
//! tags, stations, lines and name highlights, plus `shards.json` and the region boundary.

use super::dashes::{geojson_value, hl_record};
use super::track::Scan;
use super::util::{colour, rank, short, write_json, write_shards};
use super::{Logos, Lookup, BIG_LINE_BYTES};
use crate::geomx::{self, round};
use crate::site::{s, Input};
use geo::{Geometry, Simplify};
use rustc_hash::FxHashMap;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) struct Side<'a> {
    pub slug: &'a str,
    pub inp: &'a Input,
    pub lk: &'a Lookup<'a>,
    pub sc: &'a Scan<'a>,
    pub stations: &'a [Value],
    pub complexes: &'a [Value],
    pub route_order: &'a Map<String, Value>,
    pub airport_of: &'a [Option<String>],
    pub logos: &'a Logos,
    /// `<out>/<slug>`
    pub dir: &'a Path,
    pub out: &'a Path,
    pub regions_dirs: &'a [PathBuf],
}

/// km of a route relation: the summed length of its way members
fn route_km(sc: &Scan, r: &Value) -> f64 {
    r["members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["type"] == "way" && matches!(m["role"].as_str(), Some("" | "route")))
        .map(|m| sc.way_len.get(&m["id"].as_i64().unwrap_or(0)).copied().unwrap_or(0.0))
        .sum()
}

impl<'a> Side<'a> {
    /// Writes every side file. Returns the number of named railway lines (for the summary).
    pub fn write_all(&self) -> usize {
        self.lines_index();
        let n_named = self.named_lines_and_railways();
        let (props_by_id, n_tag_shards) = self.tags();
        let n_station_shards = self.stations(&props_by_id);
        let (n_line_shards, big_lines) = self.lines();
        let n_hl_shards = self.name_highlights();
        // shard counts for the app: it hashes a key with fnv1a and takes it modulo the count; `lines_big` are line ids kept as their own files
        write_json(
            &self.dir.join("shards.json"),
            &json!({"stations": n_station_shards, "lines": n_line_shards, "lines_big": big_lines, "tags": n_tag_shards, "name-hl": n_hl_shards}),
        );
        self.boundary();
        n_named
    }

    fn lines_index(&self) {
        let mut idx = Map::new();
        for l in &self.lk.lines {
            let mut km = 0.0f64;
            for rid in l["routes"].as_array().into_iter().flatten() {
                if let Some(r) = self.lk.rels.get(rid.as_str().unwrap_or("")) {
                    km = km.max(route_km(self.sc, r));
                }
            }
            let id = l["id"].as_str().unwrap_or("");
            idx.insert(
                short(id).to_string(),
                json!({"n": l["name"], "r": l["ref"], "c": colour(l["colour"].as_str()), "t": l["type"], "o": l["operator"],
                "f": l["from"], "to": l["to"], "km": Value::from(round(km, 1)), "ne": l.get("name_en").cloned().unwrap_or(Value::Null)}),
            );
        }
        write_json(&self.dir.join("lines-index.json"), &Value::Object(idx));
    }

    /// named-lines.json (named track with no line, 1.5 km or longer) and railways.json ([name, English name, type, km] by railway number `nk`)
    fn named_lines_and_railways(&self) -> usize {
        let mut nl: Vec<&(String, Option<String>, String, f64, i64, [f64; 4])> = self.sc.named.iter().filter(|v| v.3 >= 1.5).collect();
        // by length rounded to 0.1 km, longest first (build_tiles.py sorted after rounding: equal lengths keep first-seen order)
        nl.sort_by(|a, b| round(b.3, 1).partial_cmp(&round(a.3, 1)).unwrap_or(std::cmp::Ordering::Equal));
        write_json(
            &self.dir.join("named-lines.json"),
            &Value::Array(
                nl.iter()
                    .map(|v| {
                        json!({"n": v.0, "ne": v.1, "t": v.2, "km": Value::from(round(v.3, 1)), "ways": v.4,
            "bbox": v.5.iter().map(|x| Value::from(round(*x, 4))).collect::<Vec<_>>()})
                    })
                    .collect(),
            ),
        );
        write_json(
            &self.dir.join("railways.json"),
            &Value::Array(self.sc.railways.iter().map(|r| json!([r.0, r.1, r.2, Value::from(round(r.3, 1))])).collect()),
        );
        nl.len()
    }

    /// raw (filtered) OSM tags plus Rail Globe derived fields, sharded by id for the app's Inspect mode. Returns the properties by id and the shard count.
    fn tags(&self) -> (FxHashMap<&'a str, &'a Map<String, Value>>, u32) {
        let mut props_by_id: FxHashMap<&str, &Map<String, Value>> = FxHashMap::default();
        let mut tag_recs: Vec<(String, String)> = Vec::with_capacity(self.inp.feats.len());
        for fe in &self.inp.feats {
            let oid = s(&fe.p, "osm_id").unwrap_or("");
            props_by_id.insert(oid, &fe.p);
            let without: Map<String, Value> = fe.p.iter().filter(|(k, _)| k.as_str() != "osm_id").map(|(k, v)| (k.clone(), v.clone())).collect();
            tag_recs.push((oid.to_string(), Value::Object(without).to_string()));
        }
        let n = write_shards(&self.dir.join("tags"), &tag_recs);
        (props_by_id, n)
    }

    fn stations(&self, props_by_id: &FxHashMap<&str, &Map<String, Value>>) -> u32 {
        let (lk, stations) = (self.lk, self.stations);
        let cx_name_of: FxHashMap<&str, &Value> =
            self.complexes.iter().map(|c| (c["properties"]["id"].as_str().unwrap_or(""), &c["properties"]["name"])).collect();
        let mut cx_members: FxHashMap<&str, Vec<&Value>> = FxHashMap::default();
        for x in stations.iter() {
            cx_members.entry(x["properties"]["complex"].as_str().unwrap_or("")).or_default().push(&x["properties"]);
        }
        let model_keys = [
            "score",
            "tier",
            "major",
            "terminus",
            "dead_ends",
            "mainline_platforms",
            "platform_count",
            "mainline_track_ways",
            "land_ha",
            "building_ha",
            "grouping",
            "hsr",
        ];
        let mut station_recs: Vec<(String, String)> = Vec::with_capacity(stations.len());
        for (k, stv) in stations.iter().enumerate() {
            let p = &stv["properties"];
            let mut groups: Vec<(String, Vec<i64>)> = Vec::new();
            for lid in p["line_ids"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                if let Some(t) = lk.ls(lid, "type") {
                    match groups.iter_mut().find(|g| g.0 == t) {
                        Some(g) => g.1.push(short(lid)),
                        None => groups.push((t.to_string(), vec![short(lid)])),
                    }
                }
            }
            groups.sort_by_key(|g| rank(&g.0));
            let others: Vec<Value> = cx_members
                .get(p["complex"].as_str().unwrap_or(""))
                .into_iter()
                .flatten()
                .filter(|o| o["id"] != p["id"])
                .map(|o| json!({"id": o["id"], "name": o["name"], "name_en": o["name_en"], "modes": o["modes"]}))
                .collect();
            let sources: Vec<&str> = p["source_ids"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            let mut tags: Vec<Value> = sources
                .iter()
                .filter_map(|i| {
                    props_by_id.get(i).map(|pr| {
                        let t: Map<String, Value> = pr.iter().filter(|(k, _)| k.as_str() != "osm_id").map(|(k, v)| (k.clone(), v.clone())).collect();
                        json!({"id": i, "tags": t})
                    })
                })
                .collect();
            tags.extend(sources.iter().filter_map(|i| lk.rels.get(i).map(|r| json!({"id": i, "tags": r["tags"]}))));
            let model: Map<String, Value> = model_keys.iter().map(|k| (k.to_string(), p.get(*k).cloned().unwrap_or(Value::Null))).collect();
            let rec = json!({
                "id": p["id"], "name": p["name"], "name_en": p["name_en"], "tier": p["tier"], "terminus": p["terminus"],
                "hsr": p["hsr"].as_bool().unwrap_or(false) || p["line_counts"]["high_speed"].as_i64().unwrap_or(0) > 0, "modes": p["modes"],
                "platforms": p["mainline_platforms"], "platform_count": p["platform_count"],
                "platform_refs": p["platform_refs"], "platform_refs_other": p["platform_refs_other"],
                "rail_lines": p.get("rail_lines").cloned().unwrap_or(json!([])),
                "line_count": p["line_count"], "lines": Value::Object(groups.into_iter().map(|(t, mut v)| { v.sort(); (t, json!(v)) }).collect()),
                "complex": if others.is_empty() { Value::Null } else { json!({"id": p["complex"], "name": cx_name_of.get(p["complex"].as_str().unwrap_or("")).copied().cloned().unwrap_or(Value::Null), "others": others}) },
                "construction": p["construction"].as_bool().unwrap_or(false), "airport": self.airport_of[k],
                "osm": sources.first().copied(), "wikidata": p["wikidata"], "pt": stv["geometry"]["coordinates"], "logo": self.logos.for_station(p, lk),
                "tags": tags, "model": Value::Object(model),
            });
            station_recs.push((p["id"].as_str().unwrap_or("").to_string(), rec.to_string()));
        }
        write_shards(&self.dir.join("stations"), &station_recs)
    }

    /// one record per line (its routes, stations, tags and highlight geometry); a long one stays its own file in lines-big/.
    /// Returns the shard count and the ids of the big lines.
    fn lines(&self) -> (u32, Vec<i64>) {
        let (lk, sc) = (self.lk, self.sc);
        let st_by_id: FxHashMap<&str, &Value> = self.stations.iter().map(|x| (x["properties"]["id"].as_str().unwrap_or(""), &x["properties"])).collect();
        let mut line_recs: Vec<(String, String)> = Vec::with_capacity(lk.lines.len());
        let mut big_lines: Vec<i64> = Vec::new();
        for l in &lk.lines {
            let lid = l["id"].as_str().unwrap_or("");
            let mut routes: Vec<Value> = Vec::new();
            for rid in l["routes"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                let Some(r) = lk.rels.get(rid) else { continue };
                let wl = route_km(sc, r);
                let order = self.route_order.get(rid).cloned().unwrap_or(json!([]));
                routes.push(json!({"id": rid, "name": r["tags"]["name"], "from": r["tags"]["from"], "to": r["tags"]["to"], "km": Value::from(round(wl, 1)), "stations": order}));
            }
            routes.sort_by(|a, b| {
                let k = |r: &Value| (r["stations"].as_array().map_or(0, |v| v.len()), r["km"].as_f64().unwrap_or(0.0));
                let (ka, kb) = (k(a), k(b));
                kb.0.cmp(&ka.0).then(kb.1.partial_cmp(&ka.1).unwrap_or(std::cmp::Ordering::Equal))
            });
            let used: BTreeSet<&str> = routes.iter().take(8).flat_map(|r| r["stations"].as_array().into_iter().flatten().filter_map(Value::as_str)).collect();
            let names: Map<String, Value> =
                used.iter().filter_map(|sid| st_by_id.get(sid).map(|p| (sid.to_string(), json!({"n": p["name"], "ne": p["name_en"]})))).collect();
            let km = routes.iter().filter_map(|r| r["km"].as_f64()).fold(None, |a: Option<f64>, x| Some(a.map_or(x, |a| a.max(x))));
            let c = colour(l["colour"].as_str());
            let mut extra = vec![("type", l["type"].clone())];
            if let Some(c) = &c {
                extra.push(("oc", json!(c)));
            }
            let rec = json!({
                "id": short(lid), "name": l["name"], "ref": l["ref"], "type": l["type"], "colour": c,
                "operator": l["operator"], "network": l["network"], "from": l["from"], "to": l["to"],
                "km": km.map_or(json!(0), Value::from), "osm": lid,
                "routes": routes.iter().take(8).collect::<Vec<_>>(), "station_names": names, "tags": l.get("tags").filter(|t| !t.is_null()).cloned().unwrap_or(json!({})),
                "name_en": l.get("name_en").cloned().unwrap_or(Value::Null),
                "route_ids": l["routes"].as_array().map(|v| v.iter().take(30).cloned().collect::<Vec<_>>()).unwrap_or_default(),
                "hl": hl_record(sc.line_ways.get(&short(lid)).map(Vec::as_slice).unwrap_or(&[]), &sc.way_geom, extra),
            });
            let text = rec.to_string();
            if text.len() > BIG_LINE_BYTES {
                big_lines.push(short(lid));
                let path = self.dir.join("lines-big").join(format!("{}.json", short(lid)));
                fs::write(&path, text).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
            } else {
                line_recs.push((short(lid).to_string(), text));
            }
        }
        let n = write_shards(&self.dir.join("lines"), &line_recs);
        big_lines.sort_unstable();
        (n, big_lines)
    }

    /// highlight data for named railways, sharded by a hash of the name (any track name: a hovered piece with no mapped line)
    fn name_highlights(&self) -> u32 {
        let sc = self.sc;
        let mut hl_recs: Vec<(String, String)> = Vec::with_capacity(sc.name_ways.len());
        for (ix, (n, ids, first)) in sc.name_ways.iter().enumerate() {
            // the type most of its ways have, not the first way's: a railway often starts with a yard or siding piece (Korea's Gyeonggang Line did)
            let mut by: Vec<(&String, &usize)> = sc.name_types[ix].iter().filter(|(k, _)| k.as_str() != "yard").collect();
            by.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
            let t = by.first().map(|e| e.0).unwrap_or(first);
            hl_recs.push((n.clone(), hl_record(ids, &sc.way_geom, vec![("type", json!(t))]).to_string()));
        }
        write_shards(&self.dir.join("name-hl"), &hl_recs)
    }

    /// boundary (simplified) for the faint region outline: hand-made outline first, else one written by `rail-extract outlines`
    fn boundary(&self) {
        let slug = self.slug;
        if let Some(path) = self.regions_dirs.iter().map(|d| d.join(format!("{slug}.geojson"))).find(|p| p.exists()) {
            let b: Value = serde_json::from_str(&fs::read_to_string(&path).expect("read outline")).expect("parse outline");
            if let Some(g) = geomx::from_geojson(&b["geometry"], geomx::id) {
                let g = match g {
                    Geometry::Polygon(p) => Geometry::Polygon(p.simplify(0.0008)),
                    Geometry::MultiPolygon(m) => Geometry::MultiPolygon(m.simplify(0.0008)),
                    g => g,
                };
                let v = json!({"type": "Feature", "properties": {"slug": slug}, "geometry": geojson_value(&g)});
                fs::write(self.out.join(format!("{slug}.boundary.geojson")), v.to_string()).expect("write boundary");
            }
        }
    }
}
