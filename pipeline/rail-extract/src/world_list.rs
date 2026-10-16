//! `rail-extract world-list`: which countries of a world cut have passenger rail. Reads <data>/<slug>/features.typed.geojsonl for every
//! slug and writes <data>/kept.txt and <data>/dropped.txt (one slug per line, in the order given). The rule: at least `--min-km`
//! (default 1) of open track typed anything but freight or yard. Way length, so double track counts twice; construction and proposed
//! track do not count.

use crate::tiles::haversine_len;
use rayon::prelude::*;
use serde_json::Value;
use std::path::Path;

/// kilometres of open passenger track in one typed feature line (0 for anything else: stations, freight, yard, construction, proposed)
pub fn passenger_km(line: &str) -> f64 {
    let Ok(f) = serde_json::from_str::<Value>(line) else { return 0.0 };
    let (p, g) = (&f["properties"], &f["geometry"]);
    let Some(kind) = p["type"].as_str() else { return 0.0 };
    if matches!(kind, "freight" | "yard") || matches!(p["railway"].as_str(), Some("construction" | "proposed")) {
        return 0.0;
    }
    let pts = |c: &Value| -> Vec<(f64, f64)> { c.as_array().into_iter().flatten().filter_map(|x| Some((x[0].as_f64()?, x[1].as_f64()?))).collect() };
    match g["type"].as_str() {
        Some("LineString") => haversine_len(&pts(&g["coordinates"])),
        Some("MultiLineString") => g["coordinates"].as_array().into_iter().flatten().map(|l| haversine_len(&pts(l))).sum(),
        _ => 0.0,
    }
}

pub fn run(data: &Path, slugs: &[String], min_km: f64) {
    let km: Vec<f64> = slugs
        .par_iter()
        .map(|s| std::fs::read_to_string(data.join(s).join("features.typed.geojsonl")).map_or(0.0, |t| t.lines().map(passenger_km).sum()))
        .collect();
    let (mut kept, mut dropped) = (String::new(), String::new());
    for (s, k) in slugs.iter().zip(&km) {
        let list = if *k >= min_km { &mut kept } else { &mut dropped };
        list.push_str(s);
        list.push('\n');
    }
    std::fs::write(data.join("kept.txt"), &kept).expect("write kept.txt");
    std::fs::write(data.join("dropped.txt"), &dropped).expect("write dropped.txt");
    eprintln!("world-list: {} kept, {} dropped (>= {min_km} km of open passenger track) -> {}/{{kept,dropped}}.txt", kept.lines().count(), dropped.lines().count(), data.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: &str, railway: &str, geom: &str) -> String {
        format!(r#"{{"type":"Feature","properties":{{"railway":"{railway}","type":"{kind}"}},"geometry":{geom}}}"#)
    }
    const ONE_DEGREE: &str = r#"{"type":"LineString","coordinates":[[0,0],[0,1]]}"#; // 111.2 km

    #[test]
    fn counts_only_open_passenger_track() {
        assert!((passenger_km(&line("metro", "rail", ONE_DEGREE)) - 111.2).abs() < 0.1);
        assert_eq!(passenger_km(&line("freight", "rail", ONE_DEGREE)), 0.0);
        assert_eq!(passenger_km(&line("yard", "rail", ONE_DEGREE)), 0.0);
        assert_eq!(passenger_km(&line("regional", "construction", ONE_DEGREE)), 0.0);
        assert_eq!(passenger_km(&line("regional", "proposed", ONE_DEGREE)), 0.0);
        assert_eq!(passenger_km(&line("regional", "rail", r#"{"type":"Point","coordinates":[0,0]}"#)), 0.0);
        let multi = r#"{"type":"MultiLineString","coordinates":[[[0,0],[0,1]],[[0,1],[0,2]]]}"#;
        assert!((passenger_km(&line("intercity", "rail", multi)) - 222.4).abs() < 0.2);
        assert_eq!(passenger_km("not json"), 0.0);
    }
}
