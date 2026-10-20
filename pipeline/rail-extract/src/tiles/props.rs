//! The properties of every tile feature, one struct per layer, so each key name is written once and says what the app uses it for.
//! `to_map` writes the keys in the order of the fields (serde_json preserve_order), which is the order of the tile input files.
//! Flags (`con`, `prop`, `tun`, ...) are written as the number 1 and left out when false; options are left out when `None`.

use crate::pyjson::f;
use serde_json::{json, Map, Value};

fn opt(m: &mut Map<String, Value>, key: &str, v: &Option<String>) {
    if let Some(v) = v {
        m.insert(key.into(), json!(v));
    }
}

fn flag(m: &mut Map<String, Value>, key: &str, on: bool) {
    if on {
        m.insert(key.into(), json!(1));
    }
}

/// Layer `track` (one feature per OSM way at track_minzoom and deeper).
pub(super) struct TrackProps {
    /// `id`: OSM id (`way/123`). The key of feature state (hover, selection), of the Inspect lookup and of the line highlights.
    pub id: String,
    /// `type`: rail type (high_speed, intercity, ..., yard). Colour, width, sort order, the type filters and the tooltip title.
    pub ty: String,
    /// `usage`: OSM usage tag (main, branch, ...). The tooltip's rail type label.
    pub usage: Option<String>,
    /// `el`: OSM electrified tag. The tooltip's Electrified row.
    pub el: Option<String>,
    /// `v`: OSM voltage tag. The tooltip's Electrified row.
    pub v: Option<String>,
    /// `ms`: OSM maxspeed tag as mapped. The tooltip's Max speed row.
    pub ms: Option<String>,
    /// `g`: OSM gauge tag. The tooltip's Gauge row.
    pub g: Option<String>,
    /// `tr`: OSM tracks tag. Not read by the app yet; kept in the tile input.
    pub tr: Option<String>,
    /// `n`: the way's own name. The track name label (`lbl-track`, only where the piece has no line), the railway lookup and the tooltip title.
    pub n: Option<String>,
    /// `fq`: OSM frequency tag. The tooltip's Electrified row (traction frequency).
    pub fq: Option<String>,
    /// `kmh`: maxspeed in km/h as a number, when parseable. The speed colour mode and the speed band filters.
    pub kmh: Option<i64>,
    /// `n` again, for track with no name of its own that takes the name of its railway relation (LGV Sud-Est). Written after `kmh`.
    pub n_rwy: Option<String>,
    /// `ne`: English name of `n`. The tooltip's second name line.
    pub ne: Option<String>,
    /// `sv`: OSM service tag (siding, yard, ...). Marks yard and service track in the tooltip.
    pub sv: Option<String>,
    /// `tun`: 1 for a tunnel. The track opacity (paler from z13).
    pub tun: bool,
    /// `br`: 1 for a bridge. Not read by the app yet; kept in the tile input.
    pub br: bool,
    /// `con`: 1 for track under construction (or proposed). Hover and click only; the drawing is `con_dash`.
    pub con: bool,
    /// `prop`: 1 for proposed track. The Proposed filter and the tooltip.
    pub prop: bool,
    /// `ln`: comma-separated line ids (OSM relation numbers) of the services on this way, most prominent type first. A click opens the first.
    pub ln: Option<String>,
    /// `nl`: number of lines on this way. Its presence hides the track's own label and the track itself from z12 (the strands draw it).
    pub nl: Option<usize>,
    /// `oc`: colour of the first line that has one. The operator colour mode below z12.
    pub oc: Option<String>,
}

impl TrackProps {
    /// the way's name, own or its railway's
    pub fn name(&self) -> Option<&str> {
        self.n.as_deref().or(self.n_rwy.as_deref())
    }

    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("id".into(), json!(self.id));
        m.insert("type".into(), json!(self.ty));
        opt(&mut m, "usage", &self.usage);
        opt(&mut m, "el", &self.el);
        opt(&mut m, "v", &self.v);
        opt(&mut m, "ms", &self.ms);
        opt(&mut m, "g", &self.g);
        opt(&mut m, "tr", &self.tr);
        opt(&mut m, "n", &self.n);
        opt(&mut m, "fq", &self.fq);
        if let Some(k) = self.kmh {
            m.insert("kmh".into(), json!(k));
        }
        opt(&mut m, "n", &self.n_rwy);
        opt(&mut m, "ne", &self.ne);
        opt(&mut m, "sv", &self.sv);
        flag(&mut m, "tun", self.tun);
        flag(&mut m, "br", self.br);
        flag(&mut m, "con", self.con);
        flag(&mut m, "prop", self.prop);
        opt(&mut m, "ln", &self.ln);
        if let Some(n) = self.nl {
            m.insert("nl".into(), json!(n));
        }
        opt(&mut m, "oc", &self.oc);
        m
    }
}

