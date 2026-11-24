import type { Map as MLMap } from 'maplibre-gl'

/** key (KeyboardEvent.code, so WASD is by position on any layout) to screen direction: x right, y down */
const DIRS: Record<string, [number, number]> = {
  ArrowLeft: [-1, 0],
  ArrowRight: [1, 0],
  ArrowUp: [0, -1],
  ArrowDown: [0, 1],
  KeyA: [-1, 0],
  KeyD: [1, 0],
  KeyW: [0, -1],
  KeyS: [0, 1],
}
const SPEED = 700 // px per second; Shift doubles it

/** unit direction of the held keys; opposite keys cancel, a diagonal is not faster */
export function panDir(held: Iterable<string>): [number, number] {
  let x = 0
  let y = 0
  for (const k of held) {
    const d = DIRS[k]
    if (d) {
      x += d[0]
      y += d[1]
    }
  }
  x = Math.sign(x)
  y = Math.sign(y)
  const n = Math.hypot(x, y) || 1
  return [x / n, y / n]
}

/** typing in a field must not move the map */
export function typingIn(t: EventTarget | null): boolean {
  const el = t as HTMLElement | null
  if (!el || !el.tagName) return false
  return el.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName)
}

/**
 * Arrow keys and W A S D pan the map while held, wherever the focus is (except in a text field).
 * Each frame is a jump marked `keyPan`, so a listener can tell it from a finished move; one plain `moveend` fires on release.
 */
export function attachKeyPan(map: MLMap): () => void {
  const held = new Set<string>()
  let fast = false
  let raf = 0
  let last = 0
  const step = (t: number) => {
    const [x, y] = panDir(held)
    if (!x && !y) {
      raf = 0
      map.fire('moveend')
      return
    }
    const d = (Math.min(t - last, 50) / 1000) * SPEED * (fast ? 2 : 1)
    last = t
    map.panBy([x * d, y * d], { animate: false }, { keyPan: true })
    raf = requestAnimationFrame(step)
  }
  const down = (e: KeyboardEvent) => {
    if (!(e.code in DIRS)) return
    if (e.metaKey || e.ctrlKey || e.altKey) return held.clear()
    if (typingIn(e.target)) return
    e.preventDefault()
    e.stopPropagation() // MapLibre's own arrow-key pan would add to this one
    fast = e.shiftKey
    held.add(e.code)
    if (!raf) {
      last = performance.now()
      raf = requestAnimationFrame(step)
    }
  }
  const up = (e: KeyboardEvent) => {
    fast = e.shiftKey
    held.delete(e.code)
  }
  const clear = () => held.clear()
  window.addEventListener('keydown', down, true)
  window.addEventListener('keyup', up, true)
  window.addEventListener('blur', clear)
  return () => {
    window.removeEventListener('keydown', down, true)
    window.removeEventListener('keyup', up, true)
    window.removeEventListener('blur', clear)
    cancelAnimationFrame(raf)
  }
}
