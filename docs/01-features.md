# Features

Six features make up v1. All of them run in the browser on static files; none needs a server.

## 1. Two colour modes

A toggle in the map toolbar switches between them. The choice is kept in the URL so a shared link opens the same way.

| Mode | What is coloured | Source | When data is missing |
| --- | --- | --- | --- |
| **By rail type** (default) | Each track, by the highest rail type that runs on it | Track tags plus route relations | Falls back to the track's own tags (see rail types) |
| **By operator colour** | Each line, in the colour its operator uses (BART lines, Paris metro lines) | `colour` on route and `route_master` relations | Type colour at lower weight; grey if no type |

On shared track, operator mode needs several colours on one track. From z12 the lines are drawn side by side (offset strands); below z12 the track takes one colour. Line ordering for strands is built in-house (no Portolan, since it needs GTFS).

## 2. Standard rail types

Every track and every line gets one type from a fixed list, so the legend means the same thing in every region. First matching rule wins.

| Type | Rule (first match wins) | Examples |
| --- | --- | --- |
| High-speed | Route `service=high_speed`, or track `highspeed=yes`, or maxspeed at or above 200 km/h. In China: maxspeed at or above 250 km/h, or 高速线 / 客专 in the name (200–249 km/h with `highspeed=yes` is Intercity; see overview) | TGV lines, Chinese HSR |
| Intercity | Route `service=long_distance`, or main-line track with long-distance services only | Intercités, Amtrak long-distance |
| Regional / commuter | Route `service=regional` or `service=commuter`; also the default for a train route with no service tag (commuter merged into regional 2026-10-12, to be split again later) | TER, Capitol Corridor, RER, Transilien, Caltrain |
| Metro / subway | `railway=subway` or `route=subway` | Paris Métro, Chengdu Metro, BART |
| Light rail / tram | `railway=light_rail`, `railway=tram`, `route=light_rail` or `route=tram` (merged 2026-10-11) | Muni Metro, VTA, Paris tram lines, Muni streetcar |
| Monorail | `railway=monorail` or `route=monorail` (own type since 2026-10-12) | SFO AirTrain |
| Funicular and cable car | `railway=funicular` or `route=funicular` (own type since 2026-10-12) | Peak Tram |
| Other | Tourist and heritage rail (`service=tourism` on a route, `usage=tourism` on track; added 2026-10-12) | |
| Freight / industrial | `usage=industrial` or `freight`, and no passenger route | Drawn muted grey; toggle to hide |
| Yard and service | `service=yard`, `siding`, `spur`, `crossover` | Drawn muted, z13 and up |

- Track with no mapped service falls back to its own tags: `highspeed` and maxspeed for high-speed, `usage=main` for intercity, `usage=branch` for regional.
- Hybrid systems take the route's `service` tag, else an entry in a small per-region override file (BART is Metro).
- The per-region commuter network lists (`commuter_networks` in the configs) are kept but unused while Commuter is merged into Regional.
- Construction lines are drawn dashed in their type's colour, with a toggle to hide them. Freight and industrial track has its own toggle. Proposed lines (Hong Kong only, for now) are in the data and shown dashed and faint when the Proposed switch is on.

## 3. Minimal basemap

The basemap stays out of the way so rail is the only strong colour. OpenFreeMap restyled in greys (Protomaps basemap as a fallback if restyling is too limited):

- Land, water and parks as faint fills
- Major roads only, thin and light
- Place labels for cities and districts only
- Faint administrative boundaries
- No points of interest and no buildings, except station buildings from our own data

## 4. Labels, tooltips and panels

Three levels of detail: labels on the map, a tooltip on hover, a panel on click. The tooltip answers "what is this and how big?" at a glance; the panel holds the full lists.

### Labels

- Map labels show the local name (`name`): 成都东站, Gare du Nord.
- Station labels give way to each other by size tier (`04-station-model.md`).

### Names in tooltips and panels

- Always the local name and the English name together.
- English comes from `name:en`; when it is missing, `int_name`, then a Latin-script `name:*`, then the local name alone. English names exist on only 33% of stations worldwide. Add automatic pinyin only if Chengdu reads badly.

### Station tooltip

| Row | Example |
| --- | --- |
| Name | 成都东站 / Chengdu East |
| Size tier | Tier 1, with a Terminus badge if flagged |
| Platforms | 32 |
| Lines mapped in OSM | 14: 4 commuter · 6 metro · 2 tram |

Line counts are shown but never set rank or zoom (ranking is by size only). They are labelled "mapped in OSM" because they run low where services are under-mapped, as in China.

### Track tooltip (hovering a piece of track)

| Row | Example |
| --- | --- |
| Rail type and usage | Commuter · main line |
| Lines on this track | RER B, RER D, Transilien K |
| Electrification | 25 kV AC overhead, or Not electrified |
| Max speed | 160 km/h |
| Gauge | 1435 mm (standard) |
| Tracks | 4 |

Missing values show as "unknown" rather than hiding the row. Expect many: max speed is missing on about a third of track, voltage on about 40%.

### Line tooltip (operator-colour mode, hovering a strand)

| Row | Example |
| --- | --- |
| Line | RER A, with colour swatch |
| Rail type and operator | Commuter · RATP / SNCF |
| Ends | Saint-Germain-en-Laye ↔ Marne-la-Vallée |

### Click panels

- **Station panel:** everything in the tooltip, plus every line stopping there (grouped by type, with colour swatches), platform numbers, the complex it belongs to, and a link to OpenStreetMap.
- **Line panel:** name, type, operator, colour, its stations in order, and its length.

### On a phone

There is no hover: the first tap shows the tooltip, a second tap (or a More button) opens the panel.

## 5. Infrastructure outlines

The physical size of the railway is part of the comparison, so it is drawn, not just the lines:

- Station grounds and yards (`landuse=railway`) as faint fills from z12
- Station buildings from z13, platforms from z14
- Yard track, sidings and spurs from z13, muted

## 6. Compare view

- Pick two regions from a preset list (the pilot regions first, using administrative boundaries). They show side by side at the same real scale: one map's zoom is offset by `log2(cos lat1 / cos lat2)`, so 1 km is the same width in both.
- Both maps use the same tiles, style and colour mode. Zoom is synced (scale-corrected); each map pans on its own.
- A stats panel under each map shows that region's size (km²) and totals: track km by rail type, stations by size tier, platforms, station ground and yard area, electrified share. Totals are also shown per km².
- Stats are computed at build time into one small JSON file per preset region (`regions/<slug>.json`).

On file size: both maps read the same PMTiles file and fetch only the tiles on screen, so a second map adds requests, not file size. Each preset adds a few KB of stats. Comparing custom drawn areas is parked.

## Area card (experiment, 2026-10-13)

The top-left card follows the view and is tied to the region picker, which names the same area (e.g. Chengdu, not the pilot region it sits in).
- Rail type and operator modes: route km as a grid of squares (1, 2 or 5 x 10^n km each, 250 at most), one row per type that switches it on the map (All / None), a "not in the total" block for being built and freight, and station dots.
- Speed mode: a track speed list with km per band; each band switches on or off on the map (`hs` in the URL). It replaces the old speed legend.
- The earlier Rail types legend now sits bottom right (type and operator modes only) while the cards are tried out. Per-band km comes from `speed` in `stats.json`.