/// Layer `track`, zoomed-out overview (z0 to 8): merged pieces keyed by these attributes alone, so equal neighbours merge into long lines.
pub(super) struct OverviewProps {
    /// `type`: rail type. Colour, width and the type filters.
    pub ty: String,
    /// `con`: 1 for construction. The Construction filter.
    pub con: bool,
    /// `prop`: 1 for proposed. The Proposed filter.
    pub prop: bool,
    /// `oc`: line colour. The operator colour mode.
    pub oc: Option<String>,
    /// `ln`: line ids on the piece. A click opens the first line.
    pub ln: Option<String>,
    /// `kmh`: lower bound of the speed band (not the speed itself), so pieces of one band merge. The speed colour mode.
    pub kmh: Option<i64>,
    /// `nk`: railway number into railways.json. The hover names the railway, not the services.
    pub nk: Option<usize>,
}

impl OverviewProps {
    /// the metro sketch of z4 to 6: the type only, so it merges into a few long lines
    pub fn sketch_metro() -> Self {
        OverviewProps { ty: "metro".into(), con: false, prop: false, oc: None, ln: None, kmh: None, nk: None }
    }

    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("type".into(), json!(self.ty));
        flag(&mut m, "con", self.con);
        flag(&mut m, "prop", self.prop);
        opt(&mut m, "oc", &self.oc);
        opt(&mut m, "ln", &self.ln);
        if let Some(k) = self.kmh {
            m.insert("kmh".into(), json!(k));
        }
        if let Some(n) = self.nk {
            m.insert("nk".into(), json!(n));
        }
        m
    }
}

/// Layer `con_dash`: the dashes of construction and proposed track, cut as geometry (no ids), one copy per zoom band.
pub(super) struct ConDashProps {
    /// `type`: rail type. Colour and the type filters.
    pub ty: String,
    /// `prop`: 1 for proposed. Separate layer (Proposed switch).
    pub prop: bool,
    /// `kmh`: lower bound of the speed band. The speed colour mode.
    pub kmh: Option<i64>,
    /// `b`: zoom band (see shared/constants.json `con_band`). The app shows one band's layer per zoom.
    pub b: i64,
}

impl ConDashProps {
    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("type".into(), json!(self.ty));
        flag(&mut m, "prop", self.prop);
        if let Some(k) = self.kmh {
            m.insert("kmh".into(), json!(k));
        }
        m.insert("b".into(), json!(self.b));
        m
    }
}

/// Layer `lines`: one strand per line per way (z11 and deeper), drawn side by side where lines share track.
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
    pub lbe: Option<String>,
    /// `rf`: the line's number for its badge (2, S5, M). The badge symbol.
    pub rf: Option<String>,
    /// `rl`: 1 when the line colour is light, so the badge uses dark text.
    pub rl: bool,
    /// `c`: the line's colour. The operator colour mode and the badge background.
    pub c: Option<String>,
}

impl StrandProps {
    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("lid".into(), json!(self.lid));
        m.insert("t".into(), json!(self.t));
        m.insert("i".into(), json!(self.i));
        m.insert("n".into(), json!(self.n));
        m.insert("lb".into(), json!(self.lb));
        opt(&mut m, "lbe", &self.lbe);
        opt(&mut m, "rf", &self.rf);
        flag(&mut m, "rl", self.rl);
        opt(&mut m, "c", &self.c);
        m
    }
}

/// Layers `areas`, `buildings` and `platforms` (polygons): railway land, yards, station buildings, platforms.
pub(super) struct AreaProps {
    /// `id`: OSM id. The Inspect lookup.
    pub id: String,
    /// `ha`: area in hectares. The tooltip.
    pub ha: f64,
    /// `n`: name. The tooltip.
    pub n: Option<String>,
    /// `op`: operator. The tooltip.
    pub op: Option<String>,
    /// `k`: kind (land, yard, building, platform). Picks the style layer and the tooltip title.
    pub k: &'static str,
    /// `ref`: platform ref. The platform label (platforms only).
    pub platform_ref: Option<String>,
}

impl AreaProps {
    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("id".into(), json!(self.id));
        m.insert("ha".into(), f(self.ha));
        opt(&mut m, "n", &self.n);
        opt(&mut m, "op", &self.op);
        m.insert("k".into(), json!(self.k));
        opt(&mut m, "ref", &self.platform_ref);
        m
    }
}

/// Layer `platforms` (a platform mapped as a line).
pub(super) struct PlatformLineProps {
    /// `id`: OSM id. The Inspect lookup.
    pub id: String,
    /// `ref`: platform ref. The platform label.
    pub platform_ref: String,
}

