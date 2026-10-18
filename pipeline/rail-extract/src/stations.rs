//! Station model (docs/04-station-model.md), ported from pipeline/stations.py.
//!
//! Levels: Complex (the place a traveller names) > Station (one per mode group) > stop points. Rank comes from physical size only;
//! lines are counted for the panel and never set rank. Same steps, rules and outputs as stations.py; where Python took the first of
//! equal candidates in shapely's tree order (or iterated a set), this takes the lowest index (or sorted order), so runs repeat.

use crate::geomx::{self, Index, G};
use crate::pyjson::{f, py_round};
use crate::site::{s, Feat, Input};
use geo::{Geometry, MultiPolygon, Point};
use regex::Regex;
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::{char::canonical_combining_class, UnicodeNormalization};

const RADIUS_TRAIN: f64 = 300.0; // metres, tuned on the pilots
const RADIUS_OTHER: f64 = 100.0;
const GROUP_RADIUS: f64 = 300.0;
const SECOND_DOT_M: f64 = 100.0; // same-mode stations this close with the same name are one station
const SECOND_DOT_NAMELESS_M: f64 = 60.0; // ... a nameless one joins the nearest named station this close
const KEY_MERGE_RADIUS: f64 = 1000.0; // stations sharing a wikidata or uic_ref merge only within this distance
const KEY_MERGE_RADIUS_NAMED: f64 = 3000.0; // ... or within this one when their normalised names are equal
const LAND_DISC: f64 = 500.0; // railway land counts only near the station (landuse=railway polygons span whole corridors)
const HS_NEAR: f64 = 80.0; // "next to high-speed track"
const HS_MAJOR_PLATFORMS: i64 = 8;
const TERMINUS_ZONE: f64 = 120.0; // around the station's mainline platforms
// size score weights and tiers (tuned on the answer lists)
const W_PLATFORM: f64 = 3.0;
const W_TRACK: f64 = 0.5;
const W_TRACK_CAP: f64 = 40.0;
const W_LAND: f64 = 3.0;
const W_BUILDING: f64 = 4.0;
const W_TERMINUS: f64 = 10.0;
const STOPWORDS: [&str; 29] = ["gare", "du", "de", "d", "la", "le", "les", "l", "des", "station", "paris", "stazione", "railway", "rer", "metro",
    "subway", "bart", "caltrain", "amtrak", "light", "rail", "vta", "muni", "st", "saint", "bahnhof", "bhf", "di", ""];

fn radius(m: &str) -> f64 {
    if m == "train" { RADIUS_TRAIN } else { RADIUS_OTHER }
}

fn max_radius(ms: &BTreeSet<String>) -> f64 {
    ms.iter().map(|m| radius(m)).fold(f64::MIN, f64::max)
}

fn mode_key(k: &str) -> Option<&'static str> {
    Some(match k {
        "subway" => "subway",
        "light_rail" => "light_rail",
        "train" => "train",
        "monorail" => "monorail",
        "funicular" => "funicular",
        _ => return None,
    })
}

fn track_mode(t: &str) -> &'static str {
    match t {
        "metro" => "subway",
        "light_rail" => "light_rail",
        "tram" => "tram",
        "monorail" => "monorail",
        "funicular" => "funicular",
        "other" => "other",
        _ => "train",
    }
}

/// stations.py norm_name: lower case, accents off, bracketed parts and punctuation out, a trailing 站 off, stopwords out
pub fn norm_name(name: &str) -> String {
    thread_local! {
        static RE: (Regex, Regex, Regex, Regex) = (
            Regex::new(r"\(.*?\)").unwrap(),
            Regex::new(r"[^\p{L}\p{N}_一-鿿]+").unwrap(),   // Python's \w: letters, numbers, underscore
            Regex::new(r"(地铁站|火车站|客站|站|駅)$").unwrap(),
            Regex::new(r"^jr([぀-ヿ一-鿿])").unwrap(), // "JR宝塚駅" is 宝塚
        );
    }
    if name.is_empty() {
        return String::new();
    }
    let s: String = name.to_lowercase().nfkd().filter(|c| canonical_combining_class(*c) == 0).collect();
    RE.with(|(paren, punct, tail, jr)| {
        let s = jr.replace(&s, "$1");
        let s = paren.replace_all(&s, " ");
        let s = punct.replace_all(&s, " ");
        let s = tail.replace(s.trim(), "");
        s.split_whitespace().filter(|t| !STOPWORDS.contains(t)).collect::<Vec<_>>().join(" ")
    })
}

/// the English label of a property map (rules::english_name_kv: name:en, int_name, a romanised name:* for a non-Latin name; else None)
pub fn english_name(t: &Map<String, Value>) -> Option<String> {
    crate::rules::english_name_kv(t.iter().filter_map(|(k, v)| Some((k.as_bytes(), v.as_str()?.as_bytes())))).and_then(|v| std::str::from_utf8(v).ok()).map(String::from)
}

pub fn ref_tokens(r: Option<&str>) -> BTreeSet<String> {
    r.unwrap_or("").split([';', ',', '/']).map(str::trim).filter(|t| !t.is_empty()).map(String::from).collect()
}

/// sort key of platform refs: numbers first, by value, then the rest
fn ref_key(r: &str) -> (bool, u128, String) {
    let num = !r.is_empty() && r.chars().all(|c| c.is_ascii_digit());
    (!num, if num { r.parse().unwrap_or(u128::MAX) } else { 0 }, r.to_string())
}

/// "1a" and "1b" are sections of platform 1: the platform's number is its digits when only one or two letters follow them
fn plat_key(t: &str) -> String {
    let d: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &t[d.len()..];
    if !d.is_empty() && (1..=2).contains(&rest.chars().count()) && rest.chars().all(|c| c.is_ascii_alphabetic()) { d } else { t.to_string() }
}

/// a bracket that names a line, mode or operator ("(National Rail)", "(Green line)", "(東武東上線)", "(A,C,E)"), not a place or a
/// direction ("(Cheshire)", "(West)", "(Beverly Hills)")
fn lineish(c: &str) -> bool {
    thread_local! {
        static RE: Regex = Regex::new(r"(?i)^(.*\b(line|ligne|linie|lijn)\b.*|m[eé]tro|u|s|u-?bahn|s-?bahn|tram|nr|national rail|underground|overground|rer|m\d+|.+線|.+线|国铁|國鐵|地铁|地鐵)$").unwrap();
        static LET: Regex = Regex::new(r"^[A-Z0-9]{1,2}([,/ ]+[A-Z0-9]{1,2})*$").unwrap();
    }
    let c = c.trim();
    RE.with(|r| r.is_match(c)) || LET.with(|r| r.is_match(c))
}

/// the first item of a ";" list, without a trailing "[...]" tag ("Dudley Castle & Zoo [Aug 2026]", "Męcina [Nowa]") or "under construction"
/// suffix ("龙吴路（在建）", "(under construction)"), whitespace (also the ideographic space) collapsed to single spaces
fn tidy_name(name: &str) -> String {
    thread_local! {
        static RE: (Regex, Regex) = (Regex::new(r"\s*\[[^\]]*\]\s*$").unwrap(), Regex::new(CONSTRUCTION_SUFFIX).unwrap());
    }
    let first = name.split(';').next().unwrap_or("");
    let one = first.split_whitespace().collect::<Vec<_>>().join(" ");
    RE.with(|(tag, con)| {
        let t = tag.replace(&one, "");
        let t = con.replace(&t, "");
        let t = tag.replace(&t, "");
        t.trim().to_string()
    })
}

/// a name that ends in "(在建)", "（建设中）", "(under construction)", "(en construcción)" (round or full-width brackets)
const CONSTRUCTION_SUFFIX: &str = r"(?i)\s*[(（]\s*(?:\d+号线)?(?:在建|在建中|建设中|建設中|under construction|en construcci[oó]n|u/c)\s*[)）]\s*$";

/// A name that says the station is not open: "(在建)", "（建设中）", "(under construction)", "(en construcción)".
fn con_name(p: &Map<String, Value>) -> bool {
    thread_local! {
        static RE: Regex = Regex::new(CONSTRUCTION_SUFFIX).unwrap();
    }
    s(p, "name").map_or(false, |n| RE.with(|r| r.is_match(n.trim())))
}

/// A station tagged as one being built: `construction=yes|station`, `construction:railway` or `construction:public_transport`, and none
/// of the evidence that it is open: a `railway=station|halt|tram_stop` tag of its own (a station that carries the tag of a future
/// extension, Crumpsall, Arcueil-Cachan, or a leftover `construction=yes`, Bönningstedt, is open), membership of a service route
/// (`in_route`), or for a bare `construction=yes` a stop area. A platform beside it proves nothing: the platforms of planned stations
/// are mapped without a lifecycle prefix. A feature with only `public_transport=
/// station` (or `railway=site`) and `construction:railway=station` is the lifecycle form of a station being built.
fn con_tags(p: &Map<String, Value>, in_route: bool) -> bool {
    let strong = s(p, "construction") == Some("station")
        || s(p, "construction:railway").map_or(false, |v| !v.is_empty())
        || s(p, "construction:public_transport").map_or(false, |v| !v.is_empty());
    let weak = s(p, "construction") == Some("yes");
    let open_tag = matches!(s(p, "railway"), Some("station" | "halt" | "tram_stop"));
    // a stop area is no proof of service (planned Bangkok stations have one), but it does clear a bare construction=yes
    let has_area = s(p, "_stop_area").map_or(false, |v| !v.is_empty());
    (strong || weak) && !open_tag && !in_route && (strong || !has_area)
}

/// either reason, for a feature that has no route or platform to check
#[cfg(test)]
fn under_construction(p: &Map<String, Value>) -> bool {
    con_name(p) || con_tags(p, false)
}

/// a station label cleaned of operator and format noise. Always: a leading "Bf. "/"Bhf. "/"Hp. ", a trailing ", Bahnhof", the VBB form
/// "Berlin, U Voltastraße" / "Berlin, S+U Wedding". Only when the shorter name is the name of a station within GROUP_RADIUS
/// (`bare_near`, never region-wide: "103rd Street (Beverly Hills)" keeps its place): a trailing bracket that names a line, mode or
/// operator (`is_op` = an operator or network of this station), and the Korean type words 기차역/기차 ("서울역 기차역")
fn clean_name(name: &str, bare_near: &dyn Fn(&str) -> bool, is_op: &dyn Fn(&str) -> bool) -> String {
    thread_local! {
        static RE: (Regex, Regex) = (Regex::new(r"^[^,]+, (?:S\+U|U\+S|[US]) (\S.*)$").unwrap(), Regex::new(r"^(.*\S)\s*[(（]([^()（）]*)[)）]$").unwrap());
    }
    let tidy = tidy_name(name);
    let mut n = tidy.as_str();
    if let Some(r) = n.strip_prefix("Bf. ").or_else(|| n.strip_prefix("Bhf. ")).or_else(|| n.strip_prefix("Hp. ")) {
        n = r.trim();
    }
    if let Some(r) = n.strip_suffix(", Bahnhof") {
        n = r.trim();
    }
    let vbb;
    if let Some(c) = RE.with(|(v, _)| v.captures(n).map(|c| c[1].to_string())) {
        vbb = c;
        n = vbb.trim();
    }
    let mut cands: Vec<String> = Vec::new();
    if let Some((bare, inner)) = RE.with(|(_, p)| p.captures(n).map(|c| (c[1].to_string(), c[2].to_string()))) {
        if lineish(&inner) || is_op(inner.trim()) {
            cands.push(bare);
        }
    }
    let toks: Vec<&str> = n.split_whitespace().collect();
    if toks.len() >= 2 {
        if matches!(toks[toks.len() - 1], "기차역" | "기차") {
            cands.push(toks[..toks.len() - 1].join(" "));
        }
        if matches!(toks[0], "기차역" | "기차") {
            cands.push(toks[1..].join(" "));
        }
    }
    cands.into_iter().find(|c| bare_near(c)).unwrap_or_else(|| n.to_string())
}

