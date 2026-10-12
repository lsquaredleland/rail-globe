// Station draw distance. Each tier has the zoom where its dot appears and the zoom where its name appears.
// Higher number = appears only when zoomed in closer; lower = from further out.
// Tiles hold tier 1/2/3 stations from z5/z7/z9 and names from z5/z8/z10 (pipeline/build_tiles.py), so
// values below those need a tile rebuild. Tier 1 = major, 2 = medium, 3 = minor (docs/04-station-model.md).
export const STATION_ZOOM: Record<1 | 2 | 3, { dot: number; label: number }> = {
  1: { dot: 6, label: 8 },
  2: { dot: 8, label: 9 },
  3: { dot: 10, label: 11 },
}
