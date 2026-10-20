# Deploying: R2 (data) + Vercel (app)

**Live on r2.dev (since 2026-10-16):** public development URL https://pub-dd749a156c5c4e869e4d54eb1c24d0c5.r2.dev is in app/vercel.json connect-src; VITE_DATA_BASE and ALLOW_R2_DEV=1 are set in Vercel. Switch to a custom domain and disable the r2.dev URL before public launch.

The placeholders `data.example.com` are for the custom domain at launch; `r2/cors.json` already names `rail-globe.vercel.app`. The Vercel build fails if the domain in `app/vercel.json` does not match `VITE_DATA_BASE`.

## R2 bucket (Cloudflare dashboard)
1. Create a bucket. Today r2.dev public access is enabled (dev URL); disable it once the custom domain below is live (it is uncached, rate-limited and cannot carry a WAF).
2. Settings > Custom Domains: connect `data.example.com` (a zone on Cloudflare). This gives TLS, edge cache and WAF.
3. Settings > CORS: paste `r2/cors.json` (one origin: the app; GET/HEAD only; `Range`/`If-Match` allowed; `Content-Range`/`ETag` exposed).
4. Cloudflare zone, for the data domain: SSL mode Full (strict), min TLS 1.2, "Always use HTTPS", HSTS on. Add a Cache Rule (cache everything, respect origin headers) and a rate-limiting rule (e.g. 300 requests/min/IP) so range reads cannot run up the bill.
5. R2 > Manage API tokens: create a token with **Object Read & Write on this bucket only**. No Admin token. Rotate it on a schedule; it exists only in your shell/CI secrets.

## Custom domain checklist (replaces the r2.dev URL)
Measured on the r2.dev URL on 2026-10-17 (`curl -sI`, `Origin: https://rail-globe.vercel.app`, `Accept-Encoding: br, gzip`): HTTP/1.1 for every file; no `Content-Encoding` on shard `.json` or `world.pmtiles`; CORS correct everywhere (`Access-Control-Allow-Origin` is the app origin, `Vary: Origin`, preflight 204 with `GET, HEAD` and `Range`); `Cache-Control`: tiles `world/world.pmtiles` and shard files `max-age=3600`, **`world/fr/shards.json` still `max-age=3600`**. Cause of the last one: the live objects are older than the `sync.sh` fix. `shards.json` was last modified 2026-10-16 12:22 PDT and the tiles 12:34 PDT; the no-cache upload came in with commit ec6cded at 12:39 PDT, and `sync.sh` has not been run since. The script itself is right: a local test showed `--ignore-times` re-copies a file even with `--checksum` set (without it, the identical file is skipped), so one `r2/sync.sh` run fixes the header.

