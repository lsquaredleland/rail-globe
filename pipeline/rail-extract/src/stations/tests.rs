use super::features::{con_tags, under_construction};
use super::names::{bare_kind, clean_name, core_name, decorated, english_name, norm_name, operator_tokens, place_before, qualified};
use super::run;
use super::size::plat_key;
use crate::site::{Feat, Input};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

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
    for n in ["陶然桥(在建)", "龙吴路（在建）", "青龙(建设中)", "青龙（建设中）", "怀德(5号线建设中)", "Foo (under construction)", "Foo (En construcción)"]
    {
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
    let near = names(vec![
        feat(Some("Hauptbahnhof"), 8.0, LAT, &[("station", "subway"), ("subway", "yes"), ("train", "no")]),
        feat(Some("Bremen Hbf"), 8.0, LAT + 300.0 * M, &[]),
    ]);
    assert_eq!(near, vec![Some("Bremen Hauptbahnhof".to_string()), Some("Bremen Hbf".to_string())]);
    let far = names(vec![
        feat(Some("Hauptbahnhof"), 8.0, LAT, &[("station", "subway"), ("subway", "yes"), ("train", "no")]),
        feat(Some("Bremen Hbf"), 8.0, LAT + 3000.0 * M, &[]),
    ]);
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
    assert_eq!(
        two("Sky Tower, North Tram", &[("network", "Skytrain Monorail North")], "Sky Tower, South Tram", &[("network", "Skytrain Monorail South")], 80.0),
        2
    );
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
