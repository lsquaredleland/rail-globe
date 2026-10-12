// Rail type palette from the design (Palette board). Heavy rail is warm and dark, urban rail cool and lighter.
export type RailType =
  | 'high_speed' | 'intercity' | 'regional' | 'metro' | 'light_rail' | 'monorail' | 'funicular' | 'other' | 'freight' | 'yard'

export const TYPE_ORDER: RailType[] = ['high_speed', 'intercity', 'regional', 'metro', 'light_rail', 'monorail', 'funicular', 'other']

export const TYPE_LABEL: Record<string, string> = {
  high_speed: 'High-speed', intercity: 'Intercity', regional: 'Regional / commuter',
  metro: 'Metro / subway', light_rail: 'Light rail / tram', monorail: 'Monorail', funicular: 'Funicular and cable car', other: 'Other', freight: 'Freight', yard: 'Yard and service',
}
export const TYPE_LABEL_SHORT: Record<string, string> = {
  high_speed: 'high-speed', intercity: 'intercity', regional: 'regional / commuter',
  metro: 'metro', light_rail: 'light rail / tram', monorail: 'monorail', funicular: 'funicular / cable car', other: 'other',
}

export const TYPE_COLOR: Record<RailType, string> = {
  high_speed: '#B3123A', intercity: '#E0661A', regional: '#C99A06', metro: '#1C5DB8',
  light_rail: '#13909C', monorail: '#6B3FA0', funicular: '#6E8B3D', other: '#9A6B47', freight: '#9E9B94', yard: '#BDB9B1',
}

// line width in px at z10 / z12 / z15
export const TYPE_WIDTH: Record<RailType, [number, number, number]> = {
  high_speed: [3, 4.5, 6], intercity: [2.5, 3.5, 5], regional: [2, 3, 4.5], metro: [1.6, 2.4, 3.5],
  light_rail: [1.2, 1.8, 2.8], monorail: [1.2, 1.8, 2.8], funicular: [1, 1.6, 2.5], other: [1, 1.6, 2.5], freight: [0.8, 1.3, 2], yard: [0.5, 0.9, 1.2],
}

export const FILL = {
  land: '#F3F2EF', water: '#D6DFE6', park: '#E6EAE1', road: '#E1DED8', boundary: '#CFCBC4', place: '#8E8A82',
  ground: '#E7E1D5', groundEdge: '#DDD6C8', yard: '#ECE7DE', building: '#D5CDBF', buildingEdge: '#BCB3A3', platform: '#C9C1B2',
}

export const INK = '#1E1E1C'

/** Readable text colour on a swatch background. */
export function textOn(bg: string | null | undefined): string {
  if (!bg) return INK
  const m = /^#([0-9a-f]{6})$/i.exec(bg.length === 4 ? '#' + [...bg.slice(1)].map((c) => c + c).join('') : bg)
  if (!m) return '#FFFFFF'
  const n = parseInt(m[1], 16)
  const lin = (c: number) => { c /= 255; return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4) }
  const L = 0.2126 * lin((n >> 16) & 255) + 0.7152 * lin((n >> 8) & 255) + 0.0722 * lin(n & 255)
  return L > 0.4 ? INK : '#FFFFFF'
}
