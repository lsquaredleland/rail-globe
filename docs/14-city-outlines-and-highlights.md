# City outlines (done), missing line highlights (research), small countries (done), 2026-10-16

Follow-ups to the search work in `docs/10-world-build.md`. Sections 1 and 3 are implemented; section 2 is findings and proposals.

## 1. City outlines (implemented 2026-10-16)

**Built (route B).** `rail-extract places` writes `<data>/city-outlines-wanted.json` (the point-only cities with their Wikidata ids; 660 of 663 have one). `pipeline/fetch_city_outlines.py` looks the relations up on Overpass, fetches the polygons from Nominatim (raw answers cached in `data/ext/city-outlines/`, so a re-run is offline) and writes `data/regions/<cc>-<name>.geojson` plus `index.json` entries with `"city": true` (real `admin_level` kept; a slug that already exists gets `-city`: `us-new-york-city`, `bg-sofia-city2`). `stats.rs` treats a `city: true` entry as a city (card, shape, `kind: city`) whatever its level; `places.rs` takes its box and slug into the search entry and drops the other places inside it. Run order: `places`, `fetch_city_outlines.py`, `build` (all countries touched, 35 s), `places` again.

**Result (world).** 445 cities got an outline (88 in the US, 40 in Germany, 38 in Japan, 30 in France, 12 in Canada, ...): 487 of the 726 search cities now have one (was 63, all in China), 239 stay points. Of the 238 point cities still wanted, 169 have no boundary relation under their Wikidata id (Dhaka, Istanbul, Mumbai, Lahore, Lagos, Lima, Cairo, Santiago, ...; a name-and-point fallback would catch some) and the rest were out of the 2 to 20,000 km2 range or at province level (Shanghai, Berlin, Tokyo, Mexico City are provinces already). City limits as decided: Boston 131 km of passenger route, Seattle 77, Chicago 467, New York 600, Toronto 363. The picked place keeps the card while it spans 15% of the screen. Two traps found on the way: New York is also a state (a "district of a city-province" rule would have dropped the city; it now applies to China's level 5 only) and Sofia has two province entries (the first run overwrote one; slugs are unique now).

### Scope as planned (kept for the reasoning)

**The gap.** `search.json` has 663 city entries that are only a point (every city outside China: New York, Boston, Chicago, ...). They can be found and flown to, and the region button names them, but they have no outline, no statistics card of their own and no "outline area in view" line; the card shows the state. Only China's prefecture-level cities (level 5) have outlines, because `outlines --level 5` and `stats.rs` handle only levels 2, 4 and 5 and the cache keeps only those.

**What OSM has.** Sampled by Wikidata id (Overpass, 20 cities): US cities are `admin_level=8` (Boston, Chicago, Los Angeles, Houston, Seattle, Philadelphia, Atlanta, Miami, Portland, Minneapolis, Newark), consolidated city-counties are 6 (San Francisco, Denver), New York is 5, Toronto 6, Montreal and Vancouver 8, Mexico City is a level 4 state. 19 of 20 had a boundary relation under the city's Wikidata id. 660 of the 663 point cities have a `wikidata` tag on their place node (the cache keeps it), so matching place to boundary by Wikidata id is reliable, with name plus "contains the point" as fallback.

**Two ways to get the polygons.**

| | A. Planet (cache) | B. Fetch script (like `outline_cn.py`) |
| --- | --- | --- |
| How | `rules::ADMIN_LEVELS` gains 6, 7, 8 for boundary relations whose Wikidata id is a place=city node's; `outlines --level 8` writes them | A Python script posts the 663 Wikidata ids to Overpass (or Nominatim `lookup`, 50 ids per request; with polygon threshold 0.0002 a city has 150 to 600 points) and writes `data/regions/<cc>-<name>.geojson` plus `index.json` entries |
| Needs | the planet again: **not on disk**, an 80 to 90 GB download, then a 54 s cache build | network, once; about 14 Nominatim requests |
| Matches the rest | yes (same planet, same simplification) | a slightly different date from the planet; fine for outlines |
| Verdict | right for the next monthly rebuild | **start here**: it produces the same files, so A can replace it later with no app or stats change |

**Work list (B first).**
1. `places.rs`: carry `wikidata` from `Place` into the city candidates; write the list of point cities (name, Wikidata id, point, country) next to `search.json`. About 30 lines.
2. `pipeline/fetch_city_outlines.py`: fetch, keep the largest part plus any part holding a station, simplify to about 20 m, write `data/regions/<country>-<city>.geojson`, add `index.json` entries with `admin_level` "8" and a `city: true` field. Skip a city when no boundary matches or the area is under 2 km² or over 20,000 km².
3. `stats.rs`: today level 5 is the city level (`lvl5`, `min_metro`). Generalise to "an index entry with `city: true`", so the card, `shapes.json` and `stats.json` areas get `kind: city` for them. Cities with no station inside are dropped, as for level 5.
4. `places.rs`: treat `city: true` outlines like level 5: the best place inside gives the name, the outline gives the box and slug (this also removes more satellite places, as Tianfu New Area was removed).
5. App: nothing new. `areaForView`, the outline layer and the card already take any area with a shape; `pickedSlug` in `App.tsx` keeps the card on a picked city.
6. Rebuild the side files, `stats` and `shapes` for the countries touched (about 40 s for all), re-run `places` (2 s).

**Size.** About 660 polygons of 150 to 600 points: roughly 3 to 5 MB more `shapes.json` over the world, 1.5 MB of it for the US; loaded per country on demand as now.

**Decisions for the user before building it.**
1. **City limits are small.** Boston (level 8) is 125 km², a fraction of its metro; Chicago, Houston and Los Angeles are large. The card would count only track and stations inside the legal city: right for "what is in this city", misleading for "the Boston rail network". Recommendation: city limits, shown as the city, with the state card one zoom out.
2. **Boroughs.** New York is one level 5 boundary; the five boroughs are level 7. One entry for New York, as now.
3. **Which cities.** All 663 point cities, or only those with real rail inside the limits (the stats step drops cities with no station inside anyway). Recommendation: all, and let the station test prune.
4. **A network step.** Overpass or Nominatim in the build makes one script depend on the network (the glyph step is the only other one). Recommendation: accept it for this script and commit its list of ids, not the polygons.

Effort: the fetch script plus the three Rust changes, about a day; route A adds the planet download.

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
