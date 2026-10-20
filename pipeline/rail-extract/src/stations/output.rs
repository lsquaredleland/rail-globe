//! Names and the output rows: stations.geojsonl, complexes.geojsonl, and the report.

use super::features::Proj;
use super::link::{Complexes, Lines};
use super::names::{bare_kind, clean_name, english_name, place_before, qualified};
use super::size::{ref_key, ref_tokens, Size};
use super::{station_id, station_wikidata, Model, Out, Station, GROUP_RADIUS};
use crate::geomx;
use crate::pyjson::f;
use geo::Geometry;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// `{**a, **b}`: a's keys in order, b's values winning, b's new keys appended
fn merge_tags(mut a: Map<String, Value>, b: Map<String, Value>) -> Map<String, Value> {
    for (k, v) in b {
        a.insert(k, v);
    }
    a
}

// Naming. A station's name: a stop area's name first, else the station's best-named feature (named first, then points); the
// stop areas are taken in sorted order, so runs repeat.
impl Model<'_> {
    /// is `cand` the name of a station within GROUP_RADIUS of x
    fn bare_near(&self, x: &Station, cand: &str) -> bool {
        let (st, station_feats) = (&self.st, &self.feats.station_feats);
        self.st_tree.near(&Geometry::Point(x.pt), GROUP_RADIUS).into_iter().any(|j| {
            geomx::dist_pt(&Geometry::Point(x.pt), &st[j].pt) <= GROUP_RADIUS
                && (st[j].feats.iter().any(|&k| station_feats[k].p("name") == Some(cand) || station_feats[k].p("name:en") == Some(cand))
                    || st[j].areas.iter().any(|a| self.rels.area_name(a) == Some(cand)))
        })
    }

    /// is `c` an operator or network of x's features
    fn is_op(&self, x: &Station, c: &str) -> bool {
        x.feats.iter().any(|&k| ["operator", "network", "operator:short", "brand"].iter().any(|key| self.feats.station_feats[k].p(key).map_or(false, |v| v.split(';').any(|t| t.trim() == c))))
    }

    /// a name cleaned of operator and format noise (clean_name), judged against the stations around x
    fn label(&self, x: &Station, n: &str) -> String {
        clean_name(n, &|c| self.bare_near(x, c), &|c| self.is_op(x, c))
    }

    /// the place that qualifies a bare "Hauptbahnhof" / "Bahnhof" / "Station" / "Gare": addr:city (or addr:place) on its own features or
    /// stop area, else the place in front of the same word in the name of the nearest station within 800 m ("Bremen Hbf"). The build has
    /// no place nodes at this stage, so nothing else is available; None leaves the bare name.
    fn place_of(&self, x: &Station, kind: &str) -> Option<String> {
        let (st, station_feats) = (&self.st, &self.feats.station_feats);
        let tag = |m: &Map<String, Value>| ["addr:city", "addr:place"].iter().find_map(|k| m.get(*k).and_then(Value::as_str).filter(|v| !v.is_empty()).map(String::from));
        if let Some(c) = x.feats.iter().find_map(|&k| tag(&station_feats[k].f.p)).or_else(|| x.areas.iter().find_map(|a| self.rels.area_tag_map(a).and_then(tag))) {
            return Some(c);
        }
        let here = Geometry::Point(x.pt);
        let mut best: Option<(f64, String)> = None;
        for j in self.st_tree.near(&here, 800.0) {
            let d = geomx::dist_pt(&here, &st[j].pt);
            if d > 800.0 || best.as_ref().map_or(false, |b| b.0 <= d) {
                continue;
            }
            let names = st[j].feats.iter().flat_map(|&k| [station_feats[k].p("name"), station_feats[k].p("official_name")]).flatten()
                .chain(st[j].areas.iter().filter_map(|a| self.rels.area_name(a)));
            if let Some(p) = names.into_iter().find_map(|n| place_before(n, kind)) {
                best = Some((d, p));
            }
        }
        best.map(|b| b.1)
    }

    /// a bare kind word becomes "<place> <kind>"; without a place "Station" is no name (the caller falls back to another name), the
    /// others stay as they are
    fn kinded(&self, x: &Station, n: &str) -> Option<String> {
        match bare_kind(n) {
            None => Some(n.to_string()),
            Some(kind) => match self.place_of(x, kind) {
                Some(p) => Some(qualified(&p, kind)),
                None if kind == "Station" => None,
                None => Some(n.to_string()),
            },
        }
    }

    /// (the label, the tags it came from, the source name) of a station
    fn station_name(&self, x: &Station) -> (Option<String>, Map<String, Value>, Option<String>) {
        let station_feats = &self.feats.station_feats;
        let best = x.feats.iter().copied().fold(None, |a: Option<usize>, k| {
            let rank = |k: usize| (station_feats[k].p("name").map_or(false, |v| !v.is_empty()), matches!(station_feats[k].g, Geometry::Point(_)));
            if a.map_or(true, |b| rank(k) > rank(b)) { Some(k) } else { a }
        }).unwrap();
        let own = station_feats[best].p("name").filter(|v| !v.is_empty());
        let area = x.areas.iter().find_map(|sa| {
            let tags = self.rels.area_tag_map(sa)?;
            Some((tags.get("name").and_then(Value::as_str).filter(|v| !v.is_empty())?, tags.clone()))
        });
        // the station's own name, not the stop area's (a mislabelled relation named Poznań Główny "Potęgowo"); the area's name only when
        // the station has none, or when the area's name is the own name with a place in front ("Hannover Hauptbahnhof" for "Hauptbahnhof")
        // or the own name is a bare "Station"
        let own = own.filter(|o| bare_kind(o) != Some("Station") || area.is_none());
        let picked: Option<(&str, Map<String, Value>)> = match (own, area) {
            (Some(o), Some((a, at))) if a.to_lowercase().ends_with(&format!(" {}", o.to_lowercase())) => Some((a, at)),
            (Some(o), _) => Some((o, station_feats[best].f.p.clone())),
            (None, Some((a, at))) => Some((a, at)),
            (None, None) => None,
        };
        // a bare "Station" with no other name: another feature of the station that has one
        let picked = picked.filter(|(n, _)| bare_kind(&self.label(x, n)) != Some("Station") || self.place_of(x, "Station").is_some()).or_else(|| {
            x.feats.iter().find_map(|&k| station_feats[k].p("name").filter(|n| !n.is_empty() && bare_kind(n).is_none()).map(|n| (n, station_feats[k].f.p.clone())))
        });
        match picked {
            Some((src, tags)) => (self.kinded(x, &self.label(x, src)), tags, Some(src.to_string())),
            None => (None, station_feats[best].f.p.clone(), None),
        }
    }

    /// name:* tags for the English label. A label in Latin script takes them only from the features and stop areas that carry its own name
    /// (a stop area named "Dadar (Central)" does not give the station labelled "Dadar" its name:en); a label in another script takes all
    /// of the station's name:* tags, which is where its English comes from (北京 gets "Beijing Railway Station" from its stop area)
    fn name_tags(&self, x: &Station, src: Option<&str>) -> Map<String, Value> {
        let strict = src.map_or(false, |n| crate::rules::latin(n.as_bytes()));
        let mut t = Map::new();
        for &k in &x.feats {
            let feat = &self.feats.station_feats[k];
            if strict && feat.p("name") != src {
                continue;
            }
            for (key, v) in &feat.f.p {
                if key.starts_with("name:") || key == "int_name" || key == "official_name" {
                    t.insert(key.clone(), v.clone());
                }
            }
        }
        for sa in &x.areas {
            if let Some(tags) = self.rels.area_tag_map(sa) {
                if strict && tags.get("name").and_then(Value::as_str) != src {
                    continue;
                }
                for (key, v) in tags {
                    if key.starts_with("name:") {
                        t.insert(key.clone(), v.clone());
                    }
                }
            }
        }
        t
    }

    /// the English label of a station: its name tags and the name:* tags of its sources, cleaned like the label
    fn name_en(&self, x: &Station, tags: Map<String, Value>, src: Option<&str>) -> Option<String> {
        english_name(&merge_tags(tags, self.name_tags(x, src))).map(|e| self.label(x, &e))
    }
}