Do these in order. Nothing here has been executed.
1. Cloudflare dashboard > R2 > the bucket > Settings > Custom Domains > Connect `data.<your-domain>` (the zone must be on Cloudflare). Wait until the status is Active (TLS is issued automatically).
2. Zone > SSL/TLS: Full (strict), minimum TLS 1.2, Always Use HTTPS, HSTS. Zone > Network: HTTP/2 and HTTP/3 (QUIC) on (both default on). Check later: `curl -sI --http2 https://data.<your-domain>/world/fr/shards.json` shows `HTTP/2 200`, and the response has `alt-svc: h3=...`.
3. CORS: R2 bucket > Settings > CORS policy > paste `r2/cors.json` (origin `https://rail-globe.vercel.app`; add the production domain if there is one; GET and HEAD; `Range` allowed; `Content-Range`, `ETag`, `Content-Length`, `Accept-Ranges` exposed). The custom domain uses the bucket CORS policy. Not needed: a Transform Rule for CORS.
4. Caching (zone > Caching > Cache Rules; the bucket's `Cache-Control` is the source of truth, so the rules only make Cloudflare keep the files):
   - Rule A, "data": hostname equals the data domain > Eligible for cache, Edge TTL "Use cache-control header if present, bypass cache if not", Browser TTL "Respect origin".
   - Do not add a rule that overrides `shards.json`: it must stay `no-cache` (browsers revalidate with the ETag, Cloudflare answers 304).
   - Tiles: `world.pmtiles` is not build-dated today, so it keeps `max-age=3600` and an ETag. To make it `immutable` (31536000), publish it as `world-YYYYMMDD.pmtiles` (`rail-extract world --name`), point the app at that name, upload with `Cache-Control: public, max-age=31536000, immutable`, and delete the previous file after the app deploy.
   - Cloudflare caches range requests on a cacheable object only after it has the whole object; for the large pmtiles file turn on Smart Tiered Cache (zone > Caching > Tiered Cache) and check `cf-cache-status: HIT` on a second `Range` request.
5. Compression (zone > Rules > Compression Rules): Brotli, then gzip, for hostname equals the data domain and the file extension `json`. Never compress `.pmtiles` (the tiles are already compressed, and range offsets must stay byte-exact). Then `curl -sI -H 'Accept-Encoding: br' .../world/fr/shards.json` must show `content-encoding: br`, and a `Range` request on a `.pmtiles` file must show none.
6. Rate limiting and WAF (zone > Security): the rule from step 4 of "R2 bucket" above (300 requests/min/IP), and a WAF custom rule that blocks any method other than GET, HEAD and OPTIONS.
7. Switch the app: set `VITE_DATA_BASE=https://data.<your-domain>/` in Vercel (Production and Preview), replace the r2.dev origin in the `connect-src` and the data entries of `app/vercel.json` with the data domain, remove `ALLOW_R2_DEV=1`, redeploy. The build fails if the two do not match.
8. Re-upload with the fixed headers: `R2_MODE=copy r2/sync.sh -P` (no deletes), then check `curl -sI https://data.<your-domain>/world/fr/shards.json` shows `cache-control: no-cache`.
9. Disable the r2.dev URL: bucket > Settings > Public Development URL > Disable. Check that the old URL now returns 404 or 403 and the site still loads.
10. Final check: cold load of the site with the browser network tab: protocol `h2` or `h3`, `content-encoding: br` on shards, no CORS or CSP errors.

## One command, raw data to live site
`pipeline/publish.sh` (from the repo root) does planet cache, outlines, cut, build, tiles, places, a check of the output on disk, the R2 upload (copy mode), the push, the R2 prune and a live check of R2 (bucket `world/` equals the local `world/`, nothing outside `world/`) and the site. Credentials: copy `r2/.env.example` to `r2/.env` (gitignored). Always start with `pipeline/publish.sh --dry-run`.
Hidden files and folders (such as tool state in `.omc/`) are never uploaded: `sync.sh` filters them out.

## Publish data
```
export R2_ACCOUNT_ID=... R2_BUCKET=... R2_ACCESS_KEY_ID=... R2_SECRET_ACCESS_KEY=...
r2/sync.sh --dry-run   # check first
r2/sync.sh
```
**When the data layout changes** (like the move to shards, 2026-10-16) the live app and the bucket must change together, in this order:
1. `R2_MODE=copy r2/sync.sh -P` (adds the new files, deletes nothing, so the old app keeps working).
2. Push the app; wait for the Vercel deploy.
3. `r2/sync.sh -P` (now deletes the old per-record objects).
A tab left open across step 3 shows "Could not load" on clicks until it is reloaded.

Tiles and side files go under `world/` in the bucket (they are gitignored, so Vercel cannot ship them). `r2/sync.sh` uploads and prunes only `app/public/data/world` -> bucket `world/`; anything else under `app/public/data` can never reach the public bucket, and the bucket holds nothing outside `world/` (the stale objects outside it were deleted on 2026-10-20, 3,424 objects, 1.35 GB). The `live` stage of `pipeline/publish.sh` checks both.
The app never gets write credentials; the browser only does GET/HEAD.

## Vercel
- Import the GitHub repo, **Root Directory `app`**.
- Environment variable (Production and Preview): `VITE_DATA_BASE` (currently the r2.dev URL with `ALLOW_R2_DEV=1`, `https://data.example.com/` after the custom domain). It is public by design; never put secrets in `VITE_*`.
- `app/vercel.json` sets the install (`npm ci --ignore-scripts`), the pre-build check, a strict CSP (own origin + data domain + OpenFreeMap + Google Fonts), HSTS, nosniff, frame denial, COOP/CORP and Permissions-Policy.
- Turn on: Deployment Protection for Preview deployments, 2FA on the Vercel and Cloudflare accounts, GitHub branch protection on `main`.
- Preview URLs differ from the CORS origin, so previews cannot load data unless you add them to `r2/cors.json` (or keep them protected).

## Verify after deploy
`curl -sI https://rail-globe.vercel.app | grep -iE 'content-security|strict-transport'`; `curl -sI -H 'Range: bytes=0-15' https://pub-dd749a156c5c4e869e4d54eb1c24d0c5.r2.dev/world/world.pmtiles` returns 206 (`cf-cache-status` only with a custom domain); browser console shows no CSP violations on a cold load.

## Known gaps
- Google Fonts is a third-party stylesheet (no SRI possible); self-hosting IBM Plex would let the CSP drop two origins.
- `style-src 'unsafe-inline'` is needed by MapLibre and React inline styles.
- OpenFreeMap is a third-party basemap origin.
