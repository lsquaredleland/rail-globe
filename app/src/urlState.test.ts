import { expect, test } from 'vitest'
import { readHash, writeHash } from './urlState'
import { regionBySlug } from './regions'

test('an empty hash reads as the defaults', () => {
  location.hash = ''
  const s = readHash()
  expect(s).toMatchObject({
    mode: 'type',
    hsrOnly: false,
    construction: true,
    proposed: false,
    freight: false,
    units: 'km',
    lang: 'local',
    hidden: [],
    hiddenSpeed: [],
    inspect: false,
    outline: true,
    theme: 'light',
    view: null,
  })
  expect(s.region.slug).toBe('world')
})

test('writeHash then readHash round trips every field', () => {
  const state = {
    region: regionBySlug('de-bavaria'),
    mode: 'speed' as const,
    hsrOnly: true,
    construction: false,
    proposed: true,
    freight: true,
    units: 'mi' as const,
    lang: 'en' as const,
    hidden: ['metro', 'freight'],
    hiddenSpeed: [400, -1, 0],
    inspect: true,
    outline: false,
    theme: 'dark' as const,
    view: { center: [11.5755, 48.1374] as [number, number], zoom: 7.25 },
  }
  writeHash(state)
  expect(location.hash).toMatch(/^#/)
  expect(readHash()).toEqual(state)
})

test('defaults are left out of the hash, a bad view is ignored', () => {
  location.hash = ''
  writeHash({ ...readHash(), region: regionBySlug('world'), view: null })
  expect(location.hash).toBe('#r=world&c=type')
  location.hash = '#v=3/abc/4'
  expect(readHash().view).toBeNull()
})
