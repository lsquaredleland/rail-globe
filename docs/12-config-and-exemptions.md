# Config layering and exemptions

Added 2026-10-15. Per-region rules live in `pipeline/config/<slug>.json`. This page says how config files build on each other (`extends`) and how a region records a justified exception to the generic rules (`exemptions`). Code: `pipeline/rail-extract/src/config.rs` (layers), `classify.rs` (exemptions).

## Layers

A config file may start with `"extends": "<name>"`, where `<name>` is another file in `pipeline/config/` without `.json` (for example `_default`, `_north-america`). The parent is resolved first (it may extend another), then the file is merged on top. The chain, lowest to highest:

1. `_default.json` - only for a region with **no file of its own** (name, local name and country are filled from the outline, as before), or for a file that says `"extends": "_default"` itself or through a parent.
2. the `extends` chain, root first (`_north-america.json` extends `_default`; `us.json` and `ca.json` extend `_north-america`).
3. the region's own file.

A file of its own that says `extends` still gets `stat_name`, `stat_local` and `stat_auto.country` filled from the outline when it leaves them out (explicit values win; `fill_default` in `site.rs`), so a bare `{"extends": "_default"}` file keeps its card name and its province/city areas.

A file without `extends` gets nothing beneath it: the hand-written pilot configs (`paris`, `sf-bay-area`, `chengdu`) are unchanged. (An implicit `_default` under every file would have added `stat_auto` to them and changed their statistics.)

Missing parent, a cycle (`a -> b -> a`), a bad `+key` and an unknown key (see "Config keys") stop the run with a message naming the file.

### Merge rules

| Value in the child | Result |
| --- | --- |
| scalar (string, number, bool, `null`) | replaces the parent's |
| object | merged key by key, recursively |
| array under `"key"` | **replaces** the parent's array |
| array under `"+key"` | **appends** to the parent's `key` (becomes `key` if the parent has none) |

`extends` itself is consumed. `"+key"` must be an array. Order matters where rules are "first match wins" (`type_overrides`, `track_type_overrides`): appended entries come after the parent's.

```json
{ "extends": "_north-america",
  "stat_name": "Canada",
  "stat_auto": { "country": "CA" },
  "+track_type_overrides": [ { "match": { "name_re": "^GO Transit.*Weston Subdivision" }, "type": "regional" } ] }
```

`stat_auto` merges with the parent's `{"levels": ["4"]}`, so Canada keeps `levels` and adds `country`.

## Config keys

Top-level keys a region config may use (after merging; a `+key` counts as `key`). Every file of an `extends` chain is checked against the list `KNOWN_KEYS` in `config.rs` (one comment per key); an unknown key, a typo or a key nothing reads any more, stops the run with the file and the key (`cn.json: unknown key "stat_nmae"`). `extends` is consumed. `_groups.json` and `_station-names.json` are not region configs and have their own formats. Never keep a key "for later": remove it when its code goes (`commuter_networks`, `freight_name_re`, `freight_name_per_way` and `freight_operator_exempt_re` were removed 2026-10-20; nothing had read them since 2026-10-12 and 2026-10-15).

