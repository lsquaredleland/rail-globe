# City outlines (done), missing line highlights (research), small countries (done), 2026-10-16

Follow-ups to the search work in `docs/10-world-build.md`. Sections 1 and 3 are implemented; section 2 is findings and proposals.

## 1. City outlines (implemented 2026-10-16)

**Built from the local planet (route A).** Pass 0 of the cache build (`build.rs`, `city_wikidata`) reads the node blobs' tags and collects the Wikidata ids of all 11,443 `place=city` nodes (24 s); pass 1 then keeps every boundary relation of admin levels 6 to 9 whose Wikidata id is one of them (`rules::city_boundary`), next to the countries, provinces and level-5 cities it kept before. The cache build takes 68 s (was about 55 s) and the file grew from 1,207 to 1,226 MB. `rail-extract places` writes `<data>/city-outlines-wanted.json` (the cities that are points or have a generated outline, with their Wikidata ids); `rail-extract outlines --cities <that file>` picks, per id, the boundary of the highest admin level 5 to 9, finds its country like a province's, skips polygons under 2 km2 or over 20,000 km2 and writes `data/regions/<cc>-<name>.geojson` plus an `index.json` entry with `"city": true` and the city's Wikidata id (the real `admin_level` stays; the name is the place node's English name, since a relation's `name:*` fallback gave `Statutarstadt` and `Municipiul`; a slug taken already gets `-city`, `-city2`; an earlier entry for the same relation is replaced). `stats.rs` treats a `city: true` entry as a city (card, shape, `kind: city`) whatever its level; `places.rs` takes its box and slug into the search entry and drops the other places inside it. Run order after a new planet: `cache`, the usual `cut`/`build`, `places`, `outlines --cities`, `build` (all countries, 35 s), `places`. 0.4 s for the outlines step.

A first version fetched the polygons from Overpass and Nominatim (a Python script, since removed): the cache route gives the same 445 relations plus two more (Buffalo, Sarlerua), with no network. I wrongly said the planet was not on disk; it is `planet-260928.osm.pbf` in the repository root.

**Result (world).** 446 cities got an outline (88 in the US, 40 in Germany, 38 in Japan, 30 in France, 12 in Canada, ...): 487 of the 726 search cities now have one (was 63, all in China), 239 stay points. 212 of the 660 cities asked for have no boundary of level 5 to 9 under their Wikidata id (Dhaka, Istanbul, Mumbai, Lahore, Lagos, Lima, Cairo, Santiago, ...: the place node and the boundary carry different ids, or the boundary is a province: Shanghai, Berlin, Tokyo, Mexico City). A match by name and by the place's point inside the boundary would catch some; it needs the cache to keep boundaries without a Wikidata match, which is large. City limits as decided: Boston 131 km of passenger route, Seattle 77, Chicago 467, New York 600, Toronto 363. The picked place keeps the card while it spans 15% of the screen. Two traps found on the way: New York is also a state (a "district of a city-province" rule would have dropped the city; it now applies to China's level 5 only) and Sofia has two province entries (the first run overwrote one; slugs are unique now).

### Decisions made (user, 2026-10-16)
City limits, not metro areas (Boston is 131 km of passenger route inside its limits); one entry for New York; all cities with a boundary, the station test prunes; the local planet, not Overpass.

## 2. Why some lines have no highlight (research)

**How a hover highlights.** A track piece in a tile carries `ln` (its line ids) or, if it is on no line, its name `n` (railway number `nk` in the overview). Hover looks up the line's `hl` (track pieces and a simplified geometry) in `lines/<id>.json`, or the railway's pieces in `name-hl/<shard>.json`. A piece with neither has nothing to highlight.

**Findings** (open track km, not yard, world build):

| Country | on a mapped line | named only | **no line and no name** |
| --- | --- | --- | --- |
| Germany | 85.7% | 5.7% | **8.6%** (4.3% without freight) |
| Italy | 65.3% | 27.3% | **7.4%** |
| United States | 23.9% | 70.5% | **5.7%** (44% of it freight) |
| France | 89.0% | 8.7% | 2.4% |
| China | 14.9% | 82.6% | 2.5% |

1. **Bare track.** 2 to 9% of the km has no route relation and no name, so there is nothing to look up. In Germany 74% of it has a `ref`, in France 76%, in Italy 17%. In a Munich view at z9, 385 of 800 Intercity pieces are bare. Fix options by cost: (a) highlight by `ref` plus operator as a third key, like the named railways; (b) group connected bare pieces of one type into a "stretch" and highlight the stretch; (c) accept it for freight and yard, where nobody asks.
2. **Lines with no pieces.** 17% of Italy's lines (88 of 509), 4% of France's and 1% of Germany's have an empty `hl.ways`: the route relation has no rail ways in the data (0 km: former tracks, tramways never mapped as rail, proposals, members dropped as yard or stray). They are listed in "Lines" and cannot be highlighted. Fix: do not write lines with 0 km, or flag them.
3. **Border attribution** (found testing the world build). A track piece is mapped to its country's side files from where it lies (`regionOf`, the first point of the tile geometry), so a line near a border can be asked of the wrong country: about 14 requests like `de/lines/7843286.json` for Dutch lines (404, no highlight). Cause 1, **fixed 2026-10-16**: country outlines load lazily now, and the highlight prefetch ran before they were in; it waits for `warmParts` now. Cause 2, **open, intermittent**: pieces whose first point lies across the border. Fix: on a 404, try the other countries whose bounds hold the point (line ids are relation ids, unique worldwide).

Recommended order: border fallback (small), drop empty lines (small), `ref` highlight (medium: `tiles.rs` and the side files).

## 3. Small countries are country level only (done)

Rule (user, 2026-10-16): a country with under 3,000 km of rail and no city entry in `search.json` is listed without provinces. Implemented in `places.rs` (`SMALL_COUNTRY_KM`): "rail" is the open passenger route km of the country's `stats.json` (everything but freight and other); a city counts when its outline slug starts with the country's, or its point lies inside the country outline. World: 57 countries are country-only (for example New Zealand, Iraq, Sri Lanka, Uganda, Zambia, Zimbabwe, Angola, Mozambique and Namibia: none has a metro or light rail city), 304 provinces fewer: 1,370 left (was 1,674); `search.json` 255 KB (was 295 KB), 92 KB gzipped. Not applied to the pilot (its regions are not whole countries), whose `search.json` is unchanged.