impl PlatformLineProps {
    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("id".into(), json!(self.id));
        m.insert("ref".into(), json!(self.platform_ref));
        m.insert("k".into(), json!("platform"));
        m
    }
}

/// Layer `airports`: the outline of an aerodrome or terminal.
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
    pub n: Option<String>,
    /// `ne`: English name. The tooltip.
    pub ne: Option<String>,
    /// `iata`: IATA code. The tooltip.
    pub iata: Option<String>,
    /// `icao`: ICAO code. The tooltip.
    pub icao: Option<String>,
    /// `op`: operator. The tooltip.
    pub op: Option<String>,
    /// `com`: 1 for a commercial airport (IATA code, not military, 300 ha or more). The airport style layers.
    pub com: bool,
}

impl AirportProps {
    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("id".into(), self.id.clone());
        m.insert("k".into(), self.k.clone());
        m.insert("lb".into(), json!(self.lb));
        m.insert("ha".into(), f(self.ha));
        opt(&mut m, "n", &self.n);
        opt(&mut m, "ne", &self.ne);
        opt(&mut m, "iata", &self.iata);
        opt(&mut m, "icao", &self.icao);
        opt(&mut m, "op", &self.op);
        flag(&mut m, "com", self.com);
        m
    }
}

/// Layer `airport_pts`: one icon point per commercial airport, so tile edges cannot repeat it.
pub(super) struct AirportPtProps {
    /// `id`: OSM id of the airport.
    pub id: Value,
    /// `lb`: IATA code, else name. The label under the icon.
    pub lb: String,
}

impl AirportPtProps {
    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("id".into(), self.id.clone());
        m.insert("lb".into(), json!(self.lb));
        m
    }
}

/// Layer `stations`: one point per station (stop area or station).
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
    pub term: bool,
    /// `hsr`: 1 for a high-speed station. The ring colour and the HSR only switch.
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
    pub lg: Option<String>,
    /// `ap`: airport it serves (IATA code or name). The tooltip's Airport row.
    pub ap: Option<String>,
    /// `con`: 1 for a station under construction. Dashed ring, the Construction filter and the tooltip.
    pub con: bool,
    /// `fr`: 1 for a freight or yard station (no passenger service or track). The tooltip.
    pub fr: bool,
}

impl StationProps {
    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("id".into(), self.id.clone());
        m.insert("cx".into(), self.cx.clone());
        m.insert("n".into(), json!(self.n));
        m.insert("ne".into(), json!(self.ne));
        m.insert("tier".into(), json!(self.tier));
        m.insert("term".into(), json!(self.term as i64));
        m.insert("hsr".into(), json!(self.hsr as i64));
        m.insert("mp".into(), self.mp.clone());
        m.insert("pc".into(), self.pc.clone());
        m.insert("lc".into(), self.lc.clone());
        m.insert("lct".into(), json!(self.lct));
        m.insert("sb".into(), json!(self.sb));
        m.insert("ty".into(), json!(self.ty));
        m.insert("modes".into(), json!(self.modes));
        opt(&mut m, "lg", &self.lg);
        opt(&mut m, "ap", &self.ap);
        flag(&mut m, "con", self.con);
        flag(&mut m, "fr", self.fr);
        m
    }
}

/// Layer `complexes`: one point per station complex, shown where its stations are still too small to show one by one.
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
    pub term: bool,
    /// `ns`: number of stations in the complex. The tooltip.
    pub ns: Value,
    /// `con`: 1 when every station of the complex is being built. Dashed ring.
    pub con: bool,
    /// `sb`: `,`-wrapped speed band indexes of its stations (9 = none mapped). Hides it with its hidden speed bands.
    pub sb: String,
    /// `ty`: `,`-wrapped rail types of its stations. Hides it with its hidden types.
    pub ty: String,
}

impl ComplexProps {
    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("id".into(), self.id.clone());
        m.insert("n".into(), json!(self.n));
        m.insert("ne".into(), json!(self.ne));
        m.insert("tier".into(), json!(self.tier));
        m.insert("term".into(), json!(self.term as i64));
        m.insert("ns".into(), self.ns.clone());
        flag(&mut m, "con", self.con);
        m.insert("sb".into(), json!(self.sb));
        m.insert("ty".into(), json!(self.ty));
        m
    }
}

/// Layer `cities`: one dot per city with a metro, tram or monorail system, visible from far out.
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

impl CityProps {
    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("m".into(), json!(self.m));
        m.insert("l".into(), json!(self.l));
        m.insert("r".into(), json!(self.r));
        m.insert("n".into(), json!(self.n));
        m
    }
}
