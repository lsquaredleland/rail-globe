# Deploying: R2 (data) + Vercel (app)

**Pilot on r2.dev:** enable public access on the bucket (Settings > Public Development URL), put its exact `https://pub-xxxx.r2.dev` origin in `app/vercel.json` `connect-src` (no wildcard), set `VITE_DATA_BASE=https://pub-xxxx.r2.dev/` and `ALLOW_R2_DEV=1` in Vercel. Switch to a custom domain and disable the r2.dev URL before public launch.

Replace the placeholders first: `data.example.com` (R2 custom domain) in `app/vercel.json` and `rail-globe.vercel.app` (done) in `r2/cors.json`. The Vercel build fails if they do not match `VITE_DATA_BASE`.

## R2 bucket (Cloudflare dashboard)
1. Create a bucket. Leave **r2.dev public access disabled**: it is uncached, rate-limited and cannot carry a WAF.
2. Settings > Custom Domains: connect `data.example.com` (a zone on Cloudflare). This gives TLS, edge cache and WAF.
3. Settings > CORS: paste `r2/cors.json` (one origin: the app; GET/HEAD only; `Range`/`If-Match` allowed; `Content-Range`/`ETag` exposed).
4. Cloudflare zone, for the data domain: SSL mode Full (strict), min TLS 1.2, "Always use HTTPS", HSTS on. Add a Cache Rule (cache everything, respect origin headers) and a rate-limiting rule (e.g. 300 requests/min/IP) so range reads cannot run up the bill.
5. R2 > Manage API tokens: create a token with **Object Read & Write on this bucket only**. No Admin token. Rotate it on a schedule; it exists only in your shell/CI secrets.

## Publish data
```
export R2_ACCOUNT_ID=... R2_BUCKET=... R2_ACCESS_KEY_ID=... R2_SECRET_ACCESS_KEY=...
r2/sync.sh --dry-run   # check first
r2/sync.sh
```
Tiles and side files go to the bucket root, glyphs to `glyphs/` (they are gitignored, so Vercel cannot ship them).
The app never gets write credentials; the browser only does GET/HEAD.

## Vercel
- Import the GitHub repo, **Root Directory `app`**.
- Environment variable (Production and Preview): `VITE_DATA_BASE=https://data.example.com/`. It is public by design; never put secrets in `VITE_*`.
- `app/vercel.json` sets the install (`npm ci --ignore-scripts`), the pre-build check, a strict CSP (own origin + data domain + OpenFreeMap + Google Fonts), HSTS, nosniff, frame denial, COOP/CORP and Permissions-Policy.
- Turn on: Deployment Protection for Preview deployments, 2FA on the Vercel and Cloudflare accounts, GitHub branch protection on `main`.
- Preview URLs differ from the CORS origin, so previews cannot load data unless you add them to `r2/cors.json` (or keep them protected).

## Verify after deploy
`curl -sI https://rail-globe.vercel.app | grep -iE 'content-security|strict-transport'`; `curl -sI -H 'Range: bytes=0-15' https://data.example.com/pilot.pmtiles` returns 206 and `cf-cache-status`; browser console shows no CSP violations on a cold load.

## Known gaps
- Google Fonts is a third-party stylesheet (no SRI possible); self-hosting IBM Plex would let the CSP drop two origins.
- `style-src 'unsafe-inline'` is needed by MapLibre and React inline styles.
- OpenFreeMap is a third-party basemap origin.
