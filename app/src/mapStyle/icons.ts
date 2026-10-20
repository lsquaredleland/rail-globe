import type { Map as MLMap } from 'maplibre-gl'
import { ignoreError } from '../dev'
import { logoUrl } from '../config'

/** Line number badges: a rounded rectangle in the line's colour with a white outline, stretchable so icon-text-fit can size it to the number.
 *  Made on demand: a layer asks for `badge|<colour>|<d or l>` and the styleimagemissing event draws it. */
export function addBadgeImages(map: MLMap) {
  map.on('styleimagemissing', (e) => {
    const [kind, colour] = e.id.split('|')
    if (kind === 'logo' && colour && !map.hasImage(e.id)) {
      // operator logo: fetched on first use; a missing file gets a transparent pixel so it is not asked for again
      map.addImage(e.id, { width: 1, height: 1, data: new Uint8Array(4) })
      // drawn on a white rounded chip with a thin outline, so a bare logo does not run into the dot and the track under it
      map
        .loadImage(logoUrl(colour))
        .then((r) => {
          const px = 2,
            pad = 4
          const img = r.data as CanvasImageSource & { width: number; height: number }
          const k = Math.min(56 / img.width, 24 / img.height) // wordmarks may be wide, marks are squarer
          const w = Math.round(img.width * k),
            h = Math.round(img.height * k)
          const c = document.createElement('canvas')
          c.width = (w + 2 * pad) * px
          c.height = (h + 2 * pad) * px
          const g = c.getContext('2d')!
          g.scale(px, px)
          g.beginPath()
          g.roundRect(0.75, 0.75, w + 2 * pad - 1.5, h + 2 * pad - 1.5, 5)
          g.fillStyle = '#FFFFFF'
          g.fill()
          g.lineWidth = 1.5
          g.strokeStyle = 'rgba(30,30,28,0.35)'
          g.stroke()
          g.drawImage(img, pad, pad, w, h)
          if (map.hasImage(e.id)) map.removeImage(e.id)
          map.addImage(e.id, g.getImageData(0, 0, c.width, c.height), { pixelRatio: px })
        })
        .catch(ignoreError(`logo ${colour}`))
      return
    }
    if (kind !== 'badge' || !colour || map.hasImage(e.id)) return
    const px = 2,
      size = 20 * px
    const c = document.createElement('canvas')
    c.width = c.height = size
    const g = c.getContext('2d')!
    g.scale(px, px)
    g.beginPath()
    g.roundRect(1, 1, 18, 18, 5)
    g.fillStyle = colour
    g.fill()
    g.lineWidth = 1.5
    g.strokeStyle = '#FFFFFF'
    g.stroke()
    map.addImage(e.id, g.getImageData(0, 0, size, size), { pixelRatio: px, stretchX: [[12, 28]], stretchY: [[12, 28]], content: [8, 6, 32, 34] })
  })
}

/** Material "flight" glyph, 24x24 */
export const PLANE_PATH = 'M21 16v-2l-8-5V3.5c0-.83-.67-1.5-1.5-1.5S10 2.67 10 3.5V9l-8 5v2l8-2.5V19l-2 1.5V22l3.5-1 3.5 1v-1.5L13 19v-5.5l8 2.5z'

/** Airport station marker: a plane on a white disc; red ring when high-speed rail serves the station. */
export function addAirportIcon(map: MLMap) {
  const make = (name: string, ring: string) => {
    if (map.hasImage(name)) return
    const px = 2
    const size = 26 * px
    const c = document.createElement('canvas')
    c.width = c.height = size
    const g = c.getContext('2d')!
    g.scale(px, px)
    g.beginPath()
    g.arc(13, 13, 11.5, 0, Math.PI * 2)
    g.fillStyle = '#FFFFFF'
    g.fill()
    g.lineWidth = 2.2
    g.strokeStyle = ring
    g.stroke()
    g.translate(13 - 8.4, 13 - 8.4)
    g.scale(0.7, 0.7)
    g.fillStyle = '#1E1E1C'
    g.fill(new Path2D(PLANE_PATH))
    map.addImage(name, g.getImageData(0, 0, size, size), { pixelRatio: px })
  }
  make('airport', '#1E1E1C')
}
