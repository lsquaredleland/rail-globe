//! Tag rules: what the cache keeps (a superset) and how the cut decides, ported from cpp/filter.cpp and airports.py (removed;
//! see git commit 80b9620).
//! Rules follow docs/03-data-spec.md. The same functions run on block tags while building the cache and on stored tags while cutting.

/// tag access by key
pub trait Tags {
    fn get(&self, k: &[u8]) -> Option<&[u8]>;
}

/// tags stored in the cache (or any list of key/value slices)
pub struct List<'a>(pub Vec<(&'a [u8], &'a [u8])>);

impl Tags for List<'_> {
    #[inline]
    fn get(&self, k: &[u8]) -> Option<&[u8]> {
        self.0.iter().find(|(a, _)| *a == k).map(|(_, v)| *v)
    }
}

#[inline]
pub fn is<T: Tags + ?Sized>(t: &T, k: &[u8], v: &[u8]) -> bool {
    t.get(k) == Some(v)
}
#[inline]
pub fn has<T: Tags + ?Sized>(t: &T, k: &[u8]) -> bool {
    t.get(k).is_some()
}

// funicular is not a mode flag here, as in filter.cpp
pub const MODE_FLAGS: [&[u8]; 5] = [b"train", b"subway", b"light_rail", b"tram", b"monorail"];
pub const TRACK: [&[u8]; 9] = [b"rail", b"subway", b"light_rail", b"tram", b"monorail", b"funicular", b"narrow_gauge", b"construction", b"proposed"];
pub const SERVICE_KEEP: [&[u8]; 4] = [b"yard", b"siding", b"spur", b"crossover"];
pub const ROUTE_MODES: [&[u8]; 6] = [b"train", b"subway", b"light_rail", b"tram", b"monorail", b"funicular"];

/// feature properties, in this order, then every name:* tag (filter.cpp KEEP)
pub const KEEP: [&str; 39] = [
    "railway", "public_transport", "building", "landuse", "station", "service", "usage", "railway:traffic_mode",
    "highspeed", "name", "official_name", "int_name", "ref", "wikidata", "uic_ref", "railway:ref",
    "operator", "operator:short", "network", "gauge", "electrified", "voltage", "frequency", "tracks",
    "maxspeed", "maxspeed:freight", "construction", "proposed", "bridge", "tunnel", "light_rail", "subway", "train", "start_date",
    "construction:railway", "construction:public_transport", "construction:station", "operator:wikidata", "brand:wikidata",
];

/// relation side-table tags, in this order, then every name:* tag (filter.cpp REL_TAGS)
pub const REL_TAGS: [&str; 15] =
    ["name", "ref", "colour", "operator", "operator:wikidata", "brand:wikidata", "network", "route", "service", "from", "to", "wikidata", "public_transport", "type", "route_master"];

/// Tags stored in the cache: everything any rule or output reads. Anything else is dropped at cache time, so a new output tag
/// means rebuilding the cache (one planet read).
#[inline]
pub fn cache_key(k: &[u8]) -> bool {
    if k.starts_with(b"name:") {
        return true;
    }
    matches!(
        k,
        b"railway" | b"public_transport" | b"building" | b"landuse" | b"station" | b"service" | b"usage" | b"railway:traffic_mode"
            | b"highspeed" | b"name" | b"official_name" | b"int_name" | b"ref" | b"wikidata" | b"uic_ref" | b"railway:ref"
            | b"operator" | b"operator:short" | b"operator:wikidata" | b"brand:wikidata" | b"network" | b"gauge" | b"electrified" | b"voltage" | b"frequency" | b"tracks"
            | b"maxspeed" | b"maxspeed:freight" | b"construction" | b"proposed" | b"bridge" | b"tunnel" | b"light_rail"
            | b"subway" | b"train" | b"tram" | b"monorail" | b"funicular" | b"start_date" | b"construction:railway"
            | b"construction:public_transport" | b"construction:station" | b"entrance" | b"area" | b"aeroway" | b"iata"
            | b"icao" | b"aerodrome:type" | b"military" | b"type" | b"route" | b"route_master" | b"colour" | b"from" | b"to"
            | b"boundary" | b"admin_level" | b"ISO3166-1" | b"ISO3166-1:alpha2" | b"ISO3166-2"
    )
}

