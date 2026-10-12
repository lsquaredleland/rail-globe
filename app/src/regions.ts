export interface Region {
  slug: string
  name: string
  sub: string
  local?: string
  center: [number, number] // lon, lat
  zoom: number
  zoomPhone: number
}

export const REGIONS: Region[] = [
  { slug: 'sf-bay-area', name: 'SF Bay Area', sub: '9 counties', center: [-122.22, 37.7], zoom: 9.3, zoomPhone: 8.4 },
  { slug: 'greater-bay-area', name: 'Greater Bay Area', sub: 'Pearl River Delta + HK + Macao', local: '粤港澳大湾区', center: [113.55, 22.75], zoom: 8.2, zoomPhone: 7.3 },
  { slug: 'paris', name: 'Paris', sub: 'Île-de-France', center: [2.35, 48.86], zoom: 10.6, zoomPhone: 9.6 },
  { slug: 'chengdu', name: 'Chengdu', sub: 'Municipality', local: '成都', center: [104.07, 30.66], zoom: 10.2, zoomPhone: 9.2 },
]

export const regionBySlug = (s: string) => REGIONS.find((r) => r.slug === s) ?? REGIONS[1]
