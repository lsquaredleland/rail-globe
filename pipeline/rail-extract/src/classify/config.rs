//! The per-region config (`Cfg`, pipeline/config/<slug>.json) and its match rules.

use super::*;

/// every key of the rule must match
pub(super) struct Rule {
    pub(super) network: Option<String>,
    pub(super) network_re: Option<Regex>,
    pub(super) name_re: Option<Regex>,
    pub(super) operator_re: Option<Regex>,
    pub(super) osm_id: Option<Vec<String>>,
    pub(super) ref_: Option<String>,
}

impl Rule {
    pub(super) fn parse(v: &Value) -> Rule {
        let mut r = Rule { network: None, network_re: None, name_re: None, operator_re: None, osm_id: None, ref_: None };
        let re = |x: &Value| Regex::new(x.as_str().expect("pattern")).unwrap_or_else(|e| panic!("bad pattern in config: {e}"));
        for (k, x) in v.as_object().expect("match rule") {
            match k.as_str() {
                "network" => r.network = Some(x.as_str().expect("network").to_string()),
                "network_re" => r.network_re = Some(re(x)),
                "name_re" => r.name_re = Some(re(x)),
                "operator_re" => r.operator_re = Some(re(x)),
                "ref" => r.ref_ = Some(x.as_str().expect("ref").to_string()),
                "osm_id" | "osm_ids" => {
                    r.osm_id = Some(match x {
                        Value::String(s) => vec![s.clone()],
                        Value::Array(a) => a.iter().filter_map(|s| s.as_str().map(String::from)).collect(),
                        _ => panic!("osm_id must be a string or a list"),
                    })
                }
                other => panic!("unknown match key {other} in config"),
            }
        }
        r
    }
    pub(super) fn matches<T: Get + ?Sized>(&self, t: &T) -> bool {
        (self.network.as_deref().is_none_or(|n| t.g("network") == Some(n)))
            && self.network_re.as_ref().is_none_or(|r| r.is_match(t.g("network").unwrap_or("")))
            && self.name_re.as_ref().is_none_or(|r| r.is_match(t.g("name").unwrap_or("")))
            && self.operator_re.as_ref().is_none_or(|r| r.is_match(t.g("operator").unwrap_or("")))
            && self.ref_.as_deref().is_none_or(|n| t.g("ref") == Some(n))
            && self.osm_id.as_ref().is_none_or(|ids| t.g("osm_id").is_some_and(|o| ids.iter().any(|i| i == o)))
    }
}

/// what `passenger_needs_evidence` counts as evidence of passenger service for main-line track typed from its tags alone
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Evidence {
    /// a passenger route relation, track beside it, or a passenger operator (`true` or `"routes"`): where mappers map the services
    Routes,
    /// ... or a passenger station within PAX_NEAR_M of the way (`"stations"`): where route relations are thin (Tanzania, Egypt, Sudan)
    Stations,
}

/// pipeline/config/<slug>.json
pub struct Cfg {
    pub(super) exemptions: Vec<Exemption>,
    pub(super) china: bool,
    /// `high_speed_min_kmh`: maxspeed from which untagged-by-name track is High-speed (default 200; China 250)
    pub(super) hs_min_kmh: f64,
    pub(super) type_overrides: Vec<(Rule, String)>,
    pub(super) track_type_overrides: Vec<(Rule, String)>,
    pub(super) hs_track_overrides: Vec<Rule>,
    pub(super) hs_networks: Vec<Regex>,
    pub(super) unrouted_main_type: String,
    pub(super) proposed: Option<Region>,
    /// names of the stations that carry passengers (China: the 12306 ticketing list); None: judge a station by its tags
    pub(super) passenger_names: Option<std::collections::HashSet<String>>,
    /// `passenger_needs_evidence` (docs/13-freight-north-america.md): unrouted main-line track typed from its tags alone is Freight unless it
    /// runs beside passenger-routed track or its operator is one of `passenger_operator_re`
    pub(super) needs_evidence: Option<Evidence>,
    pub(super) passenger_operators: Vec<Regex>,
}

