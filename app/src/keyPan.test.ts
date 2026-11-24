import { expect, test } from 'vitest'
import { panDir, typingIn } from './keyPan'

test('arrows and WASD give the same directions', () => {
  expect(panDir(['ArrowLeft'])).toEqual(panDir(['KeyA']))
  expect(panDir(['ArrowUp'])).toEqual([0, -1])
  expect(panDir(['KeyS'])).toEqual([0, 1])
  expect(panDir(['KeyD', 'ArrowRight'])).toEqual([1, 0])
})

test('opposite keys cancel, a diagonal has length 1, other keys do nothing', () => {
  expect(panDir(['KeyA', 'KeyD'])).toEqual([0, 0])
  const [x, y] = panDir(['KeyW', 'ArrowRight'])
  expect(Math.hypot(x, y)).toBeCloseTo(1)
  expect(panDir(['KeyQ', 'Space'])).toEqual([0, 0])
})

test('a text field keeps its keys', () => {
  expect(typingIn({ tagName: 'INPUT' } as unknown as EventTarget)).toBe(true)
  expect(typingIn({ tagName: 'BUTTON' } as unknown as EventTarget)).toBe(false)
  expect(typingIn(null)).toBe(false)
})
