//! The records of the JSON side files the app loads on demand, one struct per file, mirroring the interfaces in app/src/data.ts
//! (`LineIndexEntry`, `NamedLine`, `Shards`, `Highlight`, `StationRecord`, `LineRecord`). The field order is the key order of the file.
//! Fields that pass a value of the station model or the line list through untouched stay `Value` (null stays null).

use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// One entry of lines-index.json (keyed by line number): what the search and the line cards need without loading the line.
#[derive(Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub(super) struct LineIndexEntry<'a> {
    /// line name
    pub n: &'a Value,
    /// line ref (number or letter)
    pub r: &'a Value,
    /// line colour, normalised to `#rrggbb`
    pub c: Option<String>,
    /// rail type of the line
    pub t: &'a Value,
    /// operator
    pub o: &'a Value,
    /// first terminus
    pub f: &'a Value,
    /// last terminus
    pub to: &'a Value,
    /// length in km: the longest route
    pub km: f64,
    /// English name
    pub ne: Value,
}

/// One entry of named-lines.json: a named railway that no line runs on.
#[derive(Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub(super) struct NamedLine<'a> {
    /// name
    pub n: &'a str,
    /// English name
    pub ne: &'a Option<String>,
    /// rail type
    pub t: &'a str,
    /// length in km
    pub km: f64,
    /// number of ways
    pub ways: i64,
    /// bounding box `[west, south, east, north]`
    pub bbox: Vec<f64>,
}

/// shards.json: shard counts for the app, which hashes a key with fnv1a and takes it modulo the count.
#[derive(Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub(super) struct Shards {
    /// station records in `stations/`
    pub stations: u32,
    /// line records in `lines/`
    pub lines: u32,
    /// line numbers that are their own file in `lines-big/`
    pub lines_big: Vec<i64>,
    /// tag records in `tags/`
    pub tags: u32,
    /// name highlights in `name-hl/`
    #[serde(rename = "name-hl")]
    pub name_hl: u32,
}

/// The simplified geometry of a highlight (GeoJSON MultiLineString).
#[derive(Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub(super) struct HlGeom {
    /// always `MultiLineString`
    #[serde(rename = "type")]
    pub ty: &'static str,
    /// merged, simplified lines as `[lon, lat]`
    pub coordinates: Vec<Vec<[f64; 2]>>,
}

/// What a line or a named railway is drawn with when highlighted: its track pieces and a simplified geometry.
#[derive(Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub(super) struct Highlight<'a> {
    /// ids of its track pieces (a filter on the track layer, z9 and closer)
    pub ways: &'a [String],
    /// merged geometry, for the overview that has no per-piece ids; null when there is no track
    pub geom: Option<HlGeom>,
    /// rail type
    #[serde(rename = "type")]
    pub ty: Value,
    /// line colour, when it has one
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oc: Option<String>,
}

/// A station of the same complex, listed on the station card.
#[derive(Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub(super) struct ComplexOther<'a> {
    pub id: &'a Value,
    pub name: &'a Value,
    pub name_en: &'a Value,
    pub modes: &'a Value,
}

/// The complex a station belongs to (only written when it has other stations).
#[derive(Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub(super) struct ComplexRef<'a> {
    pub id: &'a Value,
    pub name: Value,
    pub others: Vec<ComplexOther<'a>>,
}

/// Tags of one source object of a station or line.
#[derive(Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub(super) struct SourceTags<'a> {
    /// OSM id (`node/1`, `relation/2`)
    pub id: &'a str,
    pub tags: Value,
}