/// Tags stored for the place nodes (place=city, town): what the places step reads.
#[inline]
pub fn place_key(k: &[u8]) -> bool {
    matches!(k, b"place" | b"name" | b"name:en" | b"int_name" | b"population" | b"capital" | b"wikidata")
}

/// admin levels whose boundaries the cache keeps, as region outlines (2 countries, 4 provinces / states, 5 prefecture-level cities in China and similar)
pub const ADMIN_LEVELS: [&[u8]; 3] = [b"2", b"4", b"5"];

/// an administrative boundary relation the cache keeps for region outlines
pub fn admin_boundary<T: Tags + ?Sized>(t: &T) -> bool {
    matches!(t.get(b"type"), Some(b"boundary" | b"multipolygon"))
        && is(t, b"boundary", b"administrative")
        && t.get(b"admin_level").map_or(false, |l| ADMIN_LEVELS.contains(&l))
}

/// a city's own boundary (admin levels 6 to 9: the county, municipality or city limits), kept only when its Wikidata id is that of a
/// place=city node (`wanted`, collected in a pass over the nodes before the relations are read). Level 5 is kept whole already.
pub fn city_boundary<T: Tags + ?Sized>(t: &T, wanted: &std::collections::HashSet<Vec<u8>>) -> bool {
    matches!(t.get(b"type"), Some(b"boundary" | b"multipolygon"))
        && is(t, b"boundary", b"administrative")
        && t.get(b"admin_level").map_or(false, |l| matches!(l, b"6" | b"7" | b"8" | b"9"))
        && t.get(b"wikidata").map_or(false, |w| wanted.contains(w))
}

pub fn passenger_mode<T: Tags + ?Sized>(t: &T) -> bool {
    MODE_FLAGS.iter().any(|m| is(t, m, b"yes"))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    /// a named station or stop being built
    Construction,
    /// public_transport=stop_position or railway=stop (written to stop_positions.jsonl)
    Stop,
    Station,
}

/// a named station or stop that is being built: railway=construction + construction=station|halt, or the lifecycle prefix
/// construction:railway=station|halt|stop / construction:public_transport=station|stop_position
pub fn under_construction_station<T: Tags + ?Sized>(t: &T) -> bool {
    if !has(t, b"name") {
        return false;
    }
    if is(t, b"railway", b"construction") && (is(t, b"construction", b"station") || is(t, b"construction", b"halt")) {
        return true;
    }
    let cr = t.get(b"construction:railway");
    let cp = t.get(b"construction:public_transport");
    matches!(cr, Some(b"station" | b"halt" | b"stop")) || matches!(cp, Some(b"station" | b"stop_position"))
}

/// filter.cpp Handler::node
pub fn node_kind<T: Tags + ?Sized>(t: &T) -> Option<NodeKind> {
    if has(t, b"entrance") {
        return None;
    }
    if under_construction_station(t) {
        return Some(NodeKind::Construction);
    }
    if is(t, b"public_transport", b"stop_position") || is(t, b"railway", b"stop") {
        return if passenger_mode(t) || is(t, b"railway", b"stop") { Some(NodeKind::Stop) } else { None };
    }
    let station = is(t, b"railway", b"station")
        || is(t, b"railway", b"halt")
        || (is(t, b"public_transport", b"station") && (has(t, b"railway") || passenger_mode(t)));
    // a public_transport=station that is abandoned, disused or only proposed is not a station (the data spec drops them)
    if !station || matches!(t.get(b"railway"), Some(b"abandoned" | b"disused" | b"proposed")) {
        return None;
    }
    Some(NodeKind::Station)
}

/// filter.cpp classify_poly
pub fn classify_poly<T: Tags + ?Sized>(t: &T) -> Option<&'static str> {
    if is(t, b"landuse", b"railway") {
        return Some("area:landuse_railway");
    }
    if is(t, b"railway", b"yard") {
        return Some("area:yard");
    }
    if is(t, b"building", b"train_station") {
        return Some("area:station_building");
    }
    if is(t, b"public_transport", b"station") && (has(t, b"railway") || passenger_mode(t)) {
        return Some("area:station");
    }
    if is(t, b"railway", b"station") || is(t, b"railway", b"halt") {
        return Some("area:station");
    }
    if is(t, b"railway", b"platform") || (is(t, b"public_transport", b"platform") && passenger_mode(t)) {
        return Some("area:platform");
    }
    None
}

