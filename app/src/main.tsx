import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import './styles.css'
import { GLYPHS_URL } from './config'

// one-time check: a missing glyph set (never generated) would silently fail every label
fetch(GLYPHS_URL.replace('{fontstack}', 'Noto Sans Regular').replace('{range}', '0-255'))
  .then((r) => { if (!r.ok || (r.headers.get('content-type') ?? '').includes('text/html')) throw new Error(String(r.status)) })
  .catch(() => console.error('glyphs missing: run pipeline/glyphs (see pipeline/README.md)'))

createRoot(document.getElementById('root')!).render(<StrictMode><App /></StrictMode>)
