import { expect, test, vi } from 'vitest'
import {
  bandWays,
  canExpand,
  cityRows,
  clock,
  cnFocus,
  decodeBands,
  decodeLine,
  destLookup,
  hm,
  partEnglish,
  snapshotText,
  stretchFeatures,
  wayCells,
} from './cnServices'
import { fnv1a, loadCnRecord, type CnCity, type CnRecord } from './data'

const city = (zh: string, n: number, parts: [string, number][]): CnCity => [
  zh,
  null,
  n,
  60,
  parts.map(([p, k]) => [p, p === '上海虹桥' ? 'Shanghai Hongqiao' : null, k, 60]),
]
const rec: CnRecord = {
  name: '成都东',
  name_en: 'Chengdu East',
  stops: 3,
  dep: 2,
  arr: 1,
  first: 355,
  last: 1350,
  hd: Array(24).fill(0),
  ha: Array(24).fill(0),
  dest: [
    city('重庆', 10, [
      ['重庆西', 6],
      ['重庆北', 4],
    ]),
    city('上海', 3, [['上海虹桥', 3]]),
  ],
  dst: { 'node/2': [12, 606, 393, 1071, 11, 631, 361, 1124], 'node/3': [1, 90, 400, 400, 0, 0, 0, 0] },
  st: [[3, 2, 5], [], [10], [], [0, 1]],
  pts: [[104.1, 30.6]],
}
const ids = { 'relation/9': 'node/2', 'node/7': 'node/1' } // a complex id and a metro station of the record's own complex

test('a band stores its stretch numbers as steps from the one before, the first from 0', () => {
  expect(decodeBands(rec.st)).toEqual([[3, 5, 10], [], [10], [], [0, 1]])
})

test('the focus keeps the record, its destinations and their other ids lit', () => {
  const f = cnFocus('node/1', rec, ids)
  expect(new Set(f.lit)).toEqual(new Set(['node/1', 'node/2', 'node/3', 'relation/9', 'node/7']))
  expect(f.cities).toEqual(['重庆', '上海'])
  expect(f.bands[0]).toEqual([3, 5, 10])
})

test('the city table shows the first rows and the count of the rest; only a city with several stations expands', () => {
  const many = Array.from({ length: 11 }, (_, i) => city(`城${i}`, 11 - i, [[`站${i}`, 1]]))
  expect(cityRows(many, false)).toEqual({ rows: many.slice(0, 8), more: 3 })
  expect(cityRows(many, true).rows).toHaveLength(11)
  expect(canExpand(rec.dest[0])).toBe(true)
  expect(canExpand(rec.dest[1])).toBe(false)
  expect(rec.dest[0][4].reduce((a, p) => a + p[2], 0)).toBe(rec.dest[0][2]) // the stations of Chongqing add up to the city
})

test('a hovered station finds its trains by its station id, its complex id, or ids.json', () => {
  expect(destLookup(rec, ids, 'node/3', '')?.key).toBe('node/3')
  expect(destLookup(rec, ids, 'node/99', 'node/2')?.d[0]).toBe(12)
  expect(destLookup(rec, ids, 'relation/9', '')?.key).toBe('node/2')
  expect(destLookup(rec, ids, 'node/99', 'node/98')).toBeNull()
  expect(destLookup(rec, ids, 'node/7', '')).toBeNull() // maps to the record's own station: no trains to itself
})

test('English names of destination stations come from the record, found by the Chinese name', () => {
  expect(partEnglish(rec, '上海虹桥站')).toBe('Shanghai Hongqiao')
  expect(partEnglish(rec, '无此站')).toBeNull()
})

test('times', () => {
  expect(clock(355)).toBe('05:55')
  expect(clock(1500)).toBe('01:00')
  expect(hm(606)).toBe('10h06')
})

test('way cells cover the view and a margin; band ways come from the cells loaded', () => {
  expect(wayCells([104.5, 30.5, 105, 30.8])).toEqual(['52_15'])
  expect(wayCells([104.5, 30.5, 105, 30.8], 0.8)).toEqual(['51_14', '51_15', '52_14', '52_15'])
  const cells: Record<string, number[]>[] = [
    { '3': [30, 10], '5': [20] },
    { '3': [10], '10': [99] },
  ]
  expect(bandWays(cells, [[3, 5], [], [10]])).toEqual([['way/10', 'way/20', 'way/30'], [], ['way/99']])
})