#[inline]
pub fn aeroway_area<T: Tags + ?Sized>(t: &T) -> bool {
    matches!(t.get(b"aeroway"), Some(b"aerodrome" | b"terminal"))
}

/// ways the cache keeps on their own tags (the cut applies the exact rules)
pub fn way_wanted<T: Tags + ?Sized>(t: &T) -> bool {
    if let Some(rw) = t.get(b"railway") {
        if TRACK.contains(&rw) || matches!(rw, b"siding" | b"spur" | b"crossover") {
            return true;
        }
    }
    classify_poly(t).is_some() || aeroway_area(t)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RelKind {
    StopArea,
    StopAreaGroup,
    Route,
    RouteMaster,
    /// a named infrastructure line (route=railway), not a service
    RailwayLine,
}

impl RelKind {
    pub fn name(self) -> &'static str {
        match self {
            RelKind::StopArea => "stop_area",
            RelKind::StopAreaGroup => "stop_area_group",
            RelKind::Route => "route",
            RelKind::RouteMaster => "route_master",
            RelKind::RailwayLine => "railway_line",
        }
    }
}

/// filter.cpp RelHandler: which relations go into the side table
pub fn rel_kind<T: Tags + ?Sized>(t: &T) -> Option<RelKind> {
    let typ = t.get(b"type");
    let route = t.get(b"route");
    let pt = t.get(b"public_transport");
    if typ == Some(b"public_transport") && pt == Some(b"stop_area") {
        return Some(RelKind::StopArea);
    }
    if typ == Some(b"public_transport") && pt == Some(b"stop_area_group") {
        return Some(RelKind::StopAreaGroup);
    }
    if typ == Some(b"route") && route.map_or(false, |r| ROUTE_MODES.contains(&r)) {
        return Some(RelKind::Route);
    }
    if typ == Some(b"route_master") && t.get(b"route_master").map_or(false, |r| ROUTE_MODES.contains(&r)) {
        return Some(RelKind::RouteMaster);
    }
    if typ == Some(b"route") && route == Some(b"railway") {
        return Some(RelKind::RailwayLine);
    }
    None
}

/// relations assembled into areas (libosmium MultipolygonManager: type=multipolygon or boundary), when their tags are wanted
pub fn rel_is_area_type<T: Tags + ?Sized>(t: &T) -> bool {
    matches!(t.get(b"type"), Some(b"multipolygon" | b"boundary"))
}

pub fn rel_area_wanted<T: Tags + ?Sized>(t: &T) -> bool {
    rel_is_area_type(t) && (classify_poly(t).is_some() || aeroway_area(t))
}

// ---------------------------------------------------------------- names (pipeline/names.py)

/// Latin script for labels: chars U+0000-U+024F or U+1E00-U+1EFF
pub fn latin(v: &[u8]) -> bool {
    std::str::from_utf8(v).map_or(false, |s| s.chars().all(|c| (c as u32) <= 0x24f || (0x1e00..=0x1eff).contains(&(c as u32))))
}

/// a name:* key that holds an English or romanised form of the name: name:en (also name:en-Latn, name:en_GB), or a transliteration of
/// the local name (name:ja-Latn, name:ja_rm, name:zh-Latn-pinyin, name:*_pinyin, ...). Never name:etymology*, name:*:wikidata,
/// name:left / name:right or the name of another language (name:de on a Czech station is German, not English).
fn english_key(k: &[u8]) -> bool {
    let Some(l) = k.strip_prefix(b"name:") else { return false };
    if l.contains(&b':') {
        return false;
    }
    let has = |w: &[u8]| l.windows(w.len()).any(|x| x == w);
    l == b"en" || l.starts_with(b"en-") || l.starts_with(b"en_") || has(b"Latn") || l.ends_with(b"_rm") || has(b"pinyin") || has(b"romaji") || has(b"translit")
}

/// a name:en (or int_name, or romanised name) that holds a placeholder, not a name ("待定" is "to be determined"): it is no English label
fn placeholder_name(v: &[u8]) -> bool {
    let t = String::from_utf8_lossy(v);
    matches!(t.trim().to_lowercase().as_str(), "待定" | "tbd" | "unknown")
}

