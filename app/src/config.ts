// Station draw distance. Each tier has the zoom where its dot appears and the zoom where its name appears.
// Higher number = appears only when zoomed in closer; lower = from further out.
// Tiles hold tier 1/2/3 stations from z6/z7/z9 and names from z6/z8/z10 (tiles.rs asks for z5 for tier 1, but the writer leaves stations and complexes out of z0-z5 tiles), so
// values below those need a tile rebuild. Tier 1 = major, 2 = medium, 3 = minor (docs/04-station-model.md).
export const STATION_ZOOM: Record<1 | 2 | 3, { dot: number; label: number }> = {
  1: { dot: 6, label: 8 },
  2: { dot: 8, label: 9 },
  3: { dot: 10, label: 11 },
}

/** Where the pipeline's output (tiles and side files) is served from: VITE_DATA_BASE (the R2 public URL) in a deployed build,
 *  else the app's own data/ folder (dev server, local preview). Always ends in a slash. */
const envBase = import.meta.env.VITE_DATA_BASE as string | undefined
const dataRoot = envBase ? envBase.replace(/\/?$/, '/') : new URL('data/', document.baseURI).href

/** The map shows the world build: world/world.pmtiles and the country side files in data/world/ (written by `rail-extract world`). */
export const DATA_BASE = `${dataRoot}world/`

/** Map label glyphs (SDF fonts, only the characters the map can draw), written by pipeline/glyphs: gitignored, so a deployed build reads them from the data bucket's glyphs-v2/ folder (glyphs/ is the old pilot set, kept for rollback) (VITE_DATA_BASE), else the app's own glyphs-v2/ folder. MapLibre
 *  wants an absolute URL with its {fontstack} and {range} placeholders left as they are. */
/** Operator logos (pipeline/logos): one PNG per operator Wikidata id, in the app's own logos/ folder. */
export const logoUrl = (qid: string) => `${new URL('logos/', document.baseURI).href}${qid}.png`
export const GLYPHS_URL = `${envBase ? dataRoot : new URL('.', document.baseURI).href}glyphs-v2/{fontstack}/{range}.pbf`
