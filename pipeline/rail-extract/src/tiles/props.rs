//! The properties of every tile feature, one struct per layer, so each key name is written once and says what the app uses it for.
//! `to_map` writes the keys in the order of the fields (serde_json preserve_order), which is the order of the tile input files.
//! Flags (`con`, `prop`, `tun`, ...) are written as the number 1 and left out when false; options are left out when `None`.
//! The key names of every layer are also written to shared/tile-schema.json (test `tile_schema_is_current`), which the app checks its style against.

use serde::{Serialize, Serializer};
use serde_json::{Map, Value};

/// Any props struct as the ordered JSON object `feat` writes.
pub(super) trait ToMap: Serialize {
    fn to_map(&self) -> Map<String, Value> {
        match serde_json::to_value(self).expect("tile props serialize") {
            Value::Object(m) => m,
            _ => unreachable!("tile props are structs"),
        }
    }
}

impl<T: Serialize> ToMap for T {}

fn is_false(b: &bool) -> bool {
    !*b
}

/// a flag is the number 1 (and is left out when false)
fn one<S: Serializer>(_: &bool, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_i64(1)
}

/// a flag that is always written, as 0 or 1
fn as_int<S: Serializer>(b: &bool, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_i64(*b as i64)
}

/// Layer `track` (one feature per OSM way at track_minzoom and deeper).
/// `n` and `n_rwy` both serialise as `n`: only `to_map()` (the later one wins) may serialise a `TrackProps`, never `serde_json::to_string`.
#[derive(Serialize)]
pub(super) struct TrackProps {
    /// `id`: OSM id (`way/123`). The key of feature state (hover, selection), of the Inspect lookup and of the line highlights.
    pub id: String,
    /// `type`: rail type (high_speed, intercity, ..., yard). Colour, width, sort order, the type filters and the tooltip title.
    #[serde(rename = "type")]
    pub ty: String,
    /// `usage`: OSM usage tag (main, branch, ...). The tooltip's rail type label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<String>,
    /// `el`: OSM electrified tag. The tooltip's Electrified row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub el: Option<String>,
    /// `v`: OSM voltage tag. The tooltip's Electrified row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub v: Option<String>,
    /// `ms`: OSM maxspeed tag as mapped. The tooltip's Max speed row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms: Option<String>,
    /// `g`: OSM gauge tag. The tooltip's Gauge row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub g: Option<String>,
    /// `tr`: OSM tracks tag. Not read by the app yet; kept in the tile input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tr: Option<String>,
    /// `n`: the way's own name. The track name label (`lbl-track`, only where the piece has no line), the railway lookup and the tooltip title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<String>,
    /// `fq`: OSM frequency tag. The tooltip's Electrified row (traction frequency).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fq: Option<String>,
    /// `kmh`: maxspeed in km/h as a number, when parseable. The speed colour mode and the speed band filters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kmh: Option<i64>,
    /// `n` again, for track with no name of its own that takes the name of its railway relation (LGV Sud-Est). Written after `kmh`.
    #[serde(rename = "n", skip_serializing_if = "Option::is_none")]
    pub n_rwy: Option<String>,
    /// `ne`: English name of `n`. The tooltip's second name line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ne: Option<String>,
    /// `sv`: OSM service tag (siding, yard, ...). Marks yard and service track in the tooltip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv: Option<String>,
    /// `tun`: 1 for a tunnel. The track opacity (paler from z13).
    #[serde(serialize_with = "one", skip_serializing_if = "is_false")]
    pub tun: bool,
    /// `br`: 1 for a bridge. Not read by the app yet; kept in the tile input.
    #[serde(serialize_with = "one", skip_serializing_if = "is_false")]
    pub br: bool,
    /// `con`: 1 for track under construction (or proposed). Hover and click only; the drawing is `con_dash`.
    #[serde(serialize_with = "one", skip_serializing_if = "is_false")]
    pub con: bool,
    /// `prop`: 1 for proposed track. The Proposed filter and the tooltip.
    #[serde(serialize_with = "one", skip_serializing_if = "is_false")]
    pub prop: bool,
    /// `ln`: comma-separated line ids (OSM relation numbers) of the services on this way, most prominent type first. A click opens the first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ln: Option<String>,
    /// `nl`: number of lines on this way. Its presence hides the track's own label and the track itself from z12 (the strands draw it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nl: Option<usize>,
    /// `oc`: colour of the first line that has one. The operator colour mode below z12.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oc: Option<String>,
}

