//! A track's type from its own tags alone.

use super::*;

/// track type from its own tags only
pub(super) fn own_tag_type<T: Get + ?Sized>(p: &T, cfg: &Cfg) -> (String, &'static str) {
    let mut rw = p.g("railway").map(str::to_string);
    if matches!(rw.as_deref(), Some("construction" | "proposed")) {
        // the mode lives in construction=* / proposed=*; mappers also put it under the other key
        let mode =
            p.g(rw.as_deref().unwrap()).or_else(|| p.g(if rw.as_deref() == Some("construction") { "proposed" } else { "construction" })).map(str::to_string);
        rw = match mode {
            Some(m) => Some(m),
            None => {
                let truthy = |k: &str| p.g(k).is_some_and(|v| !v.is_empty());
                if !(truthy("usage") || truthy("electrified") || truthy("gauge")) {
                    return ("other".into(), "default");
                }
                Some("rail".into())
            }
        };
    }
    let rw = rw.unwrap_or_default();
    if let Some(t) = railway_type(&rw) {
        return (t.into(), "tags");
    }
    if rw != "rail" && rw != "narrow_gauge" {
        return ("other".into(), "default");
    }
    if p.g("usage") == Some("tourism") {
        return ("other".into(), "tags");
    }
    let ms = speed_kmh(p.g("maxspeed"));
    let name = p.g("name").unwrap_or("");
    if cfg.china {
        if cfg.hs_track_overrides.iter().any(|r| r.matches(p)) {
            return ("high_speed".into(), "override");
        }
        let lower = name.to_lowercase();
        let hs_name = name.contains("高速铁路")
            || name.contains("高速鐵路")
            || lower.contains("high speed rail")
            || lower.contains("high-speed rail")
            || lower.contains("express rail link");
        if at_least(ms, cfg.hs_min_kmh) || name.contains("高速线") || name.contains("客专") || name.contains("客运专线") || hs_name {
            return ("high_speed".into(), "tags");
        }
        if p.g("highspeed") == Some("yes") || name.contains("城际") || at_least(ms, 200.0) {
            return ("intercity".into(), "tags");
        }
    } else if p.g("highspeed") == Some("yes") || at_least(ms, cfg.hs_min_kmh) {
        return ("high_speed".into(), "tags");
    }
    let usage = p.g("usage");
    if usage.is_some_and(|u| FREIGHT_USAGE.contains(&u)) || p.g("railway:traffic_mode") == Some("freight") {
        return ("freight".into(), "tags");
    }
    if usage == Some("main") {
        return (cfg.unrouted_main_type.clone(), "tags");
    }
    if usage == Some("branch") {
        return ("regional".into(), "tags");
    }
    // narrow gauge with no usage: nothing says it carries the public (mine, peat, park and miniature railways are mapped this way too)
    (if rw == "narrow_gauge" { "other" } else { "regional" }.into(), "default")
}
