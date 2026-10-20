# Operator logos (one-time pull)

Logos of the railway operators named in OSM (`operator:wikidata`, else `brand:wikidata`), for station and line labels.

**How `app/static/logos/` is made.** The 672 PNGs and `logos.json` there are committed (4.4 MB). They are NOT rebuilt by `rail-extract`, `publish.sh` or the world build; only the three steps below make them, by hand (needs the `osmium` CLI, Python 3, Node 20+ and network; about 10 minutes). Re-run only to add new operators. The pipeline only reads `logos.json`, to decide which stations get a logo (`tiles.rs`). `squarer.mjs` edits the committed PNGs in place: run it after step 3 and commit the result.

No licence check was made: logos are trademarks and some Commons files are non-free.

```bash
# 1. (needs osmium and a rail-only world extract; not part of the Rust pipeline) Features that carry an operator Wikidata id, from the world rail extract (7 s)
osmium tags-filter railways-world.osm.pbf nwr/operator:wikidata nwr/brand:wikidata -f opl -o ops.opl --overwrite
# 2. Distinct ids with counts on stations (st) and lines/routes (ln) -> operators.json (names come out percent-garbled; only the ids matter)
python3 -I pipeline/logos/operators.py ops.opl operators.json
# 3. Wikidata (P8972 small logo, else P154 logo) -> Commons PNG, 96 px: app/static/logos/<qid>.png + logos.json (6 min, polite rate)
node pipeline/logos/pull-logos.mjs operators.json
```

Result (planet 2026-09-28): 1208 operator ids, 672 with a Wikidata logo, 671 downloaded. 282 of the 442 ids seen on stations have one, covering 33,802 of 50,067 tagged stations. 
Then `node pipeline/logos/squarer.mjs` (2 min): for the 381 logos wider than 2:1 it looks for a squarer one among the operator's own logo claims (SVG or PNG only) and, for subsidiaries of MTA, Deutsche Bahn and TfL, the parent's. 37 replaced (LIRR and Metro-North now show the round MTA mark; DB Netz, DB Regio and S-Bahn Hamburg the DB logo). About 344 wide wordmarks remain.

## Where the logos are used

- Station model (`stations.rs`) keeps `operator_wd`, the station's own `operator:wikidata` (else `brand:wikidata`); lines.json (`classify/`) keeps the same per line. Both tags are in the cache (`rules.rs`), so the cache was rebuilt once.
- `tiles.rs` picks a logo per station, only from operators that have a file in `app/static/logos/logos.json`: the station's own operator, else the commonest operator of the lines serving it, else China Railway (Q1073489) for a train station in a region whose config has `"china": true`. It writes `lg` on the station tiles and `logo` in the station side file.
- App: the station card shows the logo before the name; a `logo` layer draws it above tier 1 and 2 stations from z9 (image fetched on first use, `addBadgeImages` in `mapStyle.ts`).
- Pilot result: 10,822 of 18,899 China stations, 359 of 876 Paris, 84 of 274 SF Bay Area, 3,386 of 8,911 Germany have a logo.