impl TrackProps {
    /// the way's name, own or its railway's
    pub fn name(&self) -> Option<&str> {
        self.n.as_deref().or(self.n_rwy.as_deref())
    }
}

/// Layer `track`, zoomed-out overview (z0 to 8): merged pieces keyed by these attributes alone, so equal neighbours merge into long lines.
#[derive(Serialize)]
pub(super) struct OverviewProps {
    /// `type`: rail type. Colour, width and the type filters.
    #[serde(rename = "type")]
    pub ty: String,
    /// `con`: 1 for construction. The Construction filter.
    #[serde(serialize_with = "one", skip_serializing_if = "is_false")]
    pub con: bool,
    /// `prop`: 1 for proposed. The Proposed filter.
    #[serde(serialize_with = "one", skip_serializing_if = "is_false")]
    pub prop: bool,
    /// `oc`: line colour. The operator colour mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oc: Option<String>,
    /// `ln`: line ids on the piece. A click opens the first line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ln: Option<String>,
    /// `kmh`: lower bound of the speed band (not the speed itself), so pieces of one band merge. The speed colour mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kmh: Option<i64>,
    /// `nk`: railway number into railways.json. The hover names the railway, not the services.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nk: Option<usize>,
}

impl OverviewProps {
    /// the metro sketch of z4 to 6: the type only, so it merges into a few long lines
    pub fn sketch_metro() -> Self {
        OverviewProps { ty: "metro".into(), con: false, prop: false, oc: None, ln: None, kmh: None, nk: None }
    }
}

/// Layer `con_dash`: the dashes of construction and proposed track, cut as geometry (no ids), one copy per zoom band.
#[derive(Serialize)]
pub(super) struct ConDashProps {
    /// `type`: rail type. Colour and the type filters.
    #[serde(rename = "type")]
    pub ty: String,
    /// `prop`: 1 for proposed. Separate layer (Proposed switch).
    #[serde(serialize_with = "one", skip_serializing_if = "is_false")]
    pub prop: bool,
    /// `kmh`: lower bound of the speed band. The speed colour mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kmh: Option<i64>,
    /// `b`: zoom band (see shared/constants.json `con_band`). The app shows one band's layer per zoom.
    pub b: i64,
}

/// Layer `lines`: one strand per line per way (z11 and deeper), drawn side by side where lines share track.
#[derive(Serialize)]
pub(super) struct StrandProps {
    /// `lid`: line id (OSM relation number). Hover and selection of a line, the click that opens it.
    pub lid: i64,
    /// `t`: the line's rail type. The type filters and the strand width.
    pub t: Option<String>,
    /// `i`: position of this strand among the lines on the way. Its offset across the track.
    pub i: usize,
    /// `n`: number of lines on the way. The offsets and the shrinking of crowded track.
    pub n: usize,
    /// `lb`: short label of the line (name before the first colon, else ref). The text along the strand.
    pub lb: String,
    /// `lbe`: English label. The text when the map shows English names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lbe: Option<String>,
    /// `rf`: the line's number for its badge (2, S5, M). The badge symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rf: Option<String>,
    /// `rl`: 1 when the line colour is light, so the badge uses dark text.
    #[serde(serialize_with = "one", skip_serializing_if = "is_false")]
    pub rl: bool,
    /// `c`: the line's colour. The operator colour mode and the badge background.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c: Option<String>,
}

/// Layers `areas`, `buildings` and `platforms` (polygons): railway land, yards, station buildings, platforms.
#[derive(Serialize)]
pub(super) struct AreaProps {
    /// `id`: OSM id. The Inspect lookup.
    pub id: String,
    /// `ha`: area in hectares. The tooltip.
    pub ha: f64,
    /// `n`: name. The tooltip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<String>,
    /// `op`: operator. The tooltip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
    /// `k`: kind (land, yard, building, platform). Picks the style layer and the tooltip title.
    pub k: &'static str,
    /// `ref`: platform ref. The platform label (platforms only).
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub platform_ref: Option<String>,
}

/// Layer `platforms` (a platform mapped as a line).
#[derive(Serialize)]
pub(super) struct PlatformLineProps {
    /// `id`: OSM id. The Inspect lookup.
    pub id: String,
    /// `ref`: platform ref. The platform label.
    #[serde(rename = "ref")]
    pub platform_ref: String,
    /// `k`: kind, always `platform`. Picks the style layer, as for the polygons.
    pub k: &'static str,
}

