//! Config exemptions (docs/12-config-and-exemptions.md): parsing, matching and applying them to tracks and lines.

use super::*;

/// what one exemption matched
#[derive(Default)]
pub(super) struct ExStats {
    pub(super) tracks: u64,
    pub(super) lines: u64,
    pub(super) km: f64,
}

/// what an exemption does to the features it matches
pub(super) enum Action {
    Type(String),
    Freight(bool),
    Drop,
}

/// One entry of the config's `exemptions` list (docs/12-config-and-exemptions.md): a named, justified exception to the generic rules.
/// Applied after all the other rules, in file order (a later entry wins over an earlier one), to the track ways (every action) and to the lines
/// (route masters, else routes; `type` only).
pub struct Exemption {
    pub(super) id: String,
    pub(super) rule: Rule,
    pub(super) action: Action,
    /// `match.type`: only features that the rules above have typed this way
    pub(super) was_type: Option<String>,
    /// `match.type_source`: only features whose type came from this rule (`no-evidence`, `no-station`, `tags`, `route`, ...); tracks only
    pub(super) was_source: Option<String>,
    pub(super) expect_min: u64,
}

impl Exemption {
    pub(super) fn parse(x: &Value) -> Exemption {
        let o = x.as_object().unwrap_or_else(|| panic!("exemptions: each entry must be an object"));
        let id = o.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or_else(|| panic!("exemptions: every entry needs an id")).to_string();
        let bad = |what: &str| -> ! { panic!("exemption \"{id}\": {what}") };
        if o.get("reason").and_then(Value::as_str).is_none_or(|s| s.trim().is_empty()) {
            bad("reason is required (say why the generic rules are wrong here)");
        }
        for k in o.keys() {
            if !["id", "match", "action", "reason", "source", "added", "expect_min"].contains(&k.as_str()) {
                bad(&format!("unknown key {k}"));
            }
        }
        let m = o.get("match").and_then(Value::as_object).filter(|m| !m.is_empty()).unwrap_or_else(|| bad("match must be an object with at least one key"));
        let mut m = m.clone();
        let was_type = match m.remove("type") {
            None => None,
            Some(Value::String(t)) if ORDER.contains(&t.as_str()) || t == "freight" || t == "yard" => Some(t),
            Some(_) => bad("match.type must be a rail type (freight, regional, ...)"),
        };
        let was_source = match m.remove("type_source") {
            None => None,
            Some(Value::String(t)) if !t.is_empty() => Some(t),
            Some(_) => bad("match.type_source must be a rule name (no-evidence, no-station, tags, route, ...)"),
        };
        if m.is_empty() {
            bad("match.type and match.type_source are conditions, not a selector: add name_re, operator_re, ref, network_re or osm_id");
        }
        let rule = Rule::parse(&Value::Object(m));
        for i in rule.osm_id.iter().flatten() {
            let ok = i.split_once('/').is_some_and(|(k, n)| matches!(k, "way" | "relation" | "node") && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
            if !ok {
                bad(&format!("osm id {i:?} must look like way/123 or relation/456"));
            }
        }
        let a = o.get("action").and_then(Value::as_object).unwrap_or_else(|| bad("action must be an object"));
        let action = match (a.len(), a.get("type"), a.get("freight"), a.get("drop")) {
            (1, Some(Value::String(t)), None, None) if ORDER.contains(&t.as_str()) || t == "freight" => Action::Type(t.clone()),
            (1, None, Some(Value::Bool(b)), None) => Action::Freight(*b),
            (1, None, None, Some(Value::Bool(true))) => Action::Drop,
            _ => bad("action must be exactly one of {\"type\": <rail type>}, {\"freight\": true|false}, {\"drop\": true}"),
        };
        let expect_min = match o.get("expect_min") {
            None => 1,
            Some(v) => v.as_u64().unwrap_or_else(|| bad("expect_min must be a whole number")),
        };
        Exemption { id, rule, action, was_type, was_source, expect_min }
    }
}

/// Exemptions, applied last so they win over everything above. Returns which tracks they drop.
pub(super) fn apply_exemptions(tracks: &[TrackIn], cfg: &Cfg, decided: &mut [Option<Decided>], ex_stats: &mut [ExStats]) -> Vec<bool> {
    let mut dropped = vec![false; tracks.len()];
    if cfg.exemptions.is_empty() {
        return dropped;
    }
    for (ti, tr) in tracks.iter().enumerate() {
        let Some(d) = decided[ti].as_mut() else { continue };
        for (xi, x) in cfg.exemptions.iter().enumerate() {
            if x.was_type.as_deref().is_some_and(|w| w != d.t) || x.was_source.as_deref().is_some_and(|w| w != d.s) || !x.rule.matches(&tr.props) {
                continue;
            }
            ex_stats[xi].tracks += 1;
            ex_stats[xi].km += ll_km(&tr.ll);
            dropped[ti] = false;
            match &x.action {
                Action::Type(ty) => (d.t, d.s) = (ty.clone(), "exemption"),
                Action::Freight(true) => (d.t, d.s) = ("freight".into(), "exemption"),
                Action::Freight(false) => {
                    if d.t == "freight" {
                        // back to what its own tags say, or Regional when those say freight too
                        d.t = d.own.clone().filter(|o| o != "freight").unwrap_or_else(|| "regional".into());
                        d.s = "exemption";
                    }
                }
                Action::Drop => dropped[ti] = true,
            }
        }
    }
    dropped
}
