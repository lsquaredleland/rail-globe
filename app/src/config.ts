// Station draw distance. Each tier has the zoom where its dot appears and the zoom where its name appears.
// Higher number = appears only when zoomed in closer; lower = from further out.
// Tiles hold tier 1/2/3 stations from z5/z7/z9 and names from z5/z8/z10 (pipeline/rail-extract/src/tiles.rs), so
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

/** Which build of the pipeline the map shows: `pilot` (default; pilot.pmtiles in data/), or `world` with VITE_TILE_SOURCE=world (world.pmtiles
 *  and the country side files in data/world/, written by `rail-extract world`). Dev switch for looking at the world build; the app does
 *  not offer it to users. */
export const TILE_SOURCE = import.meta.env.VITE_TILE_SOURCE === 'world' ? 'world' : 'pilot'
export const DATA_BASE = TILE_SOURCE === 'world' ? `${dataRoot}world/` : dataRoot

/** Map label glyphs (SDF fonts, only the characters the map can draw): the app's own glyphs/ folder, written by pipeline/glyphs. MapLibre
 *  wants an absolute URL with its {fontstack} and {range} placeholders left as they are. */
export const GLYPHS_URL = `${new URL('glyphs/', document.baseURI).href}{fontstack}/{range}.pbf`