/// a name that only says what kind of place it is: the name of no station on its own
fn bare_kind(n: &str) -> Option<&'static str> {
    match n.trim().to_lowercase().as_str() {
        "hauptbahnhof" => Some("Hauptbahnhof"),
        "bahnhof" => Some("Bahnhof"),
        "station" => Some("Station"),
        "gare" => Some("Gare"),
        "stazione" => Some("Stazione"),
        "estación" => Some("Estación"),
        _ => None,
    }
}

/// the place in front of a station word in another station's name ("Bremen Hbf", "Mainz Hauptbahnhof", "Berlin-Bahnhof") when `kind` is
/// that station word; None for a kind with no abbreviations here or a name that is not "<place> <word>"
fn place_before(name: &str, kind: &str) -> Option<String> {
    thread_local! {
        static HBF: Regex = Regex::new(r"(?i)^(.+?)[\s,\-–]+(?:Hauptbahnhof|Hbf\.?|Hauptbf\.?)$").unwrap();
        static BF: Regex = Regex::new(r"(?i)^(.+?)[\s,\-–]+(?:Bahnhof|Bf\.?|Bhf\.?)$").unwrap();
    }
    let re = match kind {
        "Hauptbahnhof" => &HBF,
        "Bahnhof" => &BF,
        _ => return None,
    };
    let p = re.with(|r| r.captures(name.trim()).map(|c| c[1].trim().to_string()))?;
    // the VBB form "Berlin, S Hauptbahnhof" / "Berlin, S+U Wedding" names its place before the comma
    let p = match p.split_once(", ") {
        Some((place, line)) if matches!(line, "S" | "U" | "S+U" | "U+S") => place.trim().to_string(),
        Some(_) => return None,
        None => p,
    };
    // a place is a name, not a number, a line or a qualified name ("Berlin Ost", "S+U Wedding")
    let ok = !p.is_empty() && p.split_whitespace().count() <= 2 && !p.chars().any(|c| c.is_ascii_digit() || c == '+' || c == '(')
        && bare_kind(&p).is_none() && !lineish(&p);
    ok.then_some(p)
}

/// "Bremen" + "Hauptbahnhof" -> "Bremen Hauptbahnhof"; French: "Gare de Lyon", "Gare d'Orléans"
fn qualified(place: &str, kind: &str) -> String {
    if kind == "Gare" {
        let v = place.chars().next().map_or(false, |c| "AEIOUHÀÉÈÎÔÂ".contains(c));
        return if v { format!("Gare d'{place}") } else { format!("Gare de {place}") };
    }
    format!("{place} {kind}")
}

fn has_cjk(t: &str) -> bool {
    t.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c) || ('\u{4e00}'..='\u{9fff}').contains(&c))
}

/// words that decorate a station name without naming it (Gilroy Transit Center, Neckarsulm Bahnhof, Bardowick Hp)
const DECORATION: [&str; 12] = ["hp", "pbf", "hbf", "bf", "haltepunkt", "halt", "fermata", "transit", "center", "centre", "kleinbahn", "landesbahn"];

/// the name of a station with its decoration taken off, to compare two names of one station: normalised (norm_name); a CJK name loses
/// the gloss in another script; "Berlin-Köpenick" loses the place before the hyphen; `deco` (the operator's initials, the place that
/// the other station's official_name puts in front) and DECORATION words are dropped. A name that is all decoration stays as it is.
fn core_name(raw: &str, deco: &BTreeSet<String>) -> String {
    let toks = |t: &str| -> Vec<String> { norm_name(t).split_whitespace().map(String::from).collect() };
    let mut t = toks(raw);
    if t.iter().any(|x| has_cjk(x)) {
        t.retain(|x| has_cjk(x));
    }
    if let Some((pre, rest)) = raw.split_once('-') {
        let pt = toks(pre);
        if !pt.is_empty() && !toks(rest).is_empty() && t.starts_with(&pt) {
            t.drain(..pt.len());
        }
    }
    let kept: Vec<String> = t.iter().filter(|x| !DECORATION.contains(&x.as_str()) && !deco.contains(*x)).cloned().collect();
    if kept.is_empty() { t.join(" ") } else { kept.join(" ") }
}

/// the abbreviations an operator or network name holds: its upper-case words ("AVG", "SWEG") and the initials of its words without the
/// company form ("Albtal-Verkehrs-Gesellschaft mbH" is "avg"). Whole words are not taken: "North" in a network name is no decoration.
fn operator_tokens(v: &str, out: &mut BTreeSet<String>) {
    let words: Vec<&str> = v.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    out.extend(words.iter().filter(|w| (2..=5).contains(&w.chars().count()) && w.chars().all(|c| c.is_uppercase())).map(|w| w.to_lowercase()));
    let initials: String = words
        .iter()
        .map(|w| w.to_lowercase())
        .filter(|w| !matches!(w.as_str(), "mbh" | "gmbh" | "ag" | "e" | "v" | "kg" | "co" | "ltd" | "llc" | "plc" | "sa" | "srl" | "spa" | "the" | "of" | "and" | "und"))
        .filter_map(|w| w.chars().next())
        .collect();
    if initials.chars().count() >= 2 {
        out.insert(initials);
    }
}

/// a Japanese operator in front of a station name ("阪急西院駅", "名古屋鉄道知多半田駅", "JR 亀崎駅", "津軽鉄道 五所川原駅")
fn jp_operator_prefix(p: &str) -> bool {
    thread_local! {
        static RE: Regex = Regex::new(r"^(?:jr|阪神|南海|京成|東武|西武|京王|京阪|東京メトロ|都営|名鉄|近鉄|西鉄|北海道|東日本|東海|西日本|四国|九州|.*(?:鉄道|電鉄|鉄|急|メトロ|交通局))$").unwrap();
    }
    RE.with(|r| r.is_match(p))
}

/// a Chinese station type or exit after the name ("诏安站站房", "寿县高铁站", "科宁路地铁站3号口", "中央商务区G2口")
fn cn_suffix(x: &str) -> bool {
    matches!(x.trim_matches('站'), "" | "高铁" | "动车" | "火车" | "地铁" | "客运" | "房" | "厅") || x.ends_with('口')
}

/// `long` is `short` plus a Japanese operator in front or a Chinese station type / exit behind it, spaces ignored (normalised names)
fn decorated(long: &str, short: &str) -> bool {
    let (l, s): (String, String) = (long.split_whitespace().collect(), short.split_whitespace().collect());
    !s.is_empty() && l.len() > s.len() && ((l.ends_with(&s) && jp_operator_prefix(&l[..l.len() - s.len()])) || (l.starts_with(&s) && cn_suffix(&l[s.len()..])))
}

/// what the second merge pass compares of a station
struct NameInfo {
    names: Vec<String>,        // own names of its features and stop areas
    deco: BTreeSet<String>,    // operator initials, the place its official_name adds in front
    wikidata: BTreeSet<String>,
    plain: bool,               // one or two features, no stop area of its own: a polygon or node that repeats a station's name with a decoration
}

struct UF(Vec<usize>);
impl UF {
    fn new(n: usize) -> UF {
        UF((0..n).collect())
    }
    fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[b] = a;
        }
    }
}

/// groups in order of their first member
fn groups(uf: &mut UF, n: usize) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = Vec::new();
    let mut by_root: FxHashMap<usize, usize> = FxHashMap::default();
    let mut out: Vec<Vec<usize>> = Vec::new();
    for i in 0..n {
        let r = uf.find(i);
        let k = *by_root.entry(r).or_insert_with(|| {
            order.push(r);
            out.push(Vec::new());
            out.len() - 1
        });
        out[k].push(i);
    }
    out
}

/// a projected feature (metres in a local equirectangular projection)
struct It<'a> {
    f: &'a Feat,
    g: G,
    modes: BTreeSet<String>, // platforms
}

impl It<'_> {
    fn p(&self, k: &str) -> Option<&str> {
        s(&self.f.p, k)
    }
    fn id(&self) -> &str {
        s(&self.f.p, "osm_id").unwrap_or("")
    }
}

struct Station {
    feats: Vec<usize>, // indices into station_feats
    modes: BTreeSet<String>,
    areas: BTreeSet<String>,
    platforms: Vec<usize>, // indices into platforms
    grouping: &'static str,
    pt: Point<f64>,
    norm: String,
    merged_from: usize,
    stated: bool, // some feature's tags state the mode
}

/// what stations.py writes, kept in memory for the next steps
pub struct Out {
    pub stations: Vec<Value>,  // GeoJSON features, stations.geojsonl
    pub complexes: Vec<Value>, // complexes.geojsonl
    pub route_order: Map<String, Value>,
    pub report: Value,
}