| Key | Type | Read by | Example |
| --- | --- | --- | --- |
| `_comment` | string | nothing (notes for people) | `"All of Mexico ..."` |
| `china` | bool | `classify.rs` (250 km/h high-speed floor), `tiles/` (China-only styling) | `true` |
| `exemptions` | list of objects | `classify.rs` (see Exemptions below) | `[{"id": "tren-maya-one-class", ...}]` |
| `high_speed_min_kmh` | number | `stats.rs` writes it to `stats.json` as `hs_min_kmh`; default 200 | `200` |
| `high_speed_networks_re` | list of regex | `classify.rs`: route network or name makes the route High-speed | `["^TGV", "^Eurostar"]` |
| `high_speed_track_overrides` | list of match rules | `classify.rs`: promote tracks to High-speed (match rules as in `type_overrides`, without `type`) | `[{"osm_id": "way/123"}]` |
| `main_station_re` | regex | `stations/`, `construction.rs`: names that are always major stations | `"^北京(东\|西\|南\|北)?站?$"` |
| `main_terminals` | list of names | `stations/`: terminals that count as major | `["Gare de Bercy"]` |
| `passenger_needs_evidence` | `true`, `"routes"` or `"stations"` | `classify.rs` (docs/15) | `"routes"` |
| `passenger_operator_re` | list of regex | `classify.rs`: operators that count as passenger evidence | `["(?i)Amtrak\|Metra"]` |
| `passenger_stations_file` | file name | `classify.rs`: text file beside the config, one passenger station name per line; with it the list decides what is a passenger station | `"cn-passenger-stations.txt"` |
| `proposed_region` | file name | `classify.rs`: outline in `pipeline/regions` inside which proposed track is kept | `"cn/hk.json"` |
| `stat_areas` | list of `{slug, name, local}` | `stats.rs`: extra areas for the region card (each needs an outline) | `[{"slug": "guangzhou", "name": "Guangzhou"}]` |
| `stat_auto` | object (`country`, `levels`, `min_metro`, `exclude`) | `stats.rs`, `site.rs` (fills `country`): areas picked from `data/regions/index.json` | `{"country": "CN", "levels": ["4", "5"]}` |
| `stat_local` | string | `stats.rs`, `site.rs`: region name in its own language | `"中国"` |
| `stat_name` | string | `stats.rs`, `site.rs`: region name | `"China"` |
| `track_type_overrides` | list of match rules + `type` | `classify.rs`: retype track | `[{"match": {"name_re": "^朔黄线$"}, "type": "freight"}]` |
| `type_overrides` | list of match rules + `type` | `classify.rs`: retype route lines | `[{"match": {"name_re": "市域铁路"}, "type": "regional"}]` |
| `unrouted_main_type` | type name | `classify.rs`: type of main track on no route, default `intercity` | `"regional"` |

### China configs

`_china.json` holds the China rules (`china`, 市域铁路 as Regional, 旅游|观光 as Other, the passenger-station list). It does not extend `_default` (that would add `stat_auto` to the pilots). `cn.json` (world build) extends it and adds the long `main_station_re`, the freight `track_type_overrides`, `proposed_region` and the statistics keys; `china.json` (the pilot on the hand-made outline) is just `"extends": "cn"`, so the two cannot drift apart again. `chengdu`, `greater-bay-area` and `cn-beijing` extend `_china` and keep only their own `stat_name`, `main_station_re` and (Greater Bay Area) `proposed_region`.

## Exemptions

An exemption is one named, justified exception to the generic rules, kept in the region's `exemptions` list. Use it when a specific line is wrong under the generic rules and the rules should not change for everyone else (a Metro-North line typed Freight, a scenic line typed Regional). `type_overrides` and `track_type_overrides` keep working as before; they are not exemptions.

```json
{
  "id": "metro-north-lines",
  "match": { "name_re": "^(Hudson|Harlem|New Haven) Line$", "operator_re": "^Metro-North Railroad$", "type": "freight" },
  "action": { "type": "regional" },
  "reason": "Commuter lines typed freight by the no-station test because the stations are not within 200 m.",
  "source": "https://en.wikipedia.org/wiki/Metro-North_Railroad",
  "added": "2026-10-15",
  "expect_min": 30
}
```

| Key | Meaning |
| --- | --- |
| `id` | Short, unique in the file. Shows up in the report and the warnings. Required |
| `match` | At least one of `osm_id` (one id or a list), `osm_ids` (list), `name_re`, `operator_re`, `network_re`, `ref` (exact), `network` (exact). All given keys must match. Optional `type`: only features the rules have already typed this way (`freight`, `regional`, ...). Optional `type_source`: only tracks whose type came from this rule (`no-evidence`, `no-station`, `tags`, `route`, `name`, ...; lines are never matched), to touch the ways a rule demoted and not ways mapped as freight. `type` and `type_source` alone are not enough |
| `action` | Exactly one of `{"type": "<rail type>"}` (`high_speed`, `intercity`, `regional`, `metro`, `light_rail`, `monorail`, `funicular`, `other`, `freight`), `{"freight": true}` (type Freight), `{"freight": false}` (a Freight way goes back to the type its own tags give, Regional if they also say freight; other ways are untouched), `{"drop": true}` (the way is left out of `features.typed.geojsonl`) |
| `reason` | Required. Why the generic rules are wrong here |
| `source` | Optional URL, issue or note |
| `added` | Optional date, `YYYY-MM-DD` |
| `expect_min` | Default 1. How many features it must match; fewer makes it stale |

