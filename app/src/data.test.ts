import { expect, test, vi } from 'vitest'
import { fnv1a, loadStation } from './data'

// Reference values: the Rust fnv1a in pipeline/rail-extract/src/tiles.rs (32-bit FNV-1a over the UTF-8 bytes), computed independently
test('fnv1a matches the Rust writer', () => {
  expect(fnv1a('')).toBe(2166136261)
  expect(fnv1a('a')).toBe(3826002220)
  expect(fnv1a('way/123')).toBe(3327974463)
  expect(fnv1a('Zürich HB')).toBe(3802415342)
  expect(fnv1a('node/5551234567')).toBe(3619258025)
  expect(fnv1a('東京')).toBe(1759422319)
})

test('a station is read from shard fnv1a(id) % count of shards.json', async () => {
  const asked: string[] = []
  vi.stubGlobal('fetch', async (url: string) => {
    asked.push(url.replace(/^.*\/world\//, ''))
    const path = asked[asked.length - 1]
    const body = path === 'zz/shards.json' ? { stations: 4, lines: 1, lines_big: [], tags: 1, 'name-hl': 1 } : { 'way/123': { id: 'way/123' } }
    return { ok: true, status: 200, json: async () => body }
  })
  expect((await loadStation('zz', 'way/123')).id).toBe('way/123')
  expect(asked).toEqual(['zz/shards.json', 'zz/stations/3.json'])   // 3327974463 % 4
  await expect(loadStation('zz', 'way/999')).rejects.toThrow('no station')
})