/// the operator's Wikidata id (operator:wikidata, else brand:wikidata) of the station's features: the logo key
fn operator_wikidata(m: &Model, x: &Station) -> Option<String> {
    ["operator:wikidata", "brand:wikidata"].iter().find_map(|key| x.feats.iter().find_map(|&k| m.feats.station_feats[k].p(key).filter(|v| !v.is_empty()).map(String::from)))
}

/// stations.geojsonl rows
fn station_rows(m: &Model, proj: &Proj, sizes: &[Size], lines: &Lines, cx: &Complexes) -> Vec<Value> {
    let (st, platforms) = (&m.st, &m.feats.platforms);
    let lines_by_id: rustc_hash::FxHashMap<&str, &Value> = m.inp.lines.iter().map(|l| (l["id"].as_str().unwrap_or(""), l)).collect();
    let mut rows = Vec::with_capacity(st.len());
    for (i, x) in st.iter().enumerate() {
        let (nm, tg, src) = m.station_name(x);
        let lids: Vec<&String> = lines.st_lines[i].iter().filter(|l| !l.is_empty()).collect();
        let mut by_type: Vec<(String, i64)> = Vec::new();
        for l in &lids {
            if let Some(t) = lines_by_id.get(l.as_str()).and_then(|l| l["type"].as_str()) {
                match by_type.iter_mut().find(|e| e.0 == t) {
                    Some(e) => e.1 += 1,
                    None => by_type.push((t.to_string(), 1)),
                }
            }
        }
        let z = &sizes[i];
        let mut rail: Vec<&Value> = lines.st_rail[i].values().collect();
        rail.sort_by(|a, b| (a["name"].as_str().unwrap_or(""), a["id"].as_str()).cmp(&(b["name"].as_str().unwrap_or(""), b["id"].as_str())));
        let refs = |train: bool| -> Value {
            let set: BTreeSet<String> = x.platforms.iter().filter(|&&k| platforms[k].modes.contains("train") == train).flat_map(|&k| ref_tokens(platforms[k].p("ref"))).collect();
            let mut v: Vec<String> = set.into_iter().collect();
            v.sort_by_key(|r| ref_key(r));
            json!(v)
        };
        let mut sources: Vec<Value> = x.feats.iter().map(|&k| json!(m.feats.station_feats[k].id())).collect();
        sources.extend(x.areas.iter().map(|a| json!(a)));
        let wd = station_wikidata(&m.feats, x);
        let props = json!({
            "id": station_id(&m.feats, x), "complex": cx.of[i], "name": nm, "name_en": m.name_en(x, tg, src.as_deref()),
            "modes": x.modes.iter().collect::<Vec<_>>(), "tier": z.tier, "score": f(z.score), "terminus": z.terminus,
            "mainline_platforms": z.mainline_platforms, "platform_count": z.platform_count,
            "land_ha": f(z.land_ha), "building_ha": f(z.building_ha), "mainline_track_ways": z.mainline_track_ways,
            "dead_ends": z.dead_ends, "hsr": z.hsr, "major": z.major,
            "rail_lines": rail, "platform_refs": refs(true), "platform_refs_other": refs(false),
            "line_count": lids.len(), "line_counts": Value::Object(by_type.into_iter().map(|(k, n)| (k, json!(n))).collect()), "line_ids": lids,
            "operator_wd": operator_wikidata(m, x), "wikidata": if wd.is_empty() { Value::Null } else { json!([wd.iter().next().unwrap()]) }, "grouping": x.grouping,
            "source_ids": sources,
        });
        rows.push(json!({"type": "Feature", "properties": props, "geometry": {"type": "Point", "coordinates": proj.unproj(&x.pt)}}));
    }
    rows
}