/// Layer `airports`: the outline of an aerodrome or terminal.
#[derive(Serialize)]
pub(super) struct AirportProps {
    /// `id`: OSM id. The Inspect lookup.
    pub id: Value,
    /// `k`: kind (aerodrome, terminal). Picks the style layer.
    pub k: Value,
    /// `lb`: IATA code, else name. The airport label.
    pub lb: String,
    /// `ha`: area in hectares. The tooltip.
    pub ha: f64,
    /// `n`: name. The tooltip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<String>,
    /// `ne`: English name. The tooltip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ne: Option<String>,
    /// `iata`: IATA code. The tooltip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iata: Option<String>,
    /// `icao`: ICAO code. The tooltip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icao: Option<String>,
    /// `op`: operator. The tooltip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
    /// `com`: 1 for a commercial airport (IATA code, not military, 300 ha or more). The airport style layers.
    #[serde(serialize_with = "one", skip_serializing_if = "is_false")]
    pub com: bool,
}

/// Layer `airport_pts`: one icon point per commercial airport, so tile edges cannot repeat it.
#[derive(Serialize)]
pub(super) struct AirportPtProps {
    /// `id`: OSM id of the airport.
    pub id: Value,
    /// `lb`: IATA code, else name. The label under the icon.
    pub lb: String,
}

/// Layer `stations`: one point per station (stop area or station).
#[derive(Serialize)]
pub(super) struct StationProps {
    /// `id`: station id. Feature state, selection and the side file lookup.
    pub id: Value,
    /// `cx`: id of the station's complex. Highlights the other stations of the complex.
    pub cx: Value,
    /// `n`: name. Label and tooltip.
    pub n: String,
    /// `ne`: English name. Label and tooltip when English names are on.
    pub ne: String,
    /// `tier`: size rank 1 (largest) to 3, from physical size only. Zoom where it appears, ring size, label priority.
    pub tier: i64,
    /// `term`: 1 for a terminus. The ring style.
    #[serde(serialize_with = "as_int")]
    pub term: bool,
    /// `hsr`: 1 for a high-speed station. The ring colour and the HSR only switch.
    #[serde(serialize_with = "as_int")]
    pub hsr: bool,
    /// `mp`: mainline platform count. The tooltip.
    pub mp: Value,
    /// `pc`: platform count. The tooltip.
    pub pc: Value,
    /// `lc`: line count. The tooltip.
    pub lc: Value,
    /// `lct`: line counts by type (`metro:3,regional:2`). The tooltip's line breakdown.
    pub lct: String,
    /// `sb`: `,`-wrapped speed band indexes of the track beside it (9 = none mapped). Hides the station with its hidden speed bands.
    pub sb: String,
    /// `ty`: `,`-wrapped rail types serving it. Hides the station with its hidden types.
    pub ty: String,
    /// `modes`: OSM public transport modes, comma-separated. The tooltip.
    pub modes: String,
    /// `lg`: Wikidata id of its operator, when a logo exists. The operator logo symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lg: Option<String>,
    /// `ap`: airport it serves (IATA code or name). The tooltip's Airport row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ap: Option<String>,
    /// `con`: 1 for a station under construction. Dashed ring, the Construction filter and the tooltip.
    #[serde(serialize_with = "one", skip_serializing_if = "is_false")]
    pub con: bool,
    /// `fr`: 1 for a freight or yard station (no passenger service or track). The tooltip.
    #[serde(serialize_with = "one", skip_serializing_if = "is_false")]
    pub fr: bool,
}

/// Layer `complexes`: one point per station complex, shown where its stations are still too small to show one by one.
#[derive(Serialize)]
pub(super) struct ComplexProps {
    /// `id`: complex id. Selection.
    pub id: Value,
    /// `n`: name. The label.
    pub n: String,
    /// `ne`: English name. The label when English names are on.
    pub ne: String,
    /// `tier`: size rank 1 (largest) to 3. Zoom where it appears and label priority.
    pub tier: i64,
    /// `term`: 1 for a terminus. The ring style.
    #[serde(serialize_with = "as_int")]
    pub term: bool,
    /// `ns`: number of stations in the complex. The tooltip.
    pub ns: Value,
    /// `con`: 1 when every station of the complex is being built. Dashed ring.
    #[serde(serialize_with = "one", skip_serializing_if = "is_false")]
    pub con: bool,
    /// `sb`: `,`-wrapped speed band indexes of its stations (9 = none mapped). Hides it with its hidden speed bands.
    pub sb: String,
    /// `ty`: `,`-wrapped rail types of its stations. Hides it with its hidden types.
    pub ty: String,
}

