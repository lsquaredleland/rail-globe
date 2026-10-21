//! Name helpers: normalising a station name, cleaning a label, comparing two names of one station. The region-specific word lists
//! (stopwords, the Japanese operator prefixes, the Chinese station-type suffixes) are data in pipeline/config/_station-names.json.

use regex::Regex;
use rustc_hash::FxHashSet;
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::sync::LazyLock;
use unicode_normalization::{char::canonical_combining_class, UnicodeNormalization};

/// pipeline/config/_station-names.json, loaded once
struct NameData {
    stopwords: FxHashSet<String>,
    jp_operator_prefix: Regex,
    cn_suffix: Vec<String>,
    cn_exit_suffix: String,
}

static NAMES: LazyLock<NameData> = LazyLock::new(|| {
    let v: Value = serde_json::from_str(include_str!("../../../config/_station-names.json")).expect("_station-names.json");
    let list = |k: &str| -> Vec<String> {
        v[k].as_array().unwrap_or_else(|| panic!("_station-names.json: {k}")).iter().map(|x| x.as_str().unwrap().to_string()).collect()
    };
    NameData {
        stopwords: list("stopwords").into_iter().collect(),
        jp_operator_prefix: Regex::new(v["jp_operator_prefix"].as_str().expect("jp_operator_prefix")).expect("jp_operator_prefix"),
        cn_suffix: list("cn_suffix"),
        cn_exit_suffix: v["cn_exit_suffix"].as_str().expect("cn_exit_suffix").to_string(),
    }
});

/// lower case, accents off, bracketed parts and punctuation out, a trailing 站 off, stopwords out
pub(super) fn norm_name(name: &str) -> String {
    thread_local! {
        static RE: (Regex, Regex, Regex, Regex) = (
            Regex::new(r"\(.*?\)").unwrap(),
            Regex::new(r"[^\p{L}\p{N}_一-鿿]+").unwrap(),   // word characters: letters, numbers, underscore
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
        s.split_whitespace().filter(|t| !NAMES.stopwords.contains(*t)).collect::<Vec<_>>().join(" ")
    })
}

/// the English label of a property map (rules::english_name_kv: name:en, int_name, a romanised name:* for a non-Latin name; else None)
pub fn english_name(t: &Map<String, Value>) -> Option<String> {
    crate::rules::english_name_kv(t.iter().filter_map(|(k, v)| Some((k.as_bytes(), v.as_str()?.as_bytes()))))
        .and_then(|v| std::str::from_utf8(v).ok())
        .map(String::from)
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
pub(super) const CONSTRUCTION_SUFFIX: &str = r"(?i)\s*[(（]\s*(?:\d+号线)?(?:在建|在建中|建设中|建設中|under construction|en construcci[oó]n|u/c)\s*[)）]\s*$";

/// a station label cleaned of operator and format noise. Always: a leading "Bf. "/"Bhf. "/"Hp. ", a trailing ", Bahnhof", the VBB form
/// "Berlin, U Voltastraße" / "Berlin, S+U Wedding". Only when the shorter name is the name of a station within GROUP_RADIUS
/// (`bare_near`, never region-wide: "103rd Street (Beverly Hills)" keeps its place): a trailing bracket that names a line, mode or
/// operator (`is_op` = an operator or network of this station), and the Korean type words 기차역/기차 ("서울역 기차역")
pub(super) fn clean_name(name: &str, bare_near: &dyn Fn(&str) -> bool, is_op: &dyn Fn(&str) -> bool) -> String {
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
pub(super) fn bare_kind(n: &str) -> Option<&'static str> {
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
pub(super) fn place_before(name: &str, kind: &str) -> Option<String> {
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
        Some((place, "S" | "U" | "S+U" | "U+S")) => place.trim().to_string(),
        Some(_) => return None,
        None => p,
    };
    // a place is a name, not a number, a line or a qualified name ("Berlin Ost", "S+U Wedding")
    let ok = !p.is_empty()
        && p.split_whitespace().count() <= 2
        && !p.chars().any(|c| c.is_ascii_digit() || c == '+' || c == '(')
        && bare_kind(&p).is_none()
        && !lineish(&p);
    ok.then_some(p)
}

/// "Bremen" + "Hauptbahnhof" -> "Bremen Hauptbahnhof"; French: "Gare de Lyon", "Gare d'Orléans"
pub(super) fn qualified(place: &str, kind: &str) -> String {
    if kind == "Gare" {
        let v = place.chars().next().is_some_and(|c| "AEIOUHÀÉÈÎÔÂ".contains(c));
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
pub(super) fn core_name(raw: &str, deco: &BTreeSet<String>) -> String {
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
    if kept.is_empty() {
        t.join(" ")
    } else {
        kept.join(" ")
    }
}

/// the abbreviations an operator or network name holds: its upper-case words ("AVG", "SWEG") and the initials of its words without the
/// company form ("Albtal-Verkehrs-Gesellschaft mbH" is "avg"). Whole words are not taken: "North" in a network name is no decoration.
pub(super) fn operator_tokens(v: &str, out: &mut BTreeSet<String>) {
    let words: Vec<&str> = v.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    out.extend(words.iter().filter(|w| (2..=5).contains(&w.chars().count()) && w.chars().all(|c| c.is_uppercase())).map(|w| w.to_lowercase()));
    let initials: String = words
        .iter()
        .map(|w| w.to_lowercase())
        .filter(|w| {
            !matches!(w.as_str(), "mbh" | "gmbh" | "ag" | "e" | "v" | "kg" | "co" | "ltd" | "llc" | "plc" | "sa" | "srl" | "spa" | "the" | "of" | "and" | "und")
        })
        .filter_map(|w| w.chars().next())
        .collect();
    if initials.chars().count() >= 2 {
        out.insert(initials);
    }
}

/// a Japanese operator in front of a station name ("阪急西院駅", "名古屋鉄道知多半田駅", "JR 亀崎駅", "津軽鉄道 五所川原駅")
fn jp_operator_prefix(p: &str) -> bool {
    NAMES.jp_operator_prefix.is_match(p)
}

/// a Chinese station type or exit after the name ("诏安站站房", "寿县高铁站", "科宁路地铁站3号口", "中央商务区G2口")
fn cn_suffix(x: &str) -> bool {
    let core = x.trim_matches('站');
    core.is_empty() || NAMES.cn_suffix.iter().any(|s| s == core) || x.ends_with(&NAMES.cn_exit_suffix)
}

/// `long` is `short` plus a Japanese operator in front or a Chinese station type / exit behind it, spaces ignored (normalised names)
pub(super) fn decorated(long: &str, short: &str) -> bool {
    let (l, s): (String, String) = (long.split_whitespace().collect(), short.split_whitespace().collect());
    !s.is_empty() && l.len() > s.len() && ((l.ends_with(&s) && jp_operator_prefix(&l[..l.len() - s.len()])) || (l.starts_with(&s) && cn_suffix(&l[s.len()..])))
}