/// The English label of a feature, from its (key, value) tags: name:en, then int_name; for a name that is not Latin script (CJK,
/// Cyrillic, Arabic, ...) also a romanised name:* (see english_key). None when the name is already Latin script and there is no
/// name:en or int_name: the label is the name itself (a Latin name:de or name:fr is not English, and name:etymology:wikidata is a Q id).
pub fn english_name_kv<'a>(tags: impl Iterator<Item = (&'a [u8], &'a [u8])>) -> Option<&'a [u8]> {
    let (mut name, mut en, mut int, mut roman) = (None, None, None, None);
    for (k, v) in tags {
        if v.is_empty() || (k != b"name" && placeholder_name(v)) {
            continue;
        }
        match k {
            b"name" => name = Some(v),
            b"name:en" => en = en.or(Some(v)),
            b"int_name" => int = Some(v),
            _ if roman.is_none() && english_key(k) && latin(v) => roman = Some(v),
            _ => {}
        }
    }
    en.or(int).or_else(|| if name.map_or(false, latin) { None } else { roman })
}

pub fn english_name<'a>(t: &'a List<'a>) -> Option<&'a [u8]> {
    english_name_kv(t.0.iter().copied())
}

#[cfg(test)]
mod english_name_tests {
    use super::*;
    fn en(tags: &[(&str, &str)]) -> Option<String> {
        let l = List(tags.iter().map(|(k, v)| (k.as_bytes(), v.as_bytes())).collect());
        english_name(&l).map(|v| String::from_utf8(v.to_vec()).unwrap())
    }
    #[test]
    fn explicit_english() {
        assert_eq!(en(&[("name", "Wien Hbf"), ("name:en", "Vienna Central"), ("name:de", "Wien Hbf")]).as_deref(), Some("Vienna Central"));
        assert_eq!(en(&[("name", "Praha"), ("int_name", "Prague")]).as_deref(), Some("Prague"));
        assert_eq!(en(&[("name", "東京"), ("name:en", "Tokyo")]).as_deref(), Some("Tokyo"));
    }
    #[test]
    fn placeholder_english_names_are_dropped() {
        assert_eq!(en(&[("name", "待定"), ("name:en", "待定")]), None);
        assert_eq!(en(&[("name", "龙吴路"), ("name:en", "TBD")]), None);
        assert_eq!(en(&[("name", "龙吴路"), ("name:en", "tbd"), ("int_name", "Longwu Road")]).as_deref(), Some("Longwu Road"));
        assert_eq!(en(&[("name", "X"), ("name:en", "Unknown")]), None);
        assert_eq!(en(&[("name", "龙吴路"), ("name:en", " Tbd ")]), None);
        assert_eq!(en(&[("name", "東京"), ("name:en", "Tokyo")]).as_deref(), Some("Tokyo"));
    }
    #[test]
    fn latin_name_without_english_has_no_english_label() {
        assert_eq!(en(&[("name", "Warszawa Zachodnia"), ("name:de", "Warschau Westbahnhof")]), None);
        assert_eq!(en(&[("name", "Chicago Ogilvie"), ("name:etymology:wikidata", "Q735271")]), None);
        assert_eq!(en(&[("name", "Roma Termini"), ("name:fr", "Rome")]), None);
    }
    #[test]
    fn never_wikidata_or_other_language() {
        assert_eq!(en(&[("name", "Бриус"), ("name:etymology:wikidata", "Q735271")]), None);
        assert_eq!(en(&[("name", "Бриус"), ("name:de", "Brius")]), None);
        assert_eq!(en(&[("name:etymology", "Ogilvie")]), None);
    }
    #[test]
    fn non_latin_name_takes_a_romanisation() {
        assert_eq!(en(&[("name", "渋谷"), ("name:ja-Latn", "Shibuya"), ("name:ja", "渋谷")]).as_deref(), Some("Shibuya"));
        assert_eq!(en(&[("name", "渋谷"), ("name:ja_rm", "Shibuya")]).as_deref(), Some("Shibuya"));
        assert_eq!(en(&[("name", "成都"), ("name:zh-Latn-pinyin", "Chengdu")]).as_deref(), Some("Chengdu"));
    }
}