impl Cfg {
    /// Is this station (its name, and whether its tags say passengers) a passenger station? With a passenger list the list decides;
    /// without one the tags do.
    /// `passenger_needs_evidence: "stations"`: a bare railway=station or halt counts as a passenger station (in these countries stations are
    /// mapped without `public_transport` or `train` tags, so the tags would find almost none)
    pub fn bare_stations_count(&self) -> bool {
        self.needs_evidence == Some(Evidence::Stations)
    }

    pub fn passenger_station(&self, name: Option<&str>, tags_say_passengers: bool) -> bool {
        match &self.passenger_names {
            Some(names) => name.is_some_and(|n| names.contains(n.trim().trim_end_matches('站'))),
            None => tags_say_passengers,
        }
    }

    pub fn load(path: &Path, regions_dir: &Path) -> Cfg {
        let v = Value::Object(crate::config::load_file(path));
        let overrides = |k: &str| -> Vec<(Rule, String)> {
            v.get(k)
                .and_then(Value::as_array)
                .map_or(Vec::new(), |a| a.iter().map(|o| (Rule::parse(&o["match"]), o["type"].as_str().expect("override type").to_string())).collect())
        };
        // proposed track is only kept inside this area: a Nominatim dump ([{..., "geojson": geometry}]) in pipeline/regions/
        let proposed = v.get("proposed_region").and_then(Value::as_str).filter(|s| !s.is_empty()).map(|r| {
            let p = regions_dir.join(r);
            let d: Value =
                serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))).expect("parse proposed_region");
            Region::new(r, crate::region::rings_of(&d[0]["geojson"]))
        });
        let passenger_names = v.get("passenger_stations_file").and_then(Value::as_str).map(|f| {
            let p = path.parent().unwrap_or(Path::new(".")).join(f);
            std::fs::read_to_string(&p)
                .unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        });
        let exemptions: Vec<Exemption> = v.get("exemptions").and_then(Value::as_array).map_or(Vec::new(), |a| a.iter().map(Exemption::parse).collect());
        for (i, x) in exemptions.iter().enumerate() {
            if exemptions[..i].iter().any(|y| y.id == x.id) {
                panic!("exemptions: id \"{}\" is used twice in {}", x.id, path.display());
            }
        }
        Cfg {
            exemptions,
            passenger_names,
            china: v.get("china").is_some_and(crate::config::truthy),
            hs_min_kmh: v.get("high_speed_min_kmh").and_then(Value::as_f64).unwrap_or(200.0),
            type_overrides: overrides("type_overrides"),
            track_type_overrides: overrides("track_type_overrides"),
            hs_track_overrides: v.get("high_speed_track_overrides").and_then(Value::as_array).map_or(Vec::new(), |a| a.iter().map(Rule::parse).collect()),
            hs_networks: v
                .get("high_speed_networks_re")
                .and_then(Value::as_array)
                .map_or(Vec::new(), |a| a.iter().map(|p| Regex::new(p.as_str().expect("pattern")).expect("high_speed_networks_re")).collect()),
            needs_evidence: match v.get("passenger_needs_evidence") {
                Some(Value::String(m)) if m == "stations" => Some(Evidence::Stations),
                Some(Value::String(m)) if m == "routes" => Some(Evidence::Routes),
                Some(Value::String(m)) => panic!("passenger_needs_evidence must be true, \"routes\" or \"stations\", not {m:?}"),
                Some(x) if crate::config::truthy(x) => Some(Evidence::Routes),
                _ => None,
            },
            passenger_operators: v
                .get("passenger_operator_re")
                .and_then(Value::as_array)
                .map_or(Vec::new(), |a| a.iter().map(|p| Regex::new(p.as_str().expect("pattern")).expect("passenger_operator_re")).collect()),
            unrouted_main_type: v.get("unrouted_main_type").and_then(Value::as_str).unwrap_or("intercity").to_string(),
            proposed,
        }
    }
}
