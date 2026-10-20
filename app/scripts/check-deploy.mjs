// Fails the Vercel build unless the data origin is HTTPS, is allowed by the CSP in vercel.json, and is not r2.dev.
// r2.dev (rate-limited, no cache, no WAF) is allowed only with ALLOW_R2_DEV=1, set in the Vercel project: dev use only.
import { readFileSync } from 'node:fs'

const base = process.env.VITE_DATA_BASE
const fail = (m) => {
  console.error(`check-deploy: ${m}`)
  process.exit(1)
}
if (!base) fail('VITE_DATA_BASE is not set; set it in the Vercel project to the R2 custom domain, with a trailing slash')
let u
try {
  u = new URL(base)
} catch {
  fail(`VITE_DATA_BASE is not a URL: ${base}`)
}
if (u.protocol !== 'https:') fail('VITE_DATA_BASE must be https')
if (u.hostname.endsWith('.r2.dev')) {
  if (process.env.ALLOW_R2_DEV !== '1') fail('r2.dev is the rate-limited, uncached dev URL; use a custom domain, or set ALLOW_R2_DEV=1 for now')
  console.warn('check-deploy: WARNING r2.dev in use (dev only): move to a custom domain before public launch')
}
const csp = JSON.parse(readFileSync(new URL('../vercel.json', import.meta.url), 'utf8')).headers[0].headers.find(
  (h) => h.key === 'Content-Security-Policy'
).value
const connect =
  csp
    .split(';')
    .map((d) => d.trim())
    .find((d) => d.startsWith('connect-src ')) ?? ''
if (!connect.split(' ').includes(u.origin)) fail(`${u.origin} is not in connect-src of vercel.json (add the data origin there)`)
console.log(`check-deploy: ok (${u.origin})`)