/// complexes.geojsonl rows
fn complex_rows(m: &Model, proj: &Proj, sizes: &[Size], lines: &Lines, cx: &Complexes) -> Vec<Value> {
    let st = &m.st;
    let mut rows = Vec::with_capacity(cx.rows.len());
    for c in &cx.rows {
        let x = &st[c.biggest];
        let (nm, tg, src) = m.station_name(x);
        let mut lids: BTreeSet<&String> = BTreeSet::new();
        for &i in &c.members {
            lids.extend(lines.st_lines[i].iter().filter(|l| !l.is_empty()));
        }
        let modes: BTreeSet<&String> = c.members.iter().flat_map(|&i| st[i].modes.iter()).collect();
        let props = json!({
            "id": c.id, "name": nm, "name_en": m.name_en(x, tg, src.as_deref()), "tier": c.members.iter().map(|&i| sizes[i].tier).min(),
            "terminus": c.members.iter().any(|&i| sizes[i].terminus), "station_count": c.members.len(), "modes": modes,
            "platform_count": c.members.iter().map(|&i| sizes[i].platform_count).sum::<i64>(), "line_count": lids.len(), "grouping": c.how,
            "station_ids": c.members.iter().map(|&i| station_id(&m.feats, &st[i])).collect::<Vec<_>>(),
        });
        rows.push(json!({"type": "Feature", "properties": props, "geometry": {"type": "Point", "coordinates": proj.unproj(&x.pt)}}));
    }
    rows
}

/// the stations, complexes, route order and report
pub(super) fn write(slug: &str, m: &Model, proj: &Proj, sizes: &[Size], lines: Lines, cx: &Complexes, stop_unmapped: usize) -> Out {
    let stations = station_rows(m, proj, sizes, &lines, cx);
    let complexes = complex_rows(m, proj, sizes, &lines, cx);
    let mut tiers: BTreeMap<i64, i64> = BTreeMap::new();
    for z in sizes {
        *tiers.entry(z.tier).or_default() += 1;
    }
    let report = json!({
        "region": slug, "stations": m.st.len(), "complexes": cx.rows.len(), "merged_duplicates": m.st.iter().filter(|x| x.merged_from > 1).count(),
        "tiers": Value::Object(tiers.into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect()),
        "terminus_flagged": sizes.iter().filter(|z| z.terminus).count(),
        "stop_positions_unmapped": stop_unmapped, "stop_positions_total": m.inp.stops.len(),
        "routes_with_unmapped_stops": lines.routes_unmapped, "routes_total": lines.routes_total, "mapped_group_unions": cx.mapped_pairs,
    });
    Out { stations, complexes, route_order: lines.route_order, report }
}
