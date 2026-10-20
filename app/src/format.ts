import { TYPE_LABEL, TYPE_LABEL_SHORT, TYPE_ORDER } from './palette'

export const UNKNOWN = 'unknown'

export function railTypeLabel(t: string | undefined, usage?: string): string {
  const base = t ? (TYPE_LABEL[t] ?? t) : UNKNOWN
  const u: Record<string, string> = {
    main: 'main line',
    branch: 'branch line',
    industrial: 'industrial',
    military: 'military',
    tourism: 'tourist line',
    freight: 'freight line',
  }
  return usage && u[usage] ? `${base} · ${u[usage]}` : base
}

export function electrification(el?: string, v?: string, fq?: string): string {
  if (!el) return UNKNOWN
  if (el === 'no' || el === 'none') return 'Not electrified'
  const volts = v ? Number(String(v).split(';')[0]) : NaN
  const vs = Number.isFinite(volts) ? (volts >= 1000 ? `${+(volts / 1000).toFixed(1)} kV` : `${volts} V`) : ''
  const f = fq ? Number(String(fq).split(';')[0]) : NaN
  const cur = Number.isFinite(f) ? (f === 0 ? 'DC' : 'AC') : ''
  const how = el === 'contact_line' ? 'overhead' : el === 'rail' ? 'third rail' : el === 'yes' ? '' : el.replace(/_/g, ' ')
  const out = [vs, cur, how].filter(Boolean).join(' ')
  return out || 'Electrified'
}

export function maxSpeed(ms?: string): string {
  if (!ms) return UNKNOWN
  if (/^\d+$/.test(ms)) return `${ms} km/h`
  return /^\d+\s*mph$/.test(ms) ? ms.replace(/\s*mph/, ' mph') : ms
}

const GAUGES: Record<string, string> = {
  '1435': 'standard',
  '1000': 'metre',
  '1520': 'Russian',
  '1600': 'Irish',
  '1668': 'Iberian',
  '1067': 'Cape',
  '914': '3 ft',
  '762': '2 ft 6 in',
}
export function gauge(g?: string): string {
  if (!g) return UNKNOWN
  const first = String(g).split(';')[0]
  return GAUGES[first] ? `${first} mm (${GAUGES[first]})` : `${first} mm`
}

export function tierLabel(tier: number, hsr: boolean): string {
  if (tier === 1) return hsr ? 'Major HSR station' : 'Major station'
  if (tier === 2) return 'Major station'
  return 'Minor station'
}

/** "high_speed:8,metro:4" -> "8 high-speed · 4 metro" */
export function lineBreakdown(lct: string): string {
  if (!lct) return ''
  const parts = lct.split(',').map((p) => p.split(':'))
  parts.sort((a, b) => TYPE_ORDER.indexOf(a[0] as never) - TYPE_ORDER.indexOf(b[0] as never))
  return parts.map(([t, n]) => `${n} ${TYPE_LABEL_SHORT[t] ?? t}`).join(' · ')
}

export function km(v: number, units: 'km' | 'mi'): string {
  const x = units === 'km' ? v : v * 0.621371
  return `${x >= 100 ? Math.round(x) : x.toFixed(1)} ${units}`
}

export const AREA_LABEL: Record<string, string> = {
  aerodrome: 'Airport',
  terminal: 'Airport terminal',
  land: 'Station grounds',
  yard: 'Railway yard',
  building: 'Station building',
  platform: 'Platform',
}
export const SERVICE_LABEL: Record<string, string> = { yard: 'yard', siding: 'siding', spur: 'spur', crossover: 'crossover' }

export function areaSize(ha: number, units: 'km' | 'mi'): string {
  if (units === 'mi') {
    const ac = ha * 2.47105
    return `${ac >= 100 ? Math.round(ac) : ac.toFixed(1)} acres`
  }
  return `${ha >= 100 ? Math.round(ha) : ha.toFixed(ha < 1 ? 2 : 1)} ha`
}