Ids are OSM ids in the form kept on every feature: `way/123`, `relation/456` (a bare number is an error). They survive monthly rebuilds, so an `osm_id` exemption lasts until the way is deleted or split in OSM (then it goes stale, below).

### How they apply

- After all the other rules, including the neighbour smoothing, so they win. In file order: a later exemption that matches overrides an earlier one. `match.type` is tested against the type at that moment.
- To track ways: every action. To lines (route masters, else routes): `type` only; a line's `osm_id` is its `relation/N` id, and its `type_source` becomes `exemption`.
- A track way changed by an exemption has `type_source` = `exemption`.
- `drop` applies to track ways only, not to stations or areas.

### How to add one

1. Find the feature in the cut output (`data/<slug>/features.typed.geojsonl`) and confirm its `type`, `type_source` and tags; check the line really is what you think (OSM, an operator map).
2. Add the entry to `pipeline/config/<slug>.json` (`"exemptions": [...]`, or `"+exemptions"` to add to a parent's). Match as narrowly as you can: operator plus name, plus `"type": "freight"` when only the wrongly typed ways should change.
3. `rail-extract cut <slug>`; look at `classify_report.json` -> `exemptions` for the count and km, and at the before/after type counts.
4. Run the data checker (below).

### Stale exemptions and reporting

`classify_report.json` gets an `exemptions` section (only when the config has any):

```json
"exemptions": { "metro-north-lines": { "matched": 37, "tracks": 37, "lines": 0, "km": 19.9, "expect_min": 30, "expect_min_met": true } }
```

`matched` = tracks + lines (every way/line the exemption's match selected, whichever exemption won). An exemption that matched fewer than `expect_min` is stale: OSM changed the feature or the generic rules now get it right. `cut` and `build` print `WARNING: <slug>: stale exemption "<id>" matched N features, expected at least M`. `cut --strict-exemptions` also exits non-zero when any is stale (the outputs are written first). Default: warn only. Fix a stale one by updating the match, or delete it if the rules now agree.

### Checking

```
python3 pipeline/check.py --exemptions [--data DIR] [slug ...]
```

Reads `<DIR>/<slug>/classify_report.json`. Without `--data` (2026-10-17) a world slug is read from `data/world/<slug>` and the pilots from `data/<slug>`, and the default slug list is every region in `data/world` plus the pilots; the legacy top-level `data/{br,ca,...}` folders are ignored. A newly added exemption shows as MISSING until `data/world/<slug>` is re-cut, prints one line per exemption and exits 1 if any is stale. The normal `python3 pipeline/check.py` (answer lists) also prints and counts them for its regions.

## Current use

`pipeline/config/us.json`: four exemptions for Amtrak's New York and Washington terminal tracks, Metro-North, LIRR and the NJ Transit Atlantic City Line, all `match.type: freight` so only wrongly typed ways change. This is a demonstration of the mechanism, not a fix for the US freight problem (see `07-freight-detection.md`).


## Group templates (`_groups.json`)

A region with no `pipeline/config/<slug>.json` normally uses `_default.json`. `pipeline/config/_groups.json` maps a template name to a list of slugs (`{"_evidence-routes": ["ar", "za", ...]}`); a listed region uses `<name>.json` (a template that `"extends": "_default"`) instead, with its names filled from the outline like the default. One file for 41 countries instead of 41 files (`docs/15-argentina-africa-freight.md`). A region's own file still wins.
