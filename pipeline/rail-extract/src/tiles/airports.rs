//! Step 2 of `run`: the airport layers (outline + terminals, one icon point per commercial airport) and which airport a station serves.

use super::props::{AirportProps, AirportPtProps};
use super::dashes::geojson_value;
use super::util::{area_ha, feat, nonempty};
use crate::geomx::{self, Index};
use crate::pyjson::{self, COMPACT};
use crate::site::{s, Input};
use geo::{Geometry, Point};
use regex::Regex;
use serde_json::{Map, Value};

const BUFFER_DEG: f64 = 0.003; // about 300 m: stations sit at the terminal edge, often just outside the mapped aerodrome

pub(super) struct Airports<'a> {
    pub airports: String,
    pub airport_pts: String,
    pub n_air: usize,
    pub n_air_pts: usize,
    /// the commercial airports (IATA code, not military, 300 ha or more) and their properties: stations get the airport label only at these
    aero_geoms: Vec<geomx::G>,
    aero_props: Vec<&'a Map<String, Value>>,
    aero_tree: Index,
    words: Regex,
}

impl<'a> Airports<'a> {
    pub fn build(inp: &'a Input) -> Airports<'a> {
        let (mut airports, mut airport_pts) = (String::new(), String::new());
        let (mut aero_geoms, mut aero_props): (Vec<geomx::G>, Vec<&Map<String, Value>>) = (vec![], vec![]);
        let (mut n_air, mut n_air_pts) = (0, 0);
        for a in &inp.airports {
            let p = &a.p;
            let lb = s(p, "iata").filter(|v| !v.is_empty()).or(s(p, "name").filter(|v| !v.is_empty())).unwrap_or("");
            let ha = area_ha(&a.g);
            let mut props = AirportProps {
                id: p.get("id").cloned().unwrap_or(Value::Null), k: p.get("k").cloned().unwrap_or(Value::Null), lb: lb.to_string(), ha,
                n: nonempty(p, "name"), ne: nonempty(p, "name_en"), iata: nonempty(p, "iata"), icao: nonempty(p, "icao"), op: nonempty(p, "operator"), com: false,
            };
            if s(p, "k") == Some("aerodrome") {
                let mil = s(p, "military").map_or(false, |v| !v.is_empty()) || s(p, "atype").unwrap_or("").contains("military");
                let iata = s(p, "iata").map_or(false, |v| !v.is_empty());
                let g = geomx::from_geojson(&a.g, geomx::id);
                let commercial = iata && !mil && ha >= 300.0;
                if commercial {
                    props.com = true;
                    // one icon point per commercial airport, so tile boundaries cannot repeat it
                    if let Some(rp) = g.as_ref().and_then(geomx::rep_point) {
                        let m = AirportPtProps { id: p.get("id").cloned().unwrap_or(Value::Null), lb: lb.to_string() }.to_map();
                        feat(&mut airport_pts, &m, &pyjson::to_string(&geojson_value(&Geometry::Point(rp)), COMPACT), Some(8), None);
                        n_air_pts += 1;
                    }
                }
                feat(&mut airports, &props.to_map(), &a.graw, Some(if iata { 8 } else { 10 }), None);
                n_air += 1;
                if commercial {
                    if let Some(g) = g {
                        aero_geoms.push(geomx::make_valid(g));
                        aero_props.push(p);
                    }
                }
            } else {
                feat(&mut airports, &props.to_map(), &a.graw, Some(12), None);
                n_air += 1;
            }
        }
        let aero_tree = Index::new(aero_geoms.iter().map(geomx::bbox));
        let words = Regex::new(r"(?i)airport|a[ée]roport|flughafen|aeropuerto|机场|航站楼").unwrap();
        Airports { airports, airport_pts, n_air, n_air_pts, aero_geoms, aero_props, aero_tree, words }
    }

    /// the airport a station serves: the commercial airport within about 300 m (its IATA code, else its name), else "Airport" when the
    /// station's own name says airport
    pub fn of_station(&self, st: &Value) -> Option<String> {
        let c = &st["geometry"]["coordinates"];
        let pg = Geometry::Point(Point::new(c[0].as_f64().unwrap_or(0.0), c[1].as_f64().unwrap_or(0.0)));
        for i in self.aero_tree.near(&pg, BUFFER_DEG) {
            if geomx::dist(&self.aero_geoms[i], &pg) <= BUFFER_DEG {
                let p = self.aero_props[i];
                return Some(s(p, "iata").filter(|v| !v.is_empty()).or(s(p, "name").filter(|v| !v.is_empty())).unwrap_or("Airport").to_string());
            }
        }
        let pr = &st["properties"];
        let nm = [pr["name"].as_str(), pr["name_en"].as_str()].into_iter().flatten().filter(|v| !v.is_empty()).collect::<Vec<_>>().join(" ");
        self.words.is_match(&nm).then(|| "Airport".to_string())
    }
}
