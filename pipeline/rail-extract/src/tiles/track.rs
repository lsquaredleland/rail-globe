//! Step 1 of `run`: one pass over the region's features -> the track, strand, area, building and platform layers, the zoomed-out overview,
//! and the lookups the later steps need (way geometry, names, passenger / freight / construction track beside stations, city samples).

use super::constants::{speed_band_index, speed_band_min};
use super::props::{AreaProps, OverviewProps, PlatformLineProps, StrandProps, TrackProps};
use super::util::{area_ha, badge_of, colour, coords, feat, is_light, label_of, nonempty, rank, short, track_minzoom};
use super::{haversine_len, Lookup, OVERVIEW_MAXZOOM, SKETCH_MINZOOM};
use crate::classify::speed_kmh;
use crate::geomx;
use crate::site::{s, Feat, Input};
use crate::stations::english_name;
use rustc_hash::FxHashMap;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

type Way = Vec<(f64, f64)>;

/// the speed band index of a station's track when no maxspeed is mapped
pub(super) const SB_NONE: u8 = 9;

pub(super) struct Scan<'a> {
    pub track: String,
    pub strands: String,
    pub areas: String,
    pub buildings: String,
    pub plats: String,
    pub n_track: usize,
    pub n_strands: usize,
    pub n_areas: usize,
    pub n_bld: usize,
    pub n_plats: usize,
    /// (sort key, props, geometry, minzoom, maxzoom)
    pub overview: Vec<(String, Map<String, Value>, &'a str, i64, i64)>,
    /// (type, proposed, speed band or -1) -> ways, for the construction dashes
    pub con_groups: BTreeMap<(String, bool, i64), Vec<Way>>,
    /// line number -> its open way ids
    pub line_ways: FxHashMap<i64, Vec<String>>,
    /// (name, way ids, type) in first-seen order: the highlight of a railway by name
    pub name_ways: Vec<(String, Vec<String>, String)>,
    /// per name: ways by type, for the colour of its highlight
    pub name_types: Vec<FxHashMap<String, usize>>,
    pub way_geom: FxHashMap<String, Way>,
    pub way_len: FxHashMap<i64, f64>,
    /// (name, English name, type, km, ways, bbox) of named track with no line
    pub named: Vec<(String, Option<String>, String, f64, i64, [f64; 4])>,
    /// (name, English name, type, km) by railway number (`nk`)
    pub railways: Vec<(String, Option<String>, String, f64)>,
    /// passenger track for station types, and the speed band index of each row
    pub pax: Vec<(geomx::G, String)>,
    pub pax_sb: Vec<u8>,
    /// under-construction track (not proposed), to tell a station that is only being built, and the speed band index of each row
    pub con_pax: Vec<(geomx::G, String)>,
    pub con_sb: Vec<u8>,
    /// freight and yard track, to tell a freight station from one with no track at all
    pub frt: Vec<(geomx::G, String)>,
    /// (lon, lat, km, 0 metro / 1 tram / 2 monorail) samples for the city dots
    pub city_track: Vec<(f64, f64, f64, u8)>,
    name_ix: FxHashMap<String, usize>,
    named_ix: FxHashMap<(String, String), usize>,
    nk_of: FxHashMap<String, usize>,
}

/// Tracks of a way (1 to 4) by which its km is divided, so a double-track line counts once
fn tracks_div(p: &Map<String, Value>) -> f64 {
    let tr = s(p, "tracks").unwrap_or("");
    match tr.parse::<i64>() {
        Ok(n) if n > 0 && tr.chars().all(|c| c.is_ascii_digit()) => n.min(4) as f64,
        _ => 1.0,
    }
}