/// A record of stations/: everything the station card shows.
#[derive(Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub(super) struct StationRecord<'a> {
    pub id: &'a Value,
    pub name: &'a Value,
    pub name_en: &'a Value,
    /// size rank 1 (largest) to 3
    pub tier: &'a Value,
    pub terminus: &'a Value,
    /// high-speed station: flagged by the model or served by a high-speed line
    pub hsr: bool,
    pub modes: &'a Value,
    /// mainline platform count
    pub platforms: &'a Value,
    pub platform_count: &'a Value,
    pub platform_refs: &'a Value,
    pub platform_refs_other: &'a Value,
    /// railway lines (route=railway relations) through the station
    pub rail_lines: Value,
    pub line_count: &'a Value,
    /// line numbers by rail type, most prominent type first
    pub lines: Map<String, Value>,
    pub complex: Option<ComplexRef<'a>>,
    /// station under construction
    pub construction: bool,
    /// the airport it serves (IATA code or name)
    pub airport: &'a Option<String>,
    /// OSM id of the first source object
    pub osm: Option<&'a str>,
    pub wikidata: &'a Value,
    /// `[lon, lat]`
    pub pt: &'a Value,
    /// Wikidata id of its operator, when a logo exists
    pub logo: Option<String>,
    /// raw tags of the source objects
    pub tags: Vec<SourceTags<'a>>,
    /// the station model's own numbers (score, tier, land_ha, ...)
    pub model: Map<String, Value>,
}

/// One route of a line record.
#[derive(Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub(super) struct RouteRecord<'a> {
    pub id: &'a str,
    pub name: &'a Value,
    pub from: &'a Value,
    pub to: &'a Value,
    /// length in km
    pub km: f64,
    /// station ids in order
    pub stations: Value,
}

/// The name of a station on a line record.
#[derive(Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub(super) struct StationName<'a> {
    pub n: &'a Value,
    pub ne: &'a Value,
}

/// A record of lines/ (or lines-big/): everything the line card shows.
#[derive(Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub(super) struct LineRecord<'a> {
    /// line number
    pub id: i64,
    pub name: &'a Value,
    #[serde(rename = "ref")]
    pub ref_: &'a Value,
    #[serde(rename = "type")]
    pub ty: &'a Value,
    pub colour: Option<String>,
    pub operator: &'a Value,
    pub network: &'a Value,
    pub from: &'a Value,
    pub to: &'a Value,
    /// length in km: the longest route
    pub km: Value,
    /// OSM id of the line relation
    pub osm: &'a str,
    /// the eight longest routes
    pub routes: &'a [RouteRecord<'a>],
    /// names of the stations on those routes, by station id
    pub station_names: BTreeMap<&'a str, StationName<'a>>,
    /// raw tags of the line relation
    pub tags: Value,
    pub name_en: Value,
    /// ids of its route relations (at most 30)
    pub route_ids: Vec<&'a Value>,
    pub hl: Highlight<'a>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cn12306::records::CnRecord;
    use crate::stats::StatsRow;
    use schemars::generate::SchemaSettings;
    use serde_json::json;

    /// shared/side-schema.json is the JSON Schema of every side-file record, generated from the serde structs above (and the stats and China ones).
    /// The app (src/data.ts, checked by src/sideSchema.test.ts) and app/scripts/check-data-contract.mjs read it.
    /// Each entry is a standalone schema (its `$ref`s resolve within the entry), so the file as a whole is not one JSON Schema document.
    /// Out of date: regenerate with `UPDATE_SCHEMA=1 cargo test --release side_schema_is_current`.
    #[test]
    fn side_schema_is_current() {
        let mut defs = Map::new();
        let mut add = |name: &str, schema: schemars::Schema| {
            defs.insert(name.to_string(), schema.to_value());
        };
        // a fresh generator per entry, so no entry carries another's definitions
        let gen = || SchemaSettings::draft2020_12().for_serialize().into_generator();
        add("station", gen().root_schema_for::<StationRecord>());
        add("line", gen().root_schema_for::<LineRecord>());
        add("line_index_entry", gen().root_schema_for::<LineIndexEntry>());
        add("named_line", gen().root_schema_for::<NamedLine>());
        add("highlight", gen().root_schema_for::<Highlight>());
        add("shards", gen().root_schema_for::<Shards>());
        add("stats_area", gen().root_schema_for::<StatsRow>());
        add("china_service_record", gen().root_schema_for::<CnRecord>());
        let want = serde_json::to_string_pretty(&json!({ "definitions": defs })).unwrap() + "\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../shared/side-schema.json");
        if std::env::var_os("UPDATE_SCHEMA").is_some() {
            std::fs::write(&path, &want).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(have == want, "shared/side-schema.json is out of date: regenerate it with `UPDATE_SCHEMA=1 cargo test --release side_schema_is_current`");
    }
}