test('a stored line is its first point in 1/q degrees, then steps', () => {
  expect(decodeLine([104100, 30600, 10, -20, -5, 5], 1000)).toEqual([
    [104.1, 30.6],
    [104.11, 30.58],
    [104.105, 30.585],
  ])
})

test('stretch geometry becomes one line feature per stretch with its type and band', () => {
  const list = {
    q: 1000,
    s: [
      { t: 'high_speed', c: [[1000, 2000, 1000, 1000]] },
      {
        t: 'regional',
        c: [
          [5000, 6000, 1000, 1000],
          [9000, 10000, 1000, 1000],
        ],
      },
    ],
  }
  const f = stretchFeatures(list, [[0], [1]])
  expect(f.map((x) => x.properties)).toEqual([
    { type: 'high_speed', band: 0 },
    { type: 'regional', band: 1 },
  ])
  expect((f[1].geometry as GeoJSON.MultiLineString).coordinates).toEqual([
    [
      [5, 6],
      [6, 7],
    ],
    [
      [9, 10],
      [10, 11],
    ],
  ])
})

test('a China record is read from cn12306/<rev>/ shard fnv1a(id) % count; ids.json only for an id with no record of its own', async () => {
  const asked: string[] = []
  const shards: Record<string, CnRecord>[] = [{}, {}, {}, {}]
  const other = { ...rec, name: '成都南' } // a second keyed station of the same complex
  for (const [k, r] of [
    ['way/123', rec],
    ['way/124', other],
  ] as const)
    shards[fnv1a(k) % 4][k] = r
  vi.stubGlobal('fetch', async (url: string) => {
    const path = url.replace(/^.*\/world\//, '')
    asked.push(path)
    const n = path.match(/abcd1234\/(\d+)\.json$/)
    const body =
      path === 'cn/shards.json'
        ? {
            cn12306: {
              rev: 'abcd1234',
              count: 4,
              snapshot: [
                { date: '2026-10-21', classes: ['G', 'D', 'K', 'Z', 'T', 'plain'] },
                { date: '2026-10-28', classes: ['C', 'S'] },
              ],
              cells: ['52_15'],
            },
          }
        : path === 'cn/cn12306/abcd1234/ids.json'
          ? { 'relation/9': 'way/123' }
          : (shards[Number(n?.[1])] ?? {})
    return { ok: true, status: 200, json: async () => body }
  })
  expect((await loadCnRecord('way/124'))?.rec.name).toBe('成都南')
  const r123 = await loadCnRecord('way/123')
  expect(r123?.snapshot).toEqual([
    { date: '2026-10-21', classes: ['G', 'D', 'K', 'Z', 'T', 'plain'] },
    { date: '2026-10-28', classes: ['C', 'S'] },
  ])
  expect(snapshotText(r123!.snapshot)).toBe('Timetables: 12306, one day each: 2026-10-21 (G, D, K, Z, T, other), 2026-10-28 (C, S)')
  expect(asked.some((p) => p.endsWith('ids.json'))).toBe(false)
  expect((await loadCnRecord('relation/9'))?.key).toBe('way/123')
  expect(await loadCnRecord('way/999')).toBeNull()
})

test('loadCnRecord normalises a legacy plain string snapshot and formats the footnote', async () => {
  vi.resetModules()
  const { loadCnRecord: load } = await import('./data')
  vi.stubGlobal('fetch', async (url: string) => {
    const path = url.replace(/^.*\/world\//, '')
    const body = path === 'cn/shards.json' ? { cn12306: { rev: 'abcd1234', count: 4, snapshot: '2026-10-21', cells: ['52_15'] } } : { 'way/123': rec }
    return { ok: true, status: 200, json: async () => body }
  })
  const r = await load('way/123')
  expect(r?.snapshot).toEqual([{ date: '2026-10-21', classes: [] }])
  expect(snapshotText(r!.snapshot)).toBe('Timetables: 12306, one day each: 2026-10-21')
})

test('snapshotText maps plain to other and omits parentheses when classes is empty', () => {
  expect(
    snapshotText([
      { date: '2026-10-21', classes: ['plain'] },
      { date: '2026-10-28', classes: [] },
    ])
  ).toBe('Timetables: 12306, one day each: 2026-10-21 (other), 2026-10-28')
})