/// Layer `cities`: one dot per city with a metro, tram or monorail system, visible from far out.
#[derive(Serialize)]
pub(super) struct CityProps {
    /// `m`: metro km. The dot's colour share.
    pub m: i64,
    /// `l`: tram km. The dot's colour share.
    pub l: i64,
    /// `r`: monorail km. The dot's colour share.
    pub r: i64,
    /// `n`: all km. The dot's size.
    pub n: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiles::TILE_LAYERS;
    use serde_json::json;

    /// the keys of a props struct with every optional field set (the literals are exhaustive, so a new field stops the build here)
    fn keys<T: Serialize>(full: T) -> Vec<String> {
        full.to_map().keys().cloned().collect()
    }

    /// keys of several structs of one layer, first-seen order
    fn union(parts: Vec<Vec<String>>) -> Vec<String> {
        let mut all: Vec<String> = Vec::new();
        for k in parts.into_iter().flatten() {
            if !all.contains(&k) {
                all.push(k);
            }
        }
        all
    }

    fn s() -> Option<String> {
        Some("x".into())
    }

    fn area() -> AreaProps {
        AreaProps { id: "way/1".into(), ha: 1.0, n: s(), op: s(), k: "land", platform_ref: s() }
    }

    fn layer_keys(layer: &str) -> Vec<String> {
        match layer {
            "track" => union(vec![
                keys(TrackProps {
                    id: "way/1".into(),
                    ty: "metro".into(),
                    usage: s(),
                    el: s(),
                    v: s(),
                    ms: s(),
                    g: s(),
                    tr: s(),
                    n: s(),
                    fq: s(),
                    kmh: Some(1),
                    n_rwy: s(),
                    ne: s(),
                    sv: s(),
                    tun: true,
                    br: true,
                    con: true,
                    prop: true,
                    ln: s(),
                    nl: Some(1),
                    oc: s(),
                }),
                keys(OverviewProps { ty: "metro".into(), con: true, prop: true, oc: s(), ln: s(), kmh: Some(1), nk: Some(1) }),
            ]),
            "con_dash" => keys(ConDashProps { ty: "metro".into(), prop: true, kmh: Some(1), b: 1 }),
            "lines" => keys(StrandProps { lid: 1, t: s(), i: 0, n: 1, lb: "x".into(), lbe: s(), rf: s(), rl: true, c: s() }),
            "areas" | "buildings" => keys(area()),
            "platforms" => union(vec![keys(area()), keys(PlatformLineProps { id: "way/1".into(), platform_ref: "1".into(), k: "platform" })]),
            "stations" => keys(StationProps {
                id: json!("s"),
                cx: json!("c"),
                n: "x".into(),
                ne: "x".into(),
                tier: 1,
                term: true,
                hsr: true,
                mp: json!(1),
                pc: json!(1),
                lc: json!(1),
                lct: "x".into(),
                sb: "x".into(),
                ty: "x".into(),
                modes: "x".into(),
                lg: s(),
                ap: s(),
                con: true,
                fr: true,
            }),
            "complexes" => keys(ComplexProps {
                id: json!("c"),
                n: "x".into(),
                ne: "x".into(),
                tier: 1,
                term: true,
                ns: json!(1),
                con: true,
                sb: "x".into(),
                ty: "x".into(),
            }),
            "airports" => {
                keys(AirportProps { id: json!("a"), k: json!("aerodrome"), lb: "x".into(), ha: 1.0, n: s(), ne: s(), iata: s(), icao: s(), op: s(), com: true })
            }
            "airport_pts" => keys(AirportPtProps { id: json!("a"), lb: "x".into() }),
            "cities" => keys(CityProps { m: 1, l: 1, r: 1, n: 1 }),
            other => panic!("tile layer {other} has no props struct: add it to layer_keys and shared/tile-schema.json"),
        }
    }

    /// shared/tile-schema.json lists the properties of every tile layer; the app checks its style against it.
    /// Out of date: regenerate with `UPDATE_SCHEMA=1 cargo test --release tile_schema_is_current`.
    #[test]
    fn tile_schema_is_current() {
        let mut layers = Map::new();
        for (layer, _) in TILE_LAYERS {
            layers.entry(layer.to_string()).or_insert_with(|| json!(layer_keys(layer)));
        }
        let want = serde_json::to_string_pretty(&json!({ "layers": layers })).unwrap() + "\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../shared/tile-schema.json");
        if std::env::var_os("UPDATE_SCHEMA").is_some() {
            std::fs::write(&path, &want).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(have == want, "shared/tile-schema.json is out of date: regenerate it with `UPDATE_SCHEMA=1 cargo test --release tile_schema_is_current`");
    }
}
