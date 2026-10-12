# Prompt for Claude Design: Rail Globe, first-pass UI

Attach `docs/00-overview.md` and `docs/01-features.md` along with this prompt.

---

I'm building **Rail Globe**, a web map of passenger rail built from OpenStreetMap. Its purpose is comparison: put one city or region next to another at the same real scale and see the difference in the size of their rail systems. The attached overview and features docs are the spec. Please design a first-pass UI I can hand to a developer.

## Who uses it

Curious transit fans, urbanists and travellers. They want to see things like "how does the Bay Area's rail compare to Paris's?" They are not rail professionals, so the UI must explain itself without jargon.

## Design principles

- **Rail is the only strong colour on the screen.** The basemap is in quiet greys: faint land and water, thin major roads, city and district labels only, no points of interest.
- **The map gets the space.** Controls are compact and float over the map or sit in a slim toolbar. No heavy sidebars on desktop by default.
- **Calm and precise, not flashy.** Think a good printed atlas or a transit agency's network map, not a dashboard.
- **Desktop and phone both matter.** Design both.

## Screens to design

Use these pilot regions for sample content: **SF Bay Area, Paris (Île-de-France), Chengdu**. Use plausible placeholder numbers and mark them as placeholders.

1. **Main map view (desktop)**, showing Paris at roughly city zoom.
   - A slim toolbar with: a region picker, a colour-mode switch ("Rail type" / "Operator colours"), a toggle for construction lines, a toggle for freight and industrial track, and a button into compare view.
   - A legend for the rail types (see palette below) that can collapse.
   - Station labels in the local name. Bigger stations get more prominent labels.
   - Faint fills for station grounds and yards, to show the physical footprint of the railway.
   - Attribution: "© OpenStreetMap contributors".

2. **Same view in "Operator colours" mode.** Lines take the colours their operators use (each metro line its own colour). Where several lines share one track, show them as thin parallel strands side by side.

3. **Hover tooltips.** Compact cards that answer "what is this and how big?" at a glance. Names always show local and English together. Missing values show as "unknown" (this will be common, so make it look calm, not like an error).
   - **Station:** "成都东站 / Chengdu East", size tier with a Terminus badge where it applies, platforms (32), and lines mapped in OSM with a breakdown by type ("14: 4 commuter · 6 metro · 2 tram").
   - **Track:** rail type and usage ("Commuter · main line"), the lines running on it ("RER B, RER D, Transilien K"), electrification ("25 kV AC overhead"), max speed, gauge, number of tracks.
   - **Line** (operator-colour mode): line name with colour swatch, rail type and operator, and its two ends ("Saint-Germain-en-Laye ↔ Marne-la-Vallée").

4. **Click panels.** These hold the full lists the tooltip summarises.
   - **Station panel:** everything in the tooltip, plus every line stopping there (grouped by type, with colour swatches), platform numbers, the station complex it belongs to, and a link to OpenStreetMap.
   - **Line panel:** name, rail type, operator, colour swatch, its stations in order, and its length.
   - On a phone there is no hover: first tap shows the tooltip, a second tap or a "More" button opens the panel. Please design that interaction.

5. **Compare view (desktop).** Two maps side by side, e.g. SF Bay Area and Paris, at the same real scale. Include a visible scale bar on each to make that obvious. Zoom is linked; each map pans on its own. Under each map is a stats panel with:
   - region size (km²)
   - track km by rail type (a small bar or stacked bar works well)
   - stations by size tier
   - platforms, station ground area, yard area
   - electrified share
   - per-km² figures alongside the totals

   Show the two panels so the difference is easy to read at a glance.

6. **Phone layouts** for the main map (controls collapse into a bottom sheet or menu) and for compare view (the maps stacked or swiped, with stats in a bottom sheet). The toggles and legend must stay reachable.

## Rail type palette (the most important deliverable)

Propose a colour for each type, readable on a light grey basemap, distinguishable for common colour blindness, and ordered so the hierarchy reads at a glance:

| Type | Notes |
| --- | --- |
| High-speed | Most prominent |
| Intercity | |
| Regional | |
| Commuter / S-Bahn | |
| Metro | |
| Light rail | |
| Tram | Least prominent of the passenger types |
| Freight / industrial | Muted grey, clearly secondary |
| Yard and service track | Muted, thinner still |
| Under construction | Same colour as its type, dashed |

Also suggest line widths per type, and fill colours for station grounds and yards.

## What I'd like back

1. The screens above, desktop and phone.
2. The palette with hex values, line widths and the construction dash pattern.
3. A small component list: toolbar, toggle, legend, tooltip, station panel, line panel, stats panel, region picker, scale bar.
4. Type choices (font, sizes) for labels, panels and the toolbar.
5. Short notes on any decision you made that the spec didn't cover, so I can add it to the spec.

## Out of scope (don't design these)

Timetables, live train positions, accounts or saved views, routing, a marketing landing page, and drawing custom areas to compare. Comparison uses preset regions only.