impl<'a> Scan<'a> {
    pub fn run(inp: &'a Input, lk: &Lookup) -> Scan<'a> {
        let mut sc = Scan {
            track: String::new(),
            strands: String::new(),
            areas: String::new(),
            buildings: String::new(),
            plats: String::new(),
            n_track: 0,
            n_strands: 0,
            n_areas: 0,
            n_bld: 0,
            n_plats: 0,
            overview: Vec::new(),
            con_groups: BTreeMap::new(),
            line_ways: FxHashMap::default(),
            name_ways: Vec::new(),
            name_types: Vec::new(),
            way_geom: FxHashMap::default(),
            way_len: FxHashMap::default(),
            named: Vec::new(),
            railways: Vec::new(),
            pax: Vec::new(),
            pax_sb: Vec::new(),
            con_pax: Vec::new(),
            con_sb: Vec::new(),
            frt: Vec::new(),
            city_track: Vec::new(),
            name_ix: FxHashMap::default(),
            named_ix: FxHashMap::default(),
            nk_of: FxHashMap::default(),
        };
        for fe in &inp.feats {
            let p = &fe.p;
            let t = s(p, "type").filter(|v| !v.is_empty());
            if let (true, Some(t)) = (fe.gtype == "LineString", t) {
                sc.add_track(fe, t, lk);
            } else if fe.gtype == "Polygon" || fe.gtype == "MultiPolygon" {
                sc.add_area(fe);
            } else if fe.gtype == "LineString" && (s(p, "railway") == Some("platform") || s(p, "public_transport") == Some("platform")) {
                let m = PlatformLineProps { id: s(p, "osm_id").unwrap_or("").to_string(), platform_ref: s(p, "ref").unwrap_or("").to_string() }.to_map();
                feat(&mut sc.plats, &m, &fe.graw, Some(13), None);
                sc.n_plats += 1;
            }
        }
        sc
    }

    fn add_track(&mut self, fe: &'a Feat, t: &str, lk: &Lookup) {
        let p = &fe.p;
        let osm_id = s(p, "osm_id").unwrap_or("");
        let c = coords(&fe.g);
        if osm_id.starts_with("way/") {
            self.way_len.insert(short(osm_id), haversine_len(&c));
        }
        let prop = s(p, "railway") == Some("proposed");
        let con = s(p, "railway") == Some("construction") || prop;
        if !con && s(p, "service").is_none_or(str::is_empty) {
            self.sample_city_track(p, t, &c);
        }
        let mut lids: Vec<&str> = p
            .get("_routes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|r| lk.route_line.get(r.as_str().unwrap_or("")).copied())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        lids.sort_by_key(|l| (rank(lk.ls(l, "type").unwrap_or("")), short(l)));
        let tp = Self::track_props(p, osm_id, t, con, prop, &lids, lk);
        let name = tp.name().map(String::from);
        let mz = track_minzoom(t);
        if let Some(n) = name.as_deref().filter(|_| lids.is_empty() && !con && t != "yard" && t != "freight") {
            self.tally_named(n, tp.ne.as_deref(), t, &c, p);
        }
        if let Some(n) = name.as_deref() {
            self.tally_railway(n, tp.ne.as_deref(), t, &c, p);
        }
        feat(&mut self.track, &tp.to_map(), &fe.graw, Some(mz.max(OVERVIEW_MAXZOOM + 1)), None);
        self.n_track += 1;
        if con && t != "yard" && c.len() >= 2 {
            let band = tp.kmh.map_or(-1, speed_band_min);
            self.con_groups.entry((t.to_string(), prop, band)).or_default().push(c.clone());
        }
        self.way_geom.insert(osm_id.to_string(), c);
        if !con {
            for l in &lids {
                self.line_ways.entry(short(l)).or_default().push(osm_id.to_string());
            }
        }
        if let Some(n) = name.as_deref() {
            self.tally_name_ways(n, osm_id, t);
        }
        if mz <= OVERVIEW_MAXZOOM {
            self.add_overview(fe, &tp, name.as_deref(), t, con, mz);
        }
        if !lids.is_empty() && !con && t != "yard" {
            self.add_strands(fe, &lids, lk);
        }
        if !con && matches!(t, "freight" | "yard") {
            if let Some(g) = geomx::from_geojson(&fe.g, geomx::id) {
                self.frt.push((g, t.to_string()));
            }
        }
        if rank(t) < 9 && (!con || s(p, "railway") == Some("construction")) {
            if let Some(g) = geomx::from_geojson(&fe.g, geomx::id) {
                let sb = tp.kmh.map_or(SB_NONE, |k| speed_band_index(k) as u8);
                if con {
                    self.con_pax.push((g, t.to_string()));
                    self.con_sb.push(sb);
                } else {
                    self.pax.push((g, t.to_string()));
                    self.pax_sb.push(sb);
                }
            }
        }
    }

    /// samples of open passenger metro, tram and monorail track about every km, for the city dots
    fn sample_city_track(&mut self, p: &Map<String, Value>, t: &str, c: &[(f64, f64)]) {
        let Some(kind) = (match (s(p, "railway"), t) {
            (_, "metro") => Some(0u8),
            (Some("tram"), _) => Some(1),
            (_, "monorail") => Some(2),
            _ => None,
        }) else {
            return;
        };
        let mut acc = 0.0;
        for (i, w) in c.windows(2).enumerate() {
            acc += haversine_len(w);
            if acc >= 1.0 || i + 2 == c.len() {
                self.city_track.push((w[1].0, w[1].1, acc, kind));
                acc = 0.0;
            }
        }
    }

    /// the properties of one track way
    fn track_props(p: &Map<String, Value>, osm_id: &str, t: &str, con: bool, prop: bool, lids: &[&str], lk: &Lookup) -> TrackProps {
        let mut tp = TrackProps {
            id: osm_id.to_string(),
            ty: t.to_string(),
            usage: nonempty(p, "usage"),
            el: nonempty(p, "electrified"),
            v: nonempty(p, "voltage"),
            ms: nonempty(p, "maxspeed"),
            g: nonempty(p, "gauge"),
            tr: nonempty(p, "tracks"),
            n: nonempty(p, "name"),
            fq: nonempty(p, "frequency"),
            kmh: speed_kmh(s(p, "maxspeed")).filter(|k| *k != 0.0).map(|k| k.round_ties_even() as i64),
            n_rwy: None,
            ne: None,
            sv: nonempty(p, "service"),
            tun: s(p, "tunnel").is_some_and(|v| !v.is_empty() && v != "no"),
            br: s(p, "bridge").is_some_and(|v| !v.is_empty() && v != "no"),
            con,
            prop,
            ln: None,
            nl: None,
            oc: None,
        };
        // track with no name of its own is named after its railway (the route=railway relation it belongs to: LGV Sud-Est), not after its services
        let own_name = s(p, "name").filter(|v| !v.is_empty());
        let name = own_name.or(s(p, "_rwy").filter(|v| !v.is_empty()));
        if own_name.is_none() {
            tp.n_rwy = name.map(String::from);
        }
        if name.is_some() {
            let en = if own_name.is_some() { english_name(p) } else { s(p, "_rwy_en").map(String::from) };
            tp.ne = en.filter(|en| Some(en.as_str()) != name);
        }
        if !lids.is_empty() && !con {
            tp.ln = Some(lids.iter().map(|l| short(l).to_string()).collect::<Vec<_>>().join(","));
            tp.nl = Some(lids.len());
            // one colour for the track below z12: first line (most prominent type) that has a colour
            tp.oc = lids.iter().find_map(|l| colour(lk.ls(l, "colour")));
        }
        tp
    }

    /// named track that no line runs on: its length and box, for named-lines.json (the search)
    fn tally_named(&mut self, n: &str, ne: Option<&str>, t: &str, c: &[(f64, f64)], p: &Map<String, Value>) {
        let ix = *self.named_ix.entry((n.to_string(), t.to_string())).or_insert_with(|| {
            self.named.push((n.to_string(), ne.map(String::from), t.to_string(), 0.0, 0, [180.0, 90.0, -180.0, -90.0]));
            self.named.len() - 1
        });
        let a = &mut self.named[ix];
        a.3 += haversine_len(c) / tracks_div(p);
        a.4 += 1;
        for &(x, y) in c {
            a.5 = [a.5[0].min(x), a.5[1].min(y), a.5[2].max(x), a.5[3].max(y)];
        }
    }

    /// every named way: the railway's number (`nk`), length and most prominent type, for railways.json
    fn tally_railway(&mut self, n: &str, ne: Option<&str>, t: &str, c: &[(f64, f64)], p: &Map<String, Value>) {
        let nk = *self.nk_of.entry(n.to_string()).or_insert_with(|| {
            self.railways.push((n.to_string(), ne.map(String::from), t.to_string(), 0.0));
            self.railways.len() - 1
        });
        let r = &mut self.railways[nk];
        r.3 += haversine_len(c) / tracks_div(p);
        if rank(t) < rank(&r.2) {
            r.2 = t.to_string(); // the most prominent type on the railway
        }
    }

    fn tally_name_ways(&mut self, n: &str, osm_id: &str, t: &str) {
        let ix = *self.name_ix.entry(n.to_string()).or_insert_with(|| {
            self.name_ways.push((n.to_string(), Vec::new(), t.to_string()));
            self.name_types.push(FxHashMap::default());
            self.name_ways.len() - 1
        });
        self.name_ways[ix].1.push(osm_id.to_string());
        *self.name_types[ix].entry(t.to_string()).or_default() += 1;
    }

    /// the zoomed-out overview: attributes reduced to what the overview draws, so equal neighbours merge
    fn add_overview(&mut self, fe: &'a Feat, tp: &TrackProps, name: Option<&str>, t: &str, con: bool, mz: i64) {
        let ov = OverviewProps {
            ty: tp.ty.clone(),
            con: tp.con,
            prop: tp.prop,
            oc: tp.oc.clone(),
            ln: tp.ln.clone(),
            kmh: tp.kmh.map(speed_band_min),
            // a click opens the first line (ln); every named track also carries its railway number (nk): the hover names the railway, not the services
            nk: name.map(|n| self.nk_of[n]),
        }
        .to_map();
        // a sketch of the metro network for z4-6, with no attributes but the type, so it merges into a few long lines
        if t == "metro" && !con {
            let sk = OverviewProps::sketch_metro().to_map();
            self.overview.push((sorted_key(&Value::Object(sk.clone())), sk, fe.graw.as_str(), SKETCH_MINZOOM, mz - 1));
        }
        self.overview.push((sorted_key(&Value::Object(ov.clone())), ov, fe.graw.as_str(), mz, OVERVIEW_MAXZOOM));
    }

    /// one strand per line on the way
    fn add_strands(&mut self, fe: &Feat, lids: &[&str], lk: &Lookup) {
        for (i, l) in lids.iter().enumerate() {
            let c = colour(lk.ls(l, "colour"));
            let rf = c.as_ref().and_then(|_| badge_of(lk.ls(l, "ref")));
            let sp = StrandProps {
                lid: short(l),
                t: lk.ls(l, "type").map(String::from),
                i,
                n: lids.len(),
                lb: label_of(lk.ls(l, "name"), lk.ls(l, "ref")),
                lbe: lk.ls(l, "name_en").filter(|v| !v.is_empty()).map(|en| label_of(Some(en), lk.ls(l, "ref"))),
                rl: rf.is_some() && c.as_deref().is_some_and(is_light),
                rf,
                c,
            }
            .to_map();
            feat(&mut self.strands, &sp, &fe.graw, Some(11), None);
            self.n_strands += 1;
        }
    }

    /// railway land, yards, station buildings and platform polygons
    fn add_area(&mut self, fe: &Feat) {
        let p = &fe.p;
        let ap = |k: &'static str, platform_ref: Option<String>| {
            AreaProps { id: s(p, "osm_id").unwrap_or("").to_string(), ha: area_ha(&fe.g), n: nonempty(p, "name"), op: nonempty(p, "operator"), k, platform_ref }
                .to_map()
        };
        if s(p, "landuse") == Some("railway") {
            feat(&mut self.areas, &ap("land", None), &fe.graw, Some(10), None);
            self.n_areas += 1;
        } else if s(p, "railway") == Some("yard") {
            feat(&mut self.areas, &ap("yard", None), &fe.graw, Some(10), None);
            self.n_areas += 1;
        } else if s(p, "building") == Some("train_station") {
            feat(&mut self.buildings, &ap("building", None), &fe.graw, Some(12), None);
            self.n_bld += 1;
        } else if s(p, "railway") == Some("platform") || s(p, "public_transport") == Some("platform") {
            feat(&mut self.plats, &ap("platform", Some(s(p, "ref").unwrap_or("").to_string())), &fe.graw, Some(13), None);
            self.n_plats += 1;
        }
    }
}

/// the overview merge key: the value with its keys sorted at every level, as compact JSON
fn sorted_key(v: &Value) -> String {
    fn sort(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let mut ks: Vec<&String> = m.keys().collect();
                ks.sort();
                Value::Object(ks.into_iter().map(|k| (k.clone(), sort(&m[k]))).collect())
            }
            Value::Array(a) => Value::Array(a.iter().map(sort).collect()),
            x => x.clone(),
        }
    }
    sort(v).to_string()
}
