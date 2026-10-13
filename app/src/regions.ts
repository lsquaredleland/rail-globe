export interface Region {
  slug: string
  name: string
  sub: string
  local?: string
  center: [number, number] // lon, lat
  zoom: number
  zoomPhone: number
  /** the region whose tiles draw this one, when it lies inside a bigger region: its own outline, Lines list and side files,
   *  but drawn from the bigger region's tiles, so overlapping regions are not loaded and drawn twice */
  tiles?: string
  /** no card of its own: an area of the bigger region (a city, a province) already is this region */
  noCard?: boolean
}

export const REGIONS: Region[] = [
  { slug: 'sf-bay-area', name: 'SF Bay Area', sub: '9 counties', center: [-122.22, 37.7], zoom: 9.3, zoomPhone: 8.4 },
  { slug: 'greater-bay-area', name: 'Greater Bay Area', sub: 'Pearl River Delta + HK + Macao', local: '粤港澳大湾区', center: [113.55, 22.75], zoom: 8.2, zoomPhone: 7.3, tiles: 'china' },
  { slug: 'paris', name: 'Paris', sub: 'Île-de-France', center: [2.35, 48.86], zoom: 10.6, zoomPhone: 9.6 },
  { slug: 'chengdu', name: 'Chengdu', sub: 'Municipality', local: '成都', center: [104.07, 30.66], zoom: 10.2, zoomPhone: 9.2, tiles: 'china', noCard: true },
  { slug: 'cn-beijing', name: 'Beijing', sub: 'Municipality', local: '北京', center: [116.4, 39.93], zoom: 9.6, zoomPhone: 8.6, tiles: 'china', noCard: true },
  { slug: 'china', name: 'China', sub: 'Mainland + HK + Macao', local: '中国', center: [108.0, 34.5], zoom: 3.8, zoomPhone: 2.8 },
]

export const regionBySlug = (s: string) => REGIONS.find((r) => r.slug === s) ?? REGIONS[1]

/** the tile source that draws a region */
export const tilesOf = (slug: string) => REGIONS.find((r) => r.slug === slug)?.tiles ?? slug
/** the tile sources on the map: one per region that is not drawn from another's tiles */
export const SOURCES: string[] = REGIONS.filter((r) => !r.tiles).map((r) => r.slug)