pub fn run(slug: &str, inp: &Input) -> Out {
    // ---- local metric projection
    let lats: Vec<f64> = inp.feats.iter().filter(|f| f.gtype == "Point").filter_map(|f| f.g["coordinates"][1].as_f64()).collect();
    let lat0 = if lats.is_empty() { 40.0 } else { lats.iter().sum::<f64>() / lats.len() as f64 };
    let (kx, ky) = (111320.0 * lat0.to_radians().cos(), 110540.0);
    let proj = move |x: f64, y: f64| (x * kx, y * ky);
    let unproj = |p: &Point<f64>| json!([f(py_round(p.x() / kx, 5)), f(py_round(p.y() / ky, 5))]);

    // ---- sort features
    let (mut tracks, mut platforms, mut buildings, mut lands, mut station_feats): (Vec<It>, Vec<It>, Vec<It>, Vec<It>, Vec<It>) =
        Default::default();
    // nodes and ways that a route relation lists: a station among them is in service
    let route_members: FxHashSet<String> = inp
        .rels
        .iter()
        .filter(|r| r["kind"] == "route")
        .flat_map(|r| r["members"].as_array().into_iter().flatten())
        .filter_map(|m| Some(format!("{}/{}", m["type"].as_str()?, m["id"].as_i64()?)))
        .collect();
    for fe in &inp.feats {
        let Some(g) = geomx::from_geojson(&fe.g, proj) else { continue };
        let g = geomx::make_valid(g);
        if geomx::is_empty(&g) {
            continue;
        }
        let it = It { f: fe, g, modes: BTreeSet::new() };
        let (rw, pt) = (it.p("railway"), it.p("public_transport"));
        let ty = it.p("type").filter(|t| !t.is_empty());
        if ty.is_some() && fe.gtype == "LineString" && ty != Some("yard") && rw != Some("proposed") {
            tracks.push(it);
        } else if rw == Some("platform") || pt == Some("platform") {
            platforms.push(it);
        } else {
            // a way can be a station AND a building (Boston South Station): it is both, so test the station tags first and keep the
            // feature in `buildings` too (its footprint feeds the size score and the complex grouping)
            let is_station = matches!(rw, Some("station" | "halt" | "tram_stop")) || pt == Some("station");
            let is_building = it.p("building") == Some("train_station");
            let is_land = it.p("landuse") == Some("railway");
            // landuse=railway is a footprint too (Varanasi Junction is a station and railway land): the feature goes to each list it belongs to
            if is_land && (is_station || !is_building) {
                lands.push(It { f: it.f, g: it.g.clone(), modes: BTreeSet::new() });
            }
            if is_building {
                buildings.push(It { f: it.f, g: it.g.clone(), modes: BTreeSet::new() });
            }
            if is_station && rw != Some("construction") && !con_name(&it.f.p) {
                if !con_tags(&it.f.p, route_members.contains(it.id())) {
                    station_feats.push(it);
                }
            }
        }
    }
    let track_tree = Index::new(tracks.iter().map(|t| geomx::bbox(&t.g)));
    let land_tree = Index::new(lands.iter().map(|t| geomx::bbox(&t.g)));
    let bld_tree = Index::new(buildings.iter().map(|t| geomx::bbox(&t.g)));

    let nearest_track_mode = |g: &G, r: f64| -> Option<&'static str> {
        let mut best: Option<(usize, f64)> = None;
        for i in track_tree.near(g, r) {
            let d = geomx::dist(&tracks[i].g, g);
            if best.map_or(true, |(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        let (i, d) = best?;
        if d > r {
            return None;
        }
        Some(track_mode(tracks[i].p("type").unwrap_or("")))
    };
    // (modes, whether the tags state them): without a mode tag the mode comes from the nearest track, a guess that platforms may correct
    let feature_modes = |it: &It| -> (BTreeSet<String>, bool) {
        let mut ms = BTreeSet::new();
        if let Some(m) = it.p("station").and_then(mode_key) {
            ms.insert(m.to_string());
        }
        if it.p("railway") == Some("tram_stop") {
            ms.insert("tram".to_string());
        }
        for k in ["subway", "light_rail", "train"] {
            if it.p(k) == Some("yes") {
                ms.insert(k.to_string());
            }
        }
        let stated = !ms.is_empty();
        if !stated {
            ms.insert(nearest_track_mode(&it.g, 150.0).unwrap_or("train").to_string());
        }
        (ms, stated)
    };

    // ---- relation lookups
    let mut member_area: Vec<((String, i64), String)> = Vec::new(); // in relation order; a later stop area wins (dict semantics)
    let mut member_area_ix: FxHashMap<(String, i64), usize> = FxHashMap::default();
    let mut area_rows: FxHashMap<String, &Value> = FxHashMap::default();
    let mut group_rows: Vec<&Value> = Vec::new();
    for r in &inp.rels {
        match r["kind"].as_str() {
            Some("stop_area") => {
                let rid = r["id"].as_str().unwrap_or("").to_string();
                area_rows.insert(rid.clone(), r);
                for m in r["members"].as_array().into_iter().flatten() {
                    let key = (m["type"].as_str().unwrap_or("").to_string(), m["id"].as_i64().unwrap_or(0));
                    match member_area_ix.get(&key) {
                        Some(&k) => member_area[k].1 = rid.clone(),
                        None => {
                            member_area_ix.insert(key.clone(), member_area.len());
                            member_area.push((key, rid.clone()));
                        }
                    }
                }
            }
            Some("stop_area_group") => group_rows.push(r),
            _ => {}
        }
    }

    // ---- steps 1 + 2: stations from stop areas, else from station nodes
    let mut st: Vec<Station> = Vec::new();
    let mut by_area: Vec<(String, usize)> = Vec::new();
    let mut by_area_ix: FxHashMap<String, usize> = FxHashMap::default();
    for (k, it) in station_feats.iter().enumerate() {
        let sa = it.p("_stop_area").filter(|v| !v.is_empty());
        let (modes, stated) = feature_modes(it);
        if let Some(&i) = sa.and_then(|a| by_area_ix.get(a)) {
            st[i].feats.push(k);
            if stated && !st[i].stated {
                st[i].modes.clear(); // a guessed mode gives way to a stated one
            }
            if stated || !st[i].stated {
                st[i].modes.extend(modes);
            }
            st[i].stated |= stated;
            continue;
        }
        st.push(Station {
            feats: vec![k],
            modes,
            areas: sa.map(|a| BTreeSet::from([a.to_string()])).unwrap_or_default(),
            platforms: vec![],
            grouping: if sa.is_some() { "mapped" } else { "inferred" },
            pt: Point::new(0.0, 0.0),
            norm: String::new(),
            merged_from: 1,
            stated,
        });
        if let Some(a) = sa {
            by_area_ix.insert(a.to_string(), st.len() - 1);
            by_area.push((a.to_string(), st.len() - 1));
        }
    }
    for s_ in st.iter_mut() {
        let pts: Vec<Point<f64>> = s_.feats.iter().filter_map(|&k| if let Geometry::Point(p) = station_feats[k].g { Some(p) } else { None }).collect();
        s_.pt = pts.first().copied().or_else(|| geomx::rep_point(&station_feats[s_.feats[0]].g)).unwrap_or(Point::new(0.0, 0.0));
    }
    let s_tree = Index::new(st.iter().map(|x| geomx::bbox(&Geometry::Point(x.pt))));

    // platforms: mapped via stop_area first; the rest go to the station whose footprint (point + mapped platforms) is nearest and
    // within its radius, else the station whose railway land holds them
    let mut plat_station: Vec<(String, usize)> = Vec::new();
    let mut plat_station_ix: FxHashMap<String, usize> = FxHashMap::default();
    for k in 0..platforms.len() {
        let mut pm: BTreeSet<String> = ["subway", "light_rail", "train"].iter().filter(|m| platforms[k].p(m) == Some("yes")).map(|m| m.to_string()).collect();
        if pm.is_empty() {
            if let Some(m) = nearest_track_mode(&platforms[k].g, 25.0) {
                pm.insert(m.to_string());
            }
            // a surface platform with a heavy-rail track beside it is not a metro platform because a metro tunnel runs under it
            // (Changsha South: line 2 and 4 under the high-speed station): the nearest surface heavy-rail track decides
            let below = platforms[k].p("tunnel").map_or(false, |v| v != "no") || platforms[k].p("layer").map_or(false, |v| v.starts_with('-'));
            if !below && pm.iter().all(|m| m == "subway" || m == "light_rail") {
                let g = &platforms[k].g;
                let hv = track_tree.near(g, 25.0).into_iter().filter(|&i| matches!(track_mode(tracks[i].p("type").unwrap_or("")), "train") && tracks[i].p("tunnel").map_or(true, |v| v == "no") && geomx::dist(&tracks[i].g, g) <= 25.0).count();
                let under = track_tree.near(g, 25.0).into_iter().filter(|&i| matches!(track_mode(tracks[i].p("type").unwrap_or("")), "subway" | "light_rail") && geomx::dist(&tracks[i].g, g) <= 25.0).all(|i| tracks[i].p("tunnel").map_or(false, |v| v != "no"));
                if hv > 0 && under {
                    pm = BTreeSet::from(["train".to_string()]);
                }
            }
        }
        platforms[k].modes = pm;
        if let Some(&i) = platforms[k].p("_stop_area").and_then(|a| by_area_ix.get(a)) {
            st[i].platforms.push(k);
            plat_station_ix.insert(platforms[k].id().to_string(), plat_station.len());
            plat_station.push((platforms[k].id().to_string(), i));
        }
    }
    // a station whose mode is only a guess (nearest track, often the tram in front of Basel SBB) takes the modes of its own mapped
    // platforms: a main-line station with train platforms is a train station, not a tram stop
    for s_ in st.iter_mut().filter(|s_| !s_.stated) {
        let pm: BTreeSet<String> = s_.platforms.iter().flat_map(|&k| platforms[k].modes.iter().cloned()).filter(|m| matches!(m.as_str(), "train" | "subway" | "light_rail")).collect();
        if !pm.is_empty() {
            s_.modes = pm;
        }
    }
    let mut anchors: Vec<G> = Vec::new();
    let mut anchor_owner: Vec<usize> = Vec::new();
    for (i, s_) in st.iter().enumerate() {
        anchors.push(Geometry::Point(s_.pt));
        anchor_owner.push(i);
        for &x in &s_.platforms {
            anchors.push(platforms[x].g.clone());
            anchor_owner.push(i);
        }
    }
    let anchor_tree = Index::new(anchors.iter().map(geomx::bbox));
    for k in 0..platforms.len() {
        if plat_station_ix.contains_key(platforms[k].id()) {
            continue;
        }
        let pm = &platforms[k].modes;
        let Some(c) = geomx::rep_point(&platforms[k].g) else { continue };
        let cg = Geometry::Point(c);
        let mut best: Option<(usize, f64)> = None;
        for ai in anchor_tree.near(&cg, 300.0) {
            let i = anchor_owner[ai];
            if !pm.is_empty() && pm.is_disjoint(&st[i].modes) {
                continue;
            }
            let d = geomx::dist(&anchors[ai], &cg);
            if d <= max_radius(&st[i].modes) * if pm.is_empty() { 0.5 } else { 1.0 } && best.map_or(true, |(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        let mut best = best.map(|b| b.0);
        if best.is_none() {
            for li in land_tree.at(&c) {
                if geomx::contains_pt(&lands[li].g, &c) {
                    let near: Vec<usize> = s_tree
                        .near(&cg, 200.0)
                        .into_iter()
                        .filter(|&i| geomx::contains_pt(&lands[li].g, &st[i].pt) && (pm.is_empty() || !pm.is_disjoint(&st[i].modes)))
                        .collect();
                    if !near.is_empty() {
                        best = near.into_iter().map(|i| (i, geomx::dist_pt(&cg, &st[i].pt))).fold(None, |a: Option<(usize, f64)>, (i, d)| {
                            if a.map_or(true, |(_, bd)| d < bd) { Some((i, d)) } else { a }
                        }).map(|x| x.0);
                        break;
                    }
                }
            }
        }
        if let Some(b) = best {
            st[b].platforms.push(k);
            plat_station_ix.insert(platforms[k].id().to_string(), plat_station.len());
            plat_station.push((platforms[k].id().to_string(), b));
        }
    }

    // ---- step 3: merge duplicates of the same mode
    let mut uf = UF::new(st.len());
    let mut keys_of: Vec<BTreeSet<(String, String)>> = Vec::with_capacity(st.len());
    for s_ in st.iter_mut() {
        let name = s_.feats.iter().find_map(|&k| station_feats[k].p("name").filter(|n| !n.is_empty())).unwrap_or("");
        s_.norm = norm_name(name);
        let mut ks = BTreeSet::new();
        for &k in &s_.feats {
            // not railway:ref: it is a per-network abbreviation ("Ch" is Chociszew, Ciechanow, Chybie...), not an identifier
            for key in ["wikidata", "uic_ref"] {
                if let Some(v) = station_feats[k].p(key).filter(|v| !v.is_empty()) {
                    ks.insert((key.to_string(), v.to_string()));
                }
            }
        }
        keys_of.push(ks);
    }
    let mut key_index: BTreeMap<&(String, String), Vec<usize>> = BTreeMap::new();
    for (i, ks) in keys_of.iter().enumerate() {
        for k in ks {
            key_index.entry(k).or_default().push(i);
        }
    }
    // a shared id only merges stations that are also close: ids are copied between far-apart objects (uic_ref in Russia, wikidata in
    // Poland), and an unlimited union fused up to 600 stations into one
    for idxs in key_index.values() {
        for (n, &a) in idxs.iter().enumerate() {
            for &b in &idxs[n + 1..] {
                if st[a].modes.is_disjoint(&st[b].modes) {
                    continue;
                }
                let d = geomx::dist_pt(&Geometry::Point(st[a].pt), &st[b].pt);
                if d <= KEY_MERGE_RADIUS || (d <= KEY_MERGE_RADIUS_NAMED && !st[a].norm.is_empty() && st[a].norm == st[b].norm) {
                    uf.union(a, b);
                }
            }
        }
    }
    for i in 0..st.len() {
        if st[i].norm.is_empty() {
            continue;
        }
        let r = max_radius(&st[i].modes);
        for j in s_tree.near(&Geometry::Point(st[i].pt), r) {
            if j <= i || st[j].norm != st[i].norm || st[j].modes.is_disjoint(&st[i].modes) {
                continue;
            }
            if geomx::dist_pt(&Geometry::Point(st[i].pt), &st[j].pt) <= r {
                uf.union(i, j);
            }
        }
    }
    // a station area with no platforms, stop area or second feature (a building also tagged railway=station: 三次駅 beside the node 三次,
    // New York "Pennsylvania Station") joins the nearest same-mode station within 100 m of it, preferring one with platforms, when the
    // names agree once normalised or the area has no name; else (named differently) the one station with platforms inside the area
    for i in 0..st.len() {
        if !st[i].areas.is_empty() || st[i].feats.len() != 1 {
            continue;
        }
        let g = &station_feats[st[i].feats[0]].g;
        if !matches!(g, Geometry::Polygon(_) | Geometry::MultiPolygon(_)) {
            continue;
        }
        let mut cands: Vec<(bool, f64, usize)> = Vec::new();
        for j in s_tree.near(g, 100.0) {
            if j == i || st[j].modes.is_disjoint(&st[i].modes) {
                continue;
            }
            let d = geomx::dist(g, &Geometry::Point(st[j].pt));
            if d <= 100.0 {
                cands.push((st[j].platforms.is_empty(), d, j));
            }
        }
        cands.sort_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).unwrap());
        let inside: Vec<usize> = cands.iter().filter(|c| !c.0 && geomx::contains_pt(g, &st[c.2].pt)).map(|c| c.2).collect();
        let pick = cands.iter().find(|c| (st[i].norm.is_empty() && (st[i].platforms.is_empty() || st[c.2].platforms.is_empty())) || st[c.2].norm == st[i].norm).map(|c| c.2).or(if inside.len() == 1 && st[i].platforms.is_empty() { Some(inside[0]) } else { None });
        if let Some(j) = pick {
            uf.union(i, j);
        }
    }
    // a group of stations becomes one: the member with the most platforms gives the point and the name (the first such, as Python's max)
    let combine = |ms: &[&Station]| -> Station {
        let all_mapped = ms.iter().all(|m| m.grouping == "mapped");
        let any_mapped = ms.iter().any(|m| m.grouping == "mapped");
        let grouping = if all_mapped && ms.len() == 1 { "mapped" } else if ms.len() > 1 && !any_mapped { "inferred" } else { "mapped" };
        let mut seen = FxHashSet::default();
        let plats: Vec<usize> = ms.iter().flat_map(|m| m.platforms.iter().copied()).filter(|&x| seen.insert(platforms[x].id().to_string())).collect();
        let lead = ms.iter().fold(None, |a: Option<&&Station>, m| if a.map_or(true, |b| m.platforms.len() > b.platforms.len()) { Some(m) } else { a }).unwrap();
        Station {
            feats: ms.iter().flat_map(|m| m.feats.iter().copied()).collect(),
            modes: ms.iter().flat_map(|m| m.modes.iter().cloned()).collect(),
            areas: ms.iter().flat_map(|m| m.areas.iter().cloned()).collect(),
            platforms: plats,
            grouping,
            pt: lead.pt,
            norm: lead.norm.clone(),
            merged_from: ms.iter().map(|m| m.merged_from).sum(),
            stated: ms.iter().any(|m| m.stated),
        }
    };
    let mut stn: Vec<Station> = Vec::new(); // final stations
    let mut old_to_new = vec![0usize; st.len()];
    for members in groups(&mut uf, st.len()) {
        let ms: Vec<&Station> = members.iter().map(|&i| &st[i]).collect();
        for &i in &members {
            old_to_new[i] = stn.len();
        }
        stn.push(combine(&ms));
    }
    // ---- step 3b: the second dot of a station. Merged stations of one mode within SECOND_DOT_M that carry the same name once the
    // decoration is off (operator initials "Neckarsulm AVG", "Gilroy Transit Center", a place before a hyphen, a bilingual gloss, an
    // official_name's place), or a plain polygon/node that repeats a name with a Japanese operator in front or a Chinese station type
    // or exit behind ("阪急西院駅" beside "西院"), or a nameless one within SECOND_DOT_NAMELESS_M of a named one. Two stations whose
    // wikidata differ stay two when one name only carries a decoration or is missing (a different wikidata beside the same name is common: DB and S-Bahn Jungfernheide); names that merely sit close (Herald Square, PATH 33rd Street) are not the same name.
    let (stn, old_to_new) = {
        let infos: Vec<NameInfo> = stn
            .iter()
            .map(|x| {
                let mut names: Vec<String> = x.feats.iter().filter_map(|&k| station_feats[k].p("name").filter(|n| !n.is_empty()).map(String::from)).collect();
                let mut deco = BTreeSet::new();
                let mut wikidata = BTreeSet::new();
                for &k in &x.feats {
                    for key in ["operator", "operator:short", "brand", "network"] {
                        if let Some(v) = station_feats[k].p(key) {
                            operator_tokens(v, &mut deco);
                        }
                    }
                    if let Some(w) = station_feats[k].p("wikidata").filter(|w| !w.is_empty()) {
                        wikidata.insert(w.to_string());
                    }
                }
                for a in &x.areas {
                    if let Some(n) = area_rows.get(a).and_then(|r| r["tags"]["name"].as_str()).filter(|n| !n.is_empty()) {
                        names.push(n.to_string());
                    }
                }
                // "Berlin Jungfernheide" as official_name of "Jungfernheide": Berlin is a place, not part of the name
                for &k in &x.feats {
                    let toks = |t: &str| -> Vec<String> { norm_name(t).split_whitespace().map(String::from).collect() };
                    if let Some(of) = station_feats[k].p("official_name") {
                        let ot = toks(of);
                        for n in &names {
                            let nt = toks(n);
                            if !nt.is_empty() && ot.len() > nt.len() && ot.ends_with(&nt) {
                                deco.extend(ot[..ot.len() - nt.len()].iter().cloned());
                            }
                        }
                    }
                }
                let plain = x.feats.len() <= 2 && x.areas.is_empty();
                NameInfo { names, deco, wikidata, plain }
            })
            .collect();
        let tree = Index::new(stn.iter().map(|x| geomx::bbox(&Geometry::Point(x.pt))));
        let mut uf2 = UF::new(stn.len());
        let mut merged_any = false;
        for i in 0..stn.len() {
            let pi = Geometry::Point(stn[i].pt);
            let mut near_named: Option<(f64, usize)> = None; // for a nameless station: the nearest named one
            for j in tree.near(&pi, SECOND_DOT_M) {
                if j == i || stn[j].modes.is_disjoint(&stn[i].modes) {
                    continue;
                }
                let d = geomx::dist_pt(&pi, &stn[j].pt);
                if d > SECOND_DOT_M {
                    continue;
                }
                let (a, b) = (&infos[i], &infos[j]);
                let other_wd = !a.wikidata.is_empty() && !b.wikidata.is_empty() && a.wikidata.is_disjoint(&b.wikidata);
                if a.names.is_empty() && !b.names.is_empty() {
                    if other_wd {
                        continue;
                    }
                    if d <= SECOND_DOT_NAMELESS_M && near_named.map_or(true, |(bd, _)| d < bd) {
                        near_named = Some((d, j));
                    }
                    continue;
                }
                if j < i || a.names.is_empty() {
                    continue;
                }
                let deco: BTreeSet<String> = a.deco.union(&b.deco).cloned().collect();
                let (ca, cb): (Vec<String>, Vec<String>) = (a.names.iter().map(|n| core_name(n, &deco)).collect(), b.names.iter().map(|n| core_name(n, &deco)).collect());
                let same = ca.iter().any(|x| !x.is_empty() && cb.contains(x));
                let weak = |long: &NameInfo, short: &NameInfo| !other_wd && long.plain && long.names.iter().any(|l| short.names.iter().any(|s| decorated(&norm_name(l), &norm_name(s))));
                if same || weak(a, b) || weak(b, a) {
                    uf2.union(i, j);
                    merged_any = true;
                }
            }
            if let Some((_, j)) = near_named {
                uf2.union(i, j);
                merged_any = true;
            }
        }
        if merged_any {
            let mut out: Vec<Station> = Vec::new();
            let mut new_of = vec![0usize; stn.len()];
            for members in groups(&mut uf2, stn.len()) {
                let ms: Vec<&Station> = members.iter().map(|&i| &stn[i]).collect();
                for &i in &members {
                    new_of[i] = out.len();
                }
                out.push(combine(&ms));
            }
            let old_to_new: Vec<usize> = old_to_new.iter().map(|&k| new_of[k]).collect();
            (out, old_to_new)
        } else {
            (stn, old_to_new)
        }
    };
    let st = stn;
    let st_tree = Index::new(st.iter().map(|x| geomx::bbox(&Geometry::Point(x.pt))));
    let area_to_station: FxHashMap<String, usize> = by_area.iter().map(|(a, i)| (a.clone(), old_to_new[*i])).collect();
    for x in plat_station.iter_mut() {
        x.1 = old_to_new[x.1];
    }

    // ---- size inputs: track dead ends (terminus flag): ends used by exactly one mainline way
    let main_tracks: Vec<usize> = (0..tracks.len())
        .filter(|&i| tracks[i].p("railway") == Some("rail") && matches!(tracks[i].p("type"), Some("high_speed" | "intercity" | "regional")))
        .collect();
    let line_of = |i: usize| -> Option<&geo::LineString<f64>> { if let Geometry::LineString(l) = &tracks[i].g { Some(l) } else { None } };
    let key = |c: &geo::Coord<f64>| (c.x.round_ties_even() as i64, c.y.round_ties_even() as i64);
    let mut ends: FxHashMap<(i64, i64), u32> = FxHashMap::default();
    for &i in &main_tracks {
        if let Some(l) = line_of(i).filter(|l| l.0.len() >= 2) {
            *ends.entry(key(&l.0[0])).or_default() += 1;
            *ends.entry(key(l.0.last().unwrap())).or_default() += 1;
        }
    }
    let mut dead_pts: Vec<Point<f64>> = Vec::new();
    for &i in &main_tracks {
        if let Some(l) = line_of(i).filter(|l| !l.0.is_empty()) {
            for c in [l.0[0], *l.0.last().unwrap()] {
                if ends.get(&key(&c)) == Some(&1) {
                    dead_pts.push(Point::from(c));
                }
            }
        }
    }
    let dead_tree = Index::new(dead_pts.iter().map(|p| geomx::bbox(&Geometry::Point(*p))));
    let mt_tree = Index::new(main_tracks.iter().map(|&i| geomx::bbox(&tracks[i].g)));

    // ---- major station rules (docs/00-overview.md, 2026-10-11 "Major station rules")
    let main_re = inp.cfg.get("main_station_re").and_then(Value::as_str).filter(|v| !v.is_empty()).map(|p| Regex::new(p).expect("main_station_re"));
    let main_norms: BTreeSet<String> =
        inp.cfg.get("main_terminals").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(norm_name).filter(|n| !n.is_empty()).collect();
    // high-speed track by its OWN tags (a classic line that TGVs merely use does not count)
    let hs: Vec<usize> = (0..tracks.len()).filter(|&i| tracks[i].p("own_type") == Some("high_speed") && tracks[i].p("railway") != Some("construction")).collect();
    let hs_tree = Index::new(hs.iter().map(|&i| geomx::bbox(&tracks[i].g)));

    struct Size {
        mainline_platforms: i64,
        platform_count: i64,
        land_ha: f64,
        building_ha: f64,
        mainline_track_ways: i64,
        dead_ends: i64,
        terminus: bool,
        score: f64,
        tier: i64,
        hsr: bool,
        major: Option<&'static str>,
    }
    use rayon::prelude::*;
    let sizes: Vec<Size> = st
        .par_iter()
        .map(|x| {
            let mp: Vec<usize> = x.platforms.iter().copied().filter(|&k| platforms[k].modes.contains("train")).collect();
            // distinct platform numbers ("1a"/"1b" are one), plus every platform way without a ref
            let count = |ix: &[usize]| -> i64 {
                let (mut toks, mut unreffed) = (BTreeSet::new(), Vec::new());
                for &k in ix {
                    let t = ref_tokens(platforms[k].p("ref"));
                    if t.is_empty() {
                        unreffed.push(k);
                    } else {
                        toks.extend(t.iter().map(|t| plat_key(t)));
                    }
                }
                toks.len() as i64 + unreffed.len() as i64
            };
            let mainline_platforms = count(&mp);
            let platform_count = count(&x.platforms);
            // railway land and building holding the station's point or its platforms
            let mut probes = vec![x.pt];
            probes.extend(x.platforms.iter().take(60).filter_map(|&k| geomx::rep_point(&platforms[k].g)));
            let (mut land_ids, mut bld_ids) = (BTreeSet::new(), BTreeSet::new());
            for pr in &probes {
                for li in land_tree.at(pr) {
                    if geomx::contains_pt(&lands[li].g, pr) {
                        land_ids.insert(li);
                    }
                }
                for bi in bld_tree.at(pr) {
                    if geomx::contains_pt(&buildings[bi].g, pr) {
                        bld_ids.insert(bi);
                    }
                }
            }
            let land_ha = if land_ids.is_empty() {
                0.0
            } else {
                let u = geomx::union_all(land_ids.iter().map(|&i| geomx::polys(&lands[i].g)).collect());
                let local: MultiPolygon<f64> = geomx::intersection(&u, &geomx::circle(x.pt, LAND_DISC));
                if local.0.is_empty() { 0.0 } else { py_round(geomx::area(&Geometry::MultiPolygon(local)) / 10000.0, 2) }
            };
            let building_ha = py_round(bld_ids.iter().map(|&i| geomx::area(&buildings[i].g) / 10000.0).sum::<f64>(), 3);
            // mainline tracks serving the platforms (or near the point when no platforms are mapped)
            let mut near: BTreeSet<&str> = BTreeSet::new();
            let pg = Geometry::Point(x.pt);
            if !mp.is_empty() && x.modes.contains("train") {
                for &k in &mp {
                    for ti in mt_tree.near(&platforms[k].g, 20.0) {
                        if geomx::dist(&tracks[main_tracks[ti]].g, &platforms[k].g) <= 20.0 {
                            near.insert(tracks[main_tracks[ti]].id());
                        }
                    }
                }
            }
            // no platform mapped, or none beside a mainline track (Nanjing South: one platform away from the tracks): look near the point
            // (only when no platform has a heavy-rail or heritage track beside it (a metro track does not count): a heritage station next to a main line stays without passenger track)
            let platform_by_track = mp.iter().any(|&k| track_tree.near(&platforms[k].g, 20.0).into_iter().any(|ti| matches!(track_mode(tracks[ti].p("type").unwrap_or("")), "train" | "other") && geomx::dist(&tracks[ti].g, &platforms[k].g) <= 20.0));
            if near.is_empty() && x.modes.contains("train") && !platform_by_track {
                for ti in mt_tree.near(&pg, 80.0) {
                    if geomx::dist(&tracks[main_tracks[ti]].g, &pg) <= 80.0 {
                        near.insert(tracks[main_tracks[ti]].id());
                    }
                }
            }
            // terminus: at least two mainline dead ends within TERMINUS_ZONE of the station's mainline platforms or point
            // (stations.py buffers their union by 120 m and tests containment)
            let mut dead = 0i64;
            if x.modes.contains("train") {
                let zone: Vec<&G> = mp.iter().map(|&k| &platforms[k].g).collect();
                let mut cands: BTreeSet<usize> = dead_tree.near(&pg, TERMINUS_ZONE).into_iter().collect();
                for g in &zone {
                    cands.extend(dead_tree.near(g, TERMINUS_ZONE));
                }
                for di in cands {
                    let p = &dead_pts[di];
                    if geomx::dist_pt(&pg, p) < TERMINUS_ZONE || zone.iter().any(|g| geomx::dist_pt(g, p) < TERMINUS_ZONE) {
                        dead += 1;
                    }
                }
            }
            let terminus = dead >= 2;
            let score = py_round(
                W_PLATFORM * mainline_platforms as f64 + W_TRACK * (near.len() as f64).min(W_TRACK_CAP) + W_LAND * land_ha.sqrt()
                    + W_BUILDING * building_ha.sqrt() + if terminus { W_TERMINUS } else { 0.0 },
                1,
            );
            let mut tier = if score >= 60.0 { 1 } else if score >= 25.0 { 2 } else { 3 };
            let hsr = hs_tree.near(&pg, HS_NEAR).into_iter().any(|ti| geomx::dist(&tracks[hs[ti]].g, &pg) <= HS_NEAR);
            let names: Vec<&str> = x.feats.iter().map(|&k| station_feats[k].p("name").unwrap_or("")).collect();
            let mut norms: BTreeSet<String> = names.iter().map(|n| norm_name(n)).collect();
            norms.insert(x.norm.clone());
            let mut major = None;
            if x.modes.contains("train") {
                if main_re.as_ref().map_or(false, |re| names.iter().any(|n| re.is_match(n))) {
                    major = Some("name_pattern");
                } else if norms.iter().any(|n| main_norms.contains(n)) {
                    major = Some("canonical_list");
                } else if hsr && (terminus || mainline_platforms >= HS_MAJOR_PLATFORMS) {
                    major = Some("hsr_infrastructure");
                }
            }
            // a train station with no passenger track near it (only freight, yard or no mapped track) is not a major station, whatever its
            // name says (Zhuhai West): it stays on the map at the smallest size
            let no_passenger_track = x.modes.contains("train") && near.is_empty();
            if no_passenger_track {
                major = None;
                tier = 3;
            }
            if major.is_some() {
                tier = 1;
            }
            Size { mainline_platforms, platform_count, land_ha, building_ha, mainline_track_ways: near.len() as i64, dead_ends: dead, terminus, score, tier, hsr, major }
        })
        .collect();

    // ---- stop positions -> stations
    let mut node_station: FxHashMap<(String, i64), usize> = FxHashMap::default();
    for (si, x) in st.iter().enumerate() {
        for &k in &x.feats {
            let (t, n) = station_feats[k].id().split_once('/').unwrap_or(("", "0"));
            node_station.insert((t.to_string(), n.parse().unwrap_or(0)), si);
        }
    }
    for (key, sa) in &member_area {
        if let Some(&si) = area_to_station.get(sa) {
            node_station.entry(key.clone()).or_insert(si);
        }
    }
    for (pid, si) in &plat_station {
        let (t, n) = pid.split_once('/').unwrap_or(("", "0"));
        node_station.entry((t.to_string(), n.parse().unwrap_or(0))).or_insert(*si);
    }
    let mut stop_unmapped = 0usize;
    for sp in &inp.stops {
        let id: i64 = sp["osm_id"].as_str().and_then(|v| v.split_once('/')).and_then(|(_, n)| n.parse().ok()).unwrap_or(0);
        let key = ("node".to_string(), id);
        if node_station.contains_key(&key) {
            continue;
        }
        let (x, y) = proj(sp["lon"].as_f64().unwrap_or(0.0), sp["lat"].as_f64().unwrap_or(0.0));
        let pg = Geometry::Point(Point::new(x, y));
        let pm: BTreeSet<&str> = ["subway", "light_rail", "train"].into_iter().filter(|k| sp["modes"].get(*k).and_then(Value::as_str) == Some("yes")).collect();
        let nn = norm_name(sp["name"].as_str().unwrap_or(""));
        let mut best: Option<(usize, f64)> = None;
        for i in st_tree.near(&pg, 300.0) {
            if !pm.is_empty() && !pm.iter().any(|m| st[i].modes.contains(*m)) {
                continue;
            }
            let d = geomx::dist_pt(&pg, &st[i].pt);
            if d > max_radius(&st[i].modes) {
                continue;
            }
            let score = d - if !nn.is_empty() && nn == st[i].norm { 500.0 } else { 0.0 };
            if best.map_or(true, |(_, b)| score < b) {
                best = Some((i, score));
            }
        }
        match best {
            Some((i, _)) => {
                node_station.insert(key, i);
            }
            None => stop_unmapped += 1,
        }
    }

    // ---- step 4: lines per station
    let mut route_line: FxHashMap<&str, &str> = FxHashMap::default();
    for l in &inp.lines {
        for r in l["routes"].as_array().into_iter().flatten() {
            route_line.insert(r.as_str().unwrap_or(""), l["id"].as_str().unwrap_or(""));
        }
    }
    let sid_of = |i: usize| -> String { st[i].areas.iter().next().cloned().unwrap_or_else(|| station_feats[st[i].feats[0]].id().to_string()) };
    let mut st_lines: Vec<BTreeSet<String>> = vec![BTreeSet::new(); st.len()];
    let mut route_unmapped: FxHashSet<&str> = FxHashSet::default();
    let mut route_order = Map::new();
    let mut routes_total = 0;
    for r in inp.rels.iter().filter(|r| r["kind"] == "route") {
        routes_total += 1;
        let rid = r["id"].as_str().unwrap_or("");
        let lid = route_line.get(rid).copied();
        let mut order: Vec<usize> = Vec::new();
        for m in r["members"].as_array().into_iter().flatten() {
            let role = m["role"].as_str().unwrap_or("");
            if !(role.starts_with("stop") || role.starts_with("platform")) {
                continue;
            }
            match node_station.get(&(m["type"].as_str().unwrap_or("").to_string(), m["id"].as_i64().unwrap_or(0))) {
                None => {
                    route_unmapped.insert(rid);
                }
                Some(&si) => {
                    if let Some(l) = lid {
                        st_lines[si].insert(l.to_string());
                    }
                    if order.last() != Some(&si) {
                        order.push(si);
                    }
                }
            }
        }
        route_order.insert(rid.to_string(), Value::Array(order.iter().map(|&i| Value::String(sid_of(i))).collect()));
    }
    // named infrastructure lines (route=railway) that list the station as a member
    let mut st_rail: Vec<Map<String, Value>> = vec![Map::new(); st.len()];
    for r in inp.rels.iter().filter(|r| r["kind"] == "railway_line") {
        let rid = r["id"].as_str().unwrap_or("");
        let tags = r["tags"].as_object().cloned().unwrap_or_default();
        for m in r["members"].as_array().into_iter().flatten() {
            if let Some(&si) = node_station.get(&(m["type"].as_str().unwrap_or("").to_string(), m["id"].as_i64().unwrap_or(0))) {
                st_rail[si].insert(rid.to_string(), json!({"id": rid, "name": tags.get("name").cloned().unwrap_or(Value::Null), "name_en": english_name(&tags)}));
            }
        }
    }

    // ---- step 5: complexes
    let mut cuf = UF::new(st.len());
    let mut mapped_pairs = 0;
    let member_key = |m: &Value| format!("{}/{}", m["type"].as_str().unwrap_or(""), m["id"].as_i64().unwrap_or(0));
    for g in &group_rows {
        let idx: BTreeSet<usize> = g["members"].as_array().into_iter().flatten().filter_map(|m| area_to_station.get(&member_key(m)).copied()).collect();
        let idx: Vec<usize> = idx.into_iter().collect();
        for &a in idx.iter().skip(1) {
            cuf.union(idx[0], a);
            mapped_pairs += 1;
        }
    }
    let wd: Vec<BTreeSet<String>> = st.iter().map(|x| x.feats.iter().filter_map(|&k| station_feats[k].p("wikidata").filter(|v| !v.is_empty()).map(String::from)).collect()).collect();
    // the operator's Wikidata id (operator:wikidata, else brand:wikidata) of the station's features: the logo key
    let op_wd: Vec<Option<String>> = st.iter().map(|x| ["operator:wikidata", "brand:wikidata"].iter().find_map(|key|
        x.feats.iter().find_map(|&k| station_feats[k].p(key).filter(|v| !v.is_empty()).map(String::from)))).collect();
    // inferred: different modes, and shared wikidata, or the same normalised name within 300 m, or the same station building
    let mut by_bld: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, x) in st.iter().enumerate() {
        if let Some(bi) = bld_tree.at(&x.pt).into_iter().find(|&bi| geomx::contains_pt(&buildings[bi].g, &x.pt)) {
            by_bld.entry(bi).or_default().push(i);
        }
    }
    for idxs in by_bld.values() {
        for &a in &idxs[1..] {
            if st[a].modes != st[idxs[0]].modes {
                cuf.union(idxs[0], a);
            }
        }
    }
    for i in 0..st.len() {
        let pg = Geometry::Point(st[i].pt);
        for j in st_tree.near(&pg, GROUP_RADIUS) {
            if j <= i || st[j].modes == st[i].modes {
                continue;
            }
            let same_name = !st[i].norm.is_empty() && st[i].norm == st[j].norm && geomx::dist_pt(&pg, &st[j].pt) <= GROUP_RADIUS;
            if !wd[i].is_disjoint(&wd[j]) || same_name {
                cuf.union(i, j);
            }
        }
    }
    let complexes = groups(&mut cuf, st.len());

    // ---- write
    // name: a stop area's name first, else the station's best-named feature (named first, then points); stations.py iterated the
    // stop areas as a set, in an order that changes between runs; here they are sorted
    // is `cand` the name of a station within GROUP_RADIUS of x; is `c` an operator or network of x's features
    let bare_near = |x: &Station, cand: &str| -> bool {
        st_tree.near(&Geometry::Point(x.pt), GROUP_RADIUS).into_iter().any(|j| {
            geomx::dist_pt(&Geometry::Point(x.pt), &st[j].pt) <= GROUP_RADIUS
                && (st[j].feats.iter().any(|&k| station_feats[k].p("name") == Some(cand) || station_feats[k].p("name:en") == Some(cand))
                    || st[j].areas.iter().any(|a| area_rows.get(a).map_or(false, |r| r["tags"]["name"].as_str() == Some(cand))))
        })
    };
    let is_op = |x: &Station, c: &str| -> bool {
        x.feats.iter().any(|&k| ["operator", "network", "operator:short", "brand"].iter().any(|key| station_feats[k].p(key).map_or(false, |v| v.split(';').any(|t| t.trim() == c))))
    };
    let label = |x: &Station, n: &str| clean_name(n, &|c| bare_near(x, c), &|c| is_op(x, c));
    // the place that qualifies a bare "Hauptbahnhof" / "Bahnhof" / "Station" / "Gare": addr:city (or addr:place) on its own features or stop
    // area, else the place in front of the same word in the name of the nearest station within 800 m ("Bremen Hbf"). The build has no
    // place nodes at this stage, so nothing else is available; None leaves the bare name.
    let place_of = |x: &Station, kind: &str| -> Option<String> {
        let tag = |m: &Map<String, Value>| ["addr:city", "addr:place"].iter().find_map(|k| m.get(*k).and_then(Value::as_str).filter(|v| !v.is_empty()).map(String::from));
        if let Some(c) = x.feats.iter().find_map(|&k| tag(&station_feats[k].f.p)).or_else(|| x.areas.iter().find_map(|a| area_rows.get(a).and_then(|r| r["tags"].as_object()).and_then(tag))) {
            return Some(c);
        }
        let here = Geometry::Point(x.pt);
        let mut best: Option<(f64, String)> = None;
        for j in st_tree.near(&here, 800.0) {
            let d = geomx::dist_pt(&here, &st[j].pt);
            if d > 800.0 || best.as_ref().map_or(false, |b| b.0 <= d) {
                continue;
            }
            let names = st[j].feats.iter().flat_map(|&k| [station_feats[k].p("name"), station_feats[k].p("official_name")]).flatten()
                .chain(st[j].areas.iter().filter_map(|a| area_rows.get(a).and_then(|r| r["tags"]["name"].as_str())));
            if let Some(p) = names.into_iter().find_map(|n| place_before(n, kind)) {
                best = Some((d, p));
            }
        }
        best.map(|b| b.1)
    };
    // a bare kind word becomes "<place> <kind>"; without a place "Station" is no name (the caller falls back to another name), the
    // others stay as they are
    let kinded = |x: &Station, n: &str| -> Option<String> {
        match bare_kind(n) {
            None => Some(n.to_string()),
            Some(kind) => match place_of(x, kind) {
                Some(p) => Some(qualified(&p, kind)),
                None if kind == "Station" => None,
                None => Some(n.to_string()),
            },
        }
    };
    let stn_name = |x: &Station| -> (Option<String>, Map<String, Value>, Option<String>) {
        let best = x.feats.iter().copied().fold(None, |a: Option<usize>, k| {
            let rank = |k: usize| (station_feats[k].p("name").map_or(false, |v| !v.is_empty()), matches!(station_feats[k].g, Geometry::Point(_)));
            if a.map_or(true, |b| rank(k) > rank(b)) { Some(k) } else { a }
        }).unwrap();
        let own = station_feats[best].p("name").filter(|v| !v.is_empty());
        let area = x.areas.iter().find_map(|sa| {
            let r = area_rows.get(sa)?;
            Some((r["tags"]["name"].as_str().filter(|v| !v.is_empty())?, r["tags"].as_object().cloned().unwrap_or_default()))
        });
        // the station's own name, not the stop area's (a mislabelled relation named Poznań Główny "Potęgowo"); the area's name only when
        // the station has none, or when the area's name is the own name with a place in front ("Hannover Hauptbahnhof" for "Hauptbahnhof")
        // or the own name is a bare "Station"
        let own = own.filter(|o| bare_kind(o) != Some("Station") || area.is_none());
        let picked: Option<(&str, Map<String, Value>)> = match (own, area) {
            (Some(o), Some((a, at))) if a.to_lowercase().ends_with(&format!(" {}", o.to_lowercase())) => Some((a, at)),
            (Some(o), _) => Some((o, station_feats[best].f.p.clone())),
            (None, Some((a, at))) => Some((a, at)),
            (None, None) => None,
        };
        // a bare "Station" with no other name: another feature of the station that has one
        let picked = picked.filter(|(n, _)| bare_kind(&label(x, n)) != Some("Station") || place_of(x, "Station").is_some()).or_else(|| {
            x.feats.iter().find_map(|&k| station_feats[k].p("name").filter(|n| !n.is_empty() && bare_kind(n).is_none()).map(|n| (n, station_feats[k].f.p.clone())))
        });
        match picked {
            Some((src, tags)) => (kinded(x, &label(x, src)), tags, Some(src.to_string())),
            None => (None, station_feats[best].f.p.clone(), None),
        }
    };
    // name:* tags for the English label. A label in Latin script takes them only from the features and stop areas that carry its own name
    // (a stop area named "Dadar (Central)" does not give the station labelled "Dadar" its name:en); a label in another script takes all
    // of the station's name:* tags, which is where its English comes from (北京 gets "Beijing Railway Station" from its stop area)
    let name_tags = |x: &Station, src: Option<&str>| -> Map<String, Value> {
        let strict = src.map_or(false, |n| crate::rules::latin(n.as_bytes()));
        let mut t = Map::new();
        for &k in &x.feats {
            if strict && station_feats[k].p("name") != src {
                continue;
            }
            for (key, v) in &station_feats[k].f.p {
                if key.starts_with("name:") || key == "int_name" || key == "official_name" {
                    t.insert(key.clone(), v.clone());
                }
            }
        }
        for sa in &x.areas {
            if let Some(r) = area_rows.get(sa) {
                if strict && r["tags"]["name"].as_str() != src {
                    continue;
                }
                for (key, v) in r["tags"].as_object().into_iter().flatten() {
                    if key.starts_with("name:") {
                        t.insert(key.clone(), v.clone());
                    }
                }
            }
        }
        t
    };
    // {**tg, **nt}: tg's keys in order, nt's values winning, nt's new keys appended
    let merge = |mut a: Map<String, Value>, b: Map<String, Value>| {
        for (k, v) in b {
            a.insert(k, v);
        }
        a
    };
    let mut complex_of = vec![String::new(); st.len()];
    let mut cplx: Vec<(String, &Vec<usize>, usize, &'static str)> = Vec::new();
    for idxs in &complexes {
        let biggest = idxs.iter().copied().fold(None, |a: Option<usize>, i| {
            if a.map_or(true, |b| (sizes[i].score, sizes[i].mainline_platforms) > (sizes[b].score, sizes[b].mainline_platforms)) { Some(i) } else { a }
        }).unwrap();
        let gid = group_rows.iter().find(|g| g["members"].as_array().into_iter().flatten().any(|m| area_to_station.get(&member_key(m)).map_or(false, |si| idxs.contains(si))))
            .and_then(|g| g["id"].as_str()).map(String::from);
        let cid = gid.clone().unwrap_or_else(|| sid_of(biggest));
        for &i in idxs {
            complex_of[i] = cid.clone();
        }
        cplx.push((cid, idxs, biggest, if gid.is_some() { "mapped" } else { "inferred" }));
    }
    let lines_by_id: FxHashMap<&str, &Value> = inp.lines.iter().map(|l| (l["id"].as_str().unwrap_or(""), l)).collect();
    let mut stations_out = Vec::with_capacity(st.len());
    for (i, x) in st.iter().enumerate() {
        let (nm, tg, src) = stn_name(x);
        let lids: Vec<&String> = st_lines[i].iter().filter(|l| !l.is_empty()).collect();
        let mut by_type: Vec<(String, i64)> = Vec::new();
        for l in &lids {
            if let Some(t) = lines_by_id.get(l.as_str()).and_then(|l| l["type"].as_str()) {
                match by_type.iter_mut().find(|e| e.0 == t) {
                    Some(e) => e.1 += 1,
                    None => by_type.push((t.to_string(), 1)),
                }
            }
        }
        let z = &sizes[i];
        let mut rail: Vec<&Value> = st_rail[i].values().collect();
        rail.sort_by(|a, b| (a["name"].as_str().unwrap_or(""), a["id"].as_str()).cmp(&(b["name"].as_str().unwrap_or(""), b["id"].as_str())));
        let refs = |train: bool| -> Value {
            let set: BTreeSet<String> = x.platforms.iter().filter(|&&k| platforms[k].modes.contains("train") == train).flat_map(|&k| ref_tokens(platforms[k].p("ref"))).collect();
            let mut v: Vec<String> = set.into_iter().collect();
            v.sort_by_key(|r| ref_key(r));
            json!(v)
        };
        let mut sources: Vec<Value> = x.feats.iter().map(|&k| json!(station_feats[k].id())).collect();
        sources.extend(x.areas.iter().map(|a| json!(a)));
        let props = json!({
            "id": sid_of(i), "complex": complex_of[i], "name": nm, "name_en": english_name(&merge(tg, name_tags(x, src.as_deref()))).map(|e| label(x, &e)),
            "modes": x.modes.iter().collect::<Vec<_>>(), "tier": z.tier, "score": f(z.score), "terminus": z.terminus,
            "mainline_platforms": z.mainline_platforms, "platform_count": z.platform_count,
            "land_ha": f(z.land_ha), "building_ha": f(z.building_ha), "mainline_track_ways": z.mainline_track_ways,
            "dead_ends": z.dead_ends, "hsr": z.hsr, "major": z.major,
            "rail_lines": rail, "platform_refs": refs(true), "platform_refs_other": refs(false),
            "line_count": lids.len(), "line_counts": Value::Object(by_type.into_iter().map(|(k, n)| (k, json!(n))).collect()), "line_ids": lids,
            "operator_wd": op_wd[i], "wikidata": if wd[i].is_empty() { Value::Null } else { json!([wd[i].iter().next().unwrap()]) }, "grouping": x.grouping,
            "source_ids": sources,
        });
        stations_out.push(json!({"type": "Feature", "properties": props, "geometry": {"type": "Point", "coordinates": unproj(&x.pt)}}));
    }
    let mut complexes_out = Vec::with_capacity(cplx.len());
    for (cid, idxs, biggest, how) in &cplx {
        let x = &st[*biggest];
        let (nm, tg, src) = stn_name(x);
        let mut lids: BTreeSet<&String> = BTreeSet::new();
        for &i in idxs.iter() {
            lids.extend(st_lines[i].iter().filter(|l| !l.is_empty()));
        }
        let modes: BTreeSet<&String> = idxs.iter().flat_map(|&i| st[i].modes.iter()).collect();
        let props = json!({
            "id": cid, "name": nm, "name_en": english_name(&merge(tg, name_tags(x, src.as_deref()))).map(|e| label(x, &e)), "tier": idxs.iter().map(|&i| sizes[i].tier).min(),
            "terminus": idxs.iter().any(|&i| sizes[i].terminus), "station_count": idxs.len(), "modes": modes,
            "platform_count": idxs.iter().map(|&i| sizes[i].platform_count).sum::<i64>(), "line_count": lids.len(), "grouping": how,
            "station_ids": idxs.iter().map(|&i| sid_of(i)).collect::<Vec<_>>(),
        });
        complexes_out.push(json!({"type": "Feature", "properties": props, "geometry": {"type": "Point", "coordinates": unproj(&x.pt)}}));
    }
    let mut tiers: BTreeMap<i64, i64> = BTreeMap::new();
    for z in &sizes {
        *tiers.entry(z.tier).or_default() += 1;
    }
    let report = json!({
        "region": slug, "stations": st.len(), "complexes": cplx.len(), "merged_duplicates": st.iter().filter(|x| x.merged_from > 1).count(),
        "tiers": Value::Object(tiers.into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect()),
        "terminus_flagged": sizes.iter().filter(|z| z.terminus).count(),
        "stop_positions_unmapped": stop_unmapped, "stop_positions_total": inp.stops.len(),
        "routes_with_unmapped_stops": route_unmapped.len(), "routes_total": routes_total, "mapped_group_unions": mapped_pairs,
    });
    Out { stations: stations_out, complexes: complexes_out, route_order, report }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_sections_count_once() {
        assert_eq!(plat_key("1a"), "1");
        assert_eq!(plat_key("12B"), "12");
        assert_eq!(plat_key("3"), "3");
        assert_eq!(plat_key("A"), "A");
        assert_eq!(plat_key("1north"), "1north");
    }

    #[test]
    fn labels_lose_noise_only_beside_the_bare_station() {
        let near = |c: &str| ["Victoria", "池袋", "서울역", "Goregaon", "Baruth"].contains(&c);
        let op = |c: &str| c == "Elizabeth";
        let cn = |n: &str| clean_name(n, &near, &op);
        assert_eq!(cn("Bf. Dresden Hbf"), "Dresden Hbf");
        assert_eq!(cn("Dießen, Bahnhof"), "Dießen");
        assert_eq!(cn("Berlin, U Voltastraße"), "Voltastraße");
        assert_eq!(cn("Berlin, S+U Wedding"), "Wedding");
        assert_eq!(cn("Victoria (National Rail)"), "Victoria");
        assert_eq!(cn("池袋 (東武東上線)"), "池袋");
        assert_eq!(cn("서울역 기차역"), "서울역");
        assert_eq!(cn("Victoria (Elizabeth)"), "Victoria");
        // places and directions are not lines; the bare station must be near
        assert_eq!(cn("Goregaon (East)"), "Goregaon (East)");
        assert_eq!(cn("Baruth (Mark)"), "Baruth (Mark)");
        assert_eq!(cn("103rd Street (Beverly Hills)"), "103rd Street (Beverly Hills)");
        assert_eq!(cn("Adlington (Cheshire)"), "Adlington (Cheshire)");
        assert_eq!(cn("Liverpool Street (Elizabeth line)"), "Liverpool Street (Elizabeth line)");
        assert_eq!(cn("기차 부전역"), "기차 부전역");
    }

    #[test]
    fn japanese_and_german_suffixes_normalise() {
        assert_eq!(norm_name("三次駅"), norm_name("三次"));
        assert_eq!(norm_name("Bahnhof Mittweida"), norm_name("Mittweida"));
        assert_eq!(norm_name("JR宝塚駅"), norm_name("宝塚"));
        assert_eq!(norm_name("Stazione di Oristano"), norm_name("Oristano"));
    }

    #[test]
    fn english_name_uses_the_shared_rules() {
        let mut t = Map::new();
        t.insert("name".into(), json!("Hauptbahnhof"));
        t.insert("name:et".into(), json!("Waterloo metroojaam"));
        t.insert("name:etymology:wikidata".into(), json!("Q735271"));
        assert_eq!(english_name(&t), None);
        t.insert("name:en".into(), json!("Central Station"));
        assert_eq!(english_name(&t).as_deref(), Some("Central Station"));
    }

    // ---- the label, merge and construction rules of the A17 / A19 / A21 fixes

    fn feat(name: Option<&str>, lon: f64, lat: f64, extra: &[(&str, &str)]) -> Feat {
        let mut p = Map::new();
        p.insert("railway".into(), json!("station"));
        p.insert("public_transport".into(), json!("station"));
        p.insert("train".into(), json!("yes"));
        if let Some(n) = name {
            p.insert("name".into(), json!(n));
        }
        for (k, v) in extra {
            p.insert(k.to_string(), json!(v));
        }
        p.insert("osm_id".into(), json!(format!("node/{}", (lon * 1e6) as i64 + (lat * 1e6) as i64)));
        Feat { p, g: json!({"type": "Point", "coordinates": [lon, lat]}), graw: String::new(), gtype: "Point".into() }
    }

    fn model(feats: Vec<Feat>, rels: Vec<Value>) -> Vec<(Option<String>, Option<String>)> {
        let inp = Input { feats, rels, stops: vec![], lines: vec![], airports: vec![], cfg: Map::new() };
        let out = run("t", &inp);
        out.stations.iter().map(|f| (f["properties"]["name"].as_str().map(String::from), f["properties"]["name_en"].as_str().map(String::from))).collect()
    }

    fn names(feats: Vec<Feat>) -> Vec<Option<String>> {
        let mut v: Vec<Option<String>> = model(feats, vec![]).into_iter().map(|x| x.0).collect();
        v.sort();
        v
    }

    const LAT: f64 = 49.0;
    const M: f64 = 1.0 / 110540.0; // one metre of latitude in degrees

    #[test]
    fn stations_under_construction_are_not_open_stations() {
        let p = |kv: &[(&str, &str)]| -> Map<String, Value> { kv.iter().map(|(k, v)| (k.to_string(), json!(v))).collect() };
        // a pure station being built: public_transport=station (or railway=site) and a construction tag, no railway=station of its own
        assert!(under_construction(&p(&[("public_transport", "station"), ("construction", "yes")])));
        assert!(under_construction(&p(&[("public_transport", "station"), ("train", "yes"), ("construction", "station"), ("construction:railway", "station")])));
        assert!(under_construction(&p(&[("railway", "site"), ("public_transport", "station"), ("construction:railway", "halt")])));
        assert!(under_construction(&p(&[("public_transport", "station"), ("construction:public_transport", "station")])));
        // an open station that carries a future-extension tag or a leftover stays: its own railway=station|halt|tram_stop
        assert!(!under_construction(&p(&[("railway", "tram_stop"), ("public_transport", "stop_position"), ("construction:public_transport", "stop_position")])));
        assert!(!under_construction(&p(&[("railway", "station"), ("construction:public_transport", "station"), ("construction:station", "subway")])));
        assert!(!under_construction(&p(&[("railway", "station"), ("construction", "yes"), ("train", "yes")])));
        assert!(!under_construction(&p(&[("railway", "halt"), ("construction", "station")])));
        // ... or a service route (the caller passes it), or for a bare construction=yes a stop area
        assert!(!under_construction(&p(&[("public_transport", "station"), ("construction", "yes"), ("_stop_area", "relation/1")])));
        // a stop area does not clear a lifecycle tag (planned stations have one)
        assert!(under_construction(&p(&[("railway", "site"), ("construction:railway", "halt"), ("_stop_area", "relation/1")])));
        assert!(!con_tags(&p(&[("public_transport", "station"), ("construction", "yes")]), true));
        assert!(con_tags(&p(&[("public_transport", "station"), ("construction", "yes")]), false));
        for n in ["陶然桥(在建)", "龙吴路（在建）", "青龙(建设中)", "青龙（建设中）", "怀德(5号线建设中)", "Foo (under construction)", "Foo (En construcción)"] {
            assert!(under_construction(&p(&[("railway", "station"), ("name", n)])), "{n}");
        }
        assert!(!under_construction(&p(&[("railway", "station"), ("name", "Zeche Construction Road"), ("construction", "no")])));
        assert!(!under_construction(&p(&[("railway", "station"), ("name", "在建路")])));
        // through the model: a name with （在建）, a pure construction=station and an open station: only the open one is drawn; an open
        // station with construction:public_transport stays
        let f = vec![
            feat(Some("Open"), 8.0, LAT, &[]),
            feat(Some("陶然桥(在建)"), 8.01, LAT, &[]),
            feat(Some("Later"), 8.02, LAT, &[("railway", "site"), ("construction", "station")]),
            feat(Some("Crumpsall"), 8.03, LAT, &[("construction:public_transport", "stop_position")]),
        ];
        assert_eq!(names(f), vec![Some("Crumpsall".to_string()), Some("Open".to_string())]);
        // a platform beside a pure construction station does not make it open
        let mut plat = Map::new();
        plat.insert("railway".into(), json!("platform"));
        plat.insert("osm_id".into(), json!("node/1"));
        let platform = Feat { p: plat, g: json!({"type": "Point", "coordinates": [8.0, LAT + 20.0 * M]}), graw: String::new(), gtype: "Point".into() };
        let beside = vec![feat(Some("Beside"), 8.0, LAT, &[("railway", "site"), ("construction", "station")]), platform];
        assert!(names(beside).is_empty());
    }

    #[test]
    fn tidy_labels() {
        let cn = |n: &str| clean_name(n, &|_| false, &|_| false);
        assert_eq!(cn("黄福柵原駅;黄福柵原"), "黄福柵原駅");
        assert_eq!(cn("Bain Hollow;Pieter Pass"), "Bain Hollow");
        assert_eq!(cn("Meridian Union Station;Meridian"), "Meridian Union Station");
        assert_eq!(cn("Dudley Castle & Zoo [Aug 2026]"), "Dudley Castle & Zoo");
        assert_eq!(cn("Męcina [Nowa]"), "Męcina");
        assert_eq!(cn("龙吴路（在建）"), "龙吴路");
        assert_eq!(cn("Foo (under construction)"), "Foo");
        assert_eq!(cn("Foo  Bar\u{3000}Baz"), "Foo Bar Baz");
        assert_eq!(cn("Hp. Wiesenau"), "Wiesenau");
        assert_eq!(cn("Bf. Dresden Hbf"), "Dresden Hbf");
        assert_eq!(cn("Haus am See [x] "), "Haus am See");
        // brackets inside a name stay
        assert_eq!(cn("Frankfurt (Main) Hbf"), "Frankfurt (Main) Hbf");
    }

    #[test]
    fn bare_names_take_a_place() {
        assert_eq!(bare_kind("Hauptbahnhof"), Some("Hauptbahnhof"));
        assert_eq!(bare_kind(" station "), Some("Station"));
        assert_eq!(bare_kind("Hannover Hauptbahnhof"), None);
        assert_eq!(place_before("Bremen Hbf", "Hauptbahnhof").as_deref(), Some("Bremen"));
        assert_eq!(place_before("Mainz Hauptbahnhof", "Hauptbahnhof").as_deref(), Some("Mainz"));
        assert_eq!(place_before("München-Pasing Bf", "Bahnhof").as_deref(), Some("München-Pasing"));
        assert_eq!(place_before("Berlin, S Hauptbahnhof", "Hauptbahnhof").as_deref(), Some("Berlin"));
        assert_eq!(place_before("Berlin, S+U Hbf", "Hauptbahnhof").as_deref(), Some("Berlin"));
        assert_eq!(place_before("Berlin, Ostbahnhof Hbf", "Hauptbahnhof"), None);
        assert_eq!(place_before("U Bremen Hbf 2", "Hauptbahnhof"), None);
        assert_eq!(place_before("Hbf", "Hauptbahnhof"), None);
        assert_eq!(qualified("Bremen", "Hauptbahnhof"), "Bremen Hauptbahnhof");
        assert_eq!(qualified("Lyon", "Gare"), "Gare de Lyon");
        assert_eq!(qualified("Orléans", "Gare"), "Gare d'Orléans");
        // through the model: the place comes from the nearest station that names it; with none the bare name stays
        let alone = names(vec![feat(Some("Hauptbahnhof"), 8.0, LAT, &[])]);
        assert_eq!(alone, vec![Some("Hauptbahnhof".to_string())]);
        let near = names(vec![feat(Some("Hauptbahnhof"), 8.0, LAT, &[("station", "subway"), ("subway", "yes"), ("train", "no")]), feat(Some("Bremen Hbf"), 8.0, LAT + 300.0 * M, &[])]);
        assert_eq!(near, vec![Some("Bremen Hauptbahnhof".to_string()), Some("Bremen Hbf".to_string())]);
        let far = names(vec![feat(Some("Hauptbahnhof"), 8.0, LAT, &[("station", "subway"), ("subway", "yes"), ("train", "no")]), feat(Some("Bremen Hbf"), 8.0, LAT + 3000.0 * M, &[])]);
        assert!(far.contains(&Some("Hauptbahnhof".to_string())));
        // addr:city wins
        let city = names(vec![feat(Some("Bahnhof"), 8.0, LAT, &[("addr:city", "Hameln")])]);
        assert_eq!(city, vec![Some("Hameln Bahnhof".to_string())]);
    }

    #[test]
    fn bare_station_falls_back_or_is_dropped() {
        assert_eq!(names(vec![feat(Some("Station"), 8.0, LAT, &[])]), vec![None]);
        let rel = json!({"id": "relation/1", "kind": "stop_area", "tags": {"name": "Wilsonville"}, "members": []});
        let m = model(vec![feat(Some("Station"), 8.0, LAT, &[("_stop_area", "relation/1")])], vec![rel]);
        assert_eq!(m[0].0.as_deref(), Some("Wilsonville"));
    }

    #[test]
    fn english_name_of_a_latin_label_comes_from_its_own_source() {
        // Dadar: the station's own name wins, so the stop area's name:en ("Dadar (Central)") is not its English name
        let rel = json!({"id": "relation/1", "kind": "stop_area", "tags": {"name": "Dadar (Central)", "name:en": "Dadar (Central)"}, "members": []});
        let m = model(vec![feat(Some("Dadar"), 72.84, 19.02, &[("_stop_area", "relation/1")])], vec![rel]);
        assert_eq!(m[0].0.as_deref(), Some("Dadar"));
        assert_eq!(m[0].1, None);
        // a Latin name:en on the feature that gives the label still counts
        let m = model(vec![feat(Some("Wien Hbf"), 16.37, 48.18, &[("name:en", "Vienna Central")])], vec![]);
        assert_eq!(m[0].1.as_deref(), Some("Vienna Central"));
        // a label in another script takes its English from any of the station's sources (the stop area's name:en for 北京)
        let rel = json!({"id": "relation/1", "kind": "stop_area", "tags": {"name": "北京站", "name:en": "Beijing Railway Station"}, "members": []});
        let m = model(vec![feat(Some("北京"), 116.43, 39.9, &[("_stop_area", "relation/1")])], vec![rel]);
        assert_eq!(m[0].0.as_deref(), Some("北京"));
        assert_eq!(m[0].1.as_deref(), Some("Beijing Railway Station"));
    }

    #[test]
    fn the_second_dot_of_a_station_merges() {
        // an operator's initials, a transit-centre word, a Japanese operator in front, a Chinese station type behind, a place before a hyphen
        let one = |a: &str, ea: &[(&str, &str)], b: &str, eb: &[(&str, &str)], d: f64| {
            let f = vec![feat(Some(a), 8.0, LAT, ea), feat(Some(b), 8.0, LAT + d * M, eb)];
            model(f, vec![]).len()
        };
        let avg = [("operator", "Albtal-Verkehrs-Gesellschaft mbH")];
        assert_eq!(one("Neckarsulm", &[("operator", "DB InfraGO AG")], "Neckarsulm AVG", &avg, 40.0), 1);
        assert_eq!(one("Gilroy", &[], "Gilroy Transit Center", &[], 12.0), 1);
        assert_eq!(one("西院", &[], "阪急西院駅", &[], 35.0), 1);
        assert_eq!(one("森", &[], "ＪＲ北海道　森駅", &[], 13.0), 1);
        assert_eq!(one("五所川原", &[], "津軽鉄道 五所川原駅", &[], 57.0), 1);
        assert_eq!(one("诏安", &[], "诏安站站房", &[], 9.0), 1);
        assert_eq!(one("München-Pasing", &[], "Pasing Bahnhof", &[], 90.0), 1);
        assert_eq!(one("Bardowick", &[], "Bardowick Hp", &[], 80.0), 1);
        assert_eq!(one("Jungfernheide", &[("official_name", "Berlin Jungfernheide")], "Berlin Jungfernheide", &[], 37.0), 1);
        assert_eq!(one("富良野", &[], "", &[], 30.0), 1);
        // the same name once normalised, two dots
        assert_eq!(one("Regensburg Hauptbahnhof", &[], "Regensburg Hauptbahnhof", &[], 7.0), 1);
    }

    #[test]
    fn close_but_different_stations_stay_two() {
        let two = |a: &str, ea: &[(&str, &str)], b: &str, eb: &[(&str, &str)], d: f64| {
            let f = vec![feat(Some(a), 8.0, LAT, ea), feat(Some(b), 8.0, LAT + d * M, eb)];
            model(f, vec![]).len()
        };
        let sub = [("station", "subway"), ("subway", "yes"), ("train", "no")];
        assert_eq!(two("34th Street–Herald Square", &sub, "PATH 33rd Street", &sub, 60.0), 2);
        assert_eq!(two("Lexington Avenue–59th Street", &sub, "59th Street", &sub, 35.0), 2);
        assert_eq!(two("临高南", &[], "临高", &[], 9.0), 2); // a direction is another station
        assert_eq!(two("红安", &[], "红安西", &[], 88.0), 2);
        assert_eq!(two("Sky Tower, North Tram", &[("network", "Skytrain Monorail North")], "Sky Tower, South Tram", &[("network", "Skytrain Monorail South")], 80.0), 2);
        assert_eq!(two("Paignton", &[], "Paignton Queens Park", &[], 71.0), 2);
        // farther than 100 m
        assert_eq!(two("Neckarsulm", &[], "Neckarsulm AVG", &[("operator", "Albtal-Verkehrs-Gesellschaft mbH")], 130.0), 2);
        // other mode: Gare de Lyon (train) and Gare de Lyon (metro) are not merged here
        assert_eq!(two("Gare de Lyon", &[], "Gare de Lyon", &sub, 50.0), 2);
        // a nameless dot beside a named one only within 60 m
        assert_eq!(two("富良野", &[], "", &[], 90.0), 2);
        // two stations that each have their own stop area and a Japanese operator in front are two stations (JR 石山, 京阪石山)
        let f = vec![feat(Some("石山"), 8.0, LAT, &[("_stop_area", "relation/1")]), feat(Some("京阪石山"), 8.0, LAT + 66.0 * M, &[("_stop_area", "relation/2")])];
        let rels = vec![
            json!({"id": "relation/1", "kind": "stop_area", "tags": {"name": "石山"}, "members": []}),
            json!({"id": "relation/2", "kind": "stop_area", "tags": {"name": "京阪石山"}, "members": []}),
        ];
        assert_eq!(model(f, rels).len(), 2);
    }

    #[test]
    fn cores_drop_decoration_only() {
        let none = BTreeSet::new();
        assert_eq!(core_name("Berlin-Köpenick", &none), norm_name("Köpenick"));
        assert_eq!(core_name("Untertürkheim Bf", &none), norm_name("Untertürkheim"));
        assert_eq!(core_name("吐鲁番北 شىمالىي تۇرپان", &none), "吐鲁番北");
        assert_eq!(core_name("Hechingen Landesbahn", &none), "hechingen");
        assert_eq!(core_name("Hp", &none), "hp"); // all decoration: stays
        let mut d = BTreeSet::new();
        operator_tokens("Albtal-Verkehrs-Gesellschaft mbH", &mut d);
        assert!(d.contains("avg"));
        let mut d = BTreeSet::new();
        operator_tokens("Circus Circus Skytrain Monorail North", &mut d);
        assert!(!d.contains("north"));
        assert!(decorated("阪急西院", "西院") && decorated("jr 亀崎", "亀崎") && decorated("小杉駅南口", "小杉"));
        // (京阪石山 is "decorated" too; it stays a second station because it has a stop area of its own, see close_but_different_stations_stay_two)
        assert!(!decorated("西院", "阪急西院") && !decorated("临高南", "临高"));
    }
}
