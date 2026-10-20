import { afterEach, expect, test, vi } from 'vitest'
import { loadParts, onPartsChange, partAt } from './regions'

const square = (x0: number, y0: number, x1: number, y1: number) => ({
  type: 'Feature',
  geometry: {
    type: 'Polygon',
    coordinates: [
      [
        [x0, y0],
        [x1, y0],
        [x1, y1],
        [x0, y1],
        [x0, y0],
      ],
    ],
  },
})
const files: Record<string, unknown> = {
  'world.parts.json': [
    { slug: 'a', bounds: [0, 0, 10, 10] },
    { slug: 'b', bounds: [20, 0, 30, 10] },
    { slug: 'c', bounds: [8, 0, 12, 10] },
  ],
  'a.boundary.geojson': square(0, 0, 10, 10),
  'c.boundary.geojson': square(10, 0, 12, 10),
}
vi.stubGlobal('fetch', async (url: string) => {
  const body = files[url.split('/').pop()!]
  return { ok: body !== undefined, status: body ? 200 : 404, json: async () => body }
})
afterEach(() => vi.restoreAllMocks())

test('partAt answers empty before world.parts.json is in', () => {
  expect(partAt([5, 5])).toBe('')
})

test('partAt: inside one part, nearest bounds outside, outline wins in an overlap', async () => {
  await loadParts()
  expect(partAt([5, 5])).toBe('a')
  expect(partAt([25, 5])).toBe('b')
  expect(partAt([14, 5])).toBe('c') // outside every box: the nearest one (c reaches 12, a only 10)
  expect(partAt([2, 50])).toBe('a') // far north: nearest bounds
  // (9, 5) lies in the boxes of a and c: until the outlines are in, the smaller box (c) answers; then the outline that holds the point
  const settled = new Promise<void>((res) => {
    const off = onPartsChange(() => {
      if (partAt([9, 5]) === 'a') {
        off()
        res()
      }
    })
  })
  expect(partAt([9, 5])).toBe('c')
  await settled
  expect(partAt([9, 5])).toBe('a')
  expect(partAt([11, 5])).toBe('c')
})
