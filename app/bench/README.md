# Frame benchmark

`zoom-pan.mjs` drives the production build in Chrome (puppeteer-core) and counts frames the map failed to draw on time while zooming and panning. Measured results are in `docs/reference/research-summary.md`; this file is how to run and read it.

## What it measures, and why

- **Missed vsyncs, not percentiles.** Each frame interval is scored as `round(ms / budget) - 1`, summed. A 50 ms frame at 60 Hz counts 2. Intervals are whole frames, so p95 is always one frame and hides the long ones. p95 and p99 are printed in headed mode only as extra context.
- **Dropped / over:** intervals above 1.5 budgets. 33.3 ms sits exactly on the 2-frame rounding boundary, so the cut is 25 ms (1.5 x 16.7), not 33.
- **Jank ms:** time over budget, summed. **Max:** longest interval. **Long tasks:** main-thread tasks over 50 ms during the move.
- **Settle:** time from the end of a move until the map is idle. This is where tile work shows, because the CPU throttle slows only the main thread, not MapLibre's workers.
- **Budget.** Headless Chrome runs at 60 Hz (16.67 ms). `--headed` on a ProMotion display runs at 120 Hz (8.3 ms), so a frame that is fine headless can be a miss headed. Headed mode takes the budget from the measured idle control. Headless at 60 Hz hides 120 Hz overruns.
- **Control (refresh-halved guard).** Before and after each run, the median rAF interval of an idle page. Above 25 ms the run is flagged: the display fell to 30 Hz (low power, battery, load). Flagged runs stay in the all-runs summary; the default scenario also prints a clean-only one.
- **Warm-up run.** One unmeasured run first (`--warmup`, default 1) fills the HTTP cache for the basemap and the JIT. Skip it only on purpose.

## Setup

1. Production build, never the dev server (it is slower and not what users get):
   ```
   cd app && npm run build && npx vite preview --port 4173   # separate shell
   ```
2. Plug in AC power. Close other apps; the script warns when the 1-minute load average is above 2.
3. For `--headed`, keep the Chrome window visible and in front for the whole run. The script warns if the page is hidden or unfocused.

## Flags

| Flag | Default | Meaning |
| --- | --- | --- |
| `--url <u>` | `http://localhost:4173` | Site to test. Anything not on localhost is external (see below) |
| `--runs <n>` | 3 | Measured runs; results are the median across runs |
| `--warmup <n>` | 1 | Unmeasured runs first |
| `--throttle <x>` | 6 | CPU slowdown on the main thread; 1 = none |
| `--label <s>` | `run` | Prefix of the result file name |
| `--query <q>` | none | Extra URL query for local runs, e.g. `foo=1`; recorded in the result's `env.query` |
| `--chrome <path>` | macOS Chrome | Chrome executable |
| `--scenario fast` | default scenario | Real wheel bursts instead of the slow moves |
| `--headed` | off | Real window; refresh rate follows the display |
| `--delta <n>` | calibrated | Fixed wheel delta per tick; skips calibration (fast only) |
| `--eval '<js>'` | none | JS run in the page after load (fast only) |
| `--trace <dir>` | none | Saves a Chrome trace per view, first run only (fast only) |
| `--latency <ms>` / `--down <Mbps>` | 0 / unlimited | CDP network emulation (RTT, download cap) on the page and its workers, so tile and basemap requests are both slowed; recorded in `env.latencyMs` / `env.downMbps` (fast only). Workers attach after the first requests, so the very first startup fetches may escape it |
| `--start-zoom <z>` | 10 | Zoom the bursts start from (fast only); `--delta 70` gives about z6 to z10, `--delta 127` gives about 4 to 7 levels |
| `--view <names>` | all five | Run only these views, comma-separated, e.g. `"Chengdu East,Shenzhen"` (fast only) |
| `--zoomout` / `--outs <n>` | off / 1 | Zoom-OUT scenario (fast only): per view and cycle (3), jump to `--start-zoom`, settle, then only wheel out (n bursts 300 ms apart); frames are recorded from the first tick to the idle after the last. `idleAfterLastWheelMs` = median settle time. With `--delta 127` one burst is about 6 levels, so use `--start-zoom 14 --outs 2` (reaches z2, the min zoom), `--delta 100 --outs 2` (still z2 to 2.4) or `--start-zoom 12 --outs 1` (about z6) |
| `--tilestate` | off | Per frame, whether the map had every wanted tile loaded (`areTilesLoaded`); saved as `ts` plus burst end `marks`. Costs one call per frame, so do not compare with runs made without it (fast only) |

Examples:

```
node bench/zoom-pan.mjs --runs 3 --throttle 6 --label baseline-t6                 # default scenario
node bench/zoom-pan.mjs --scenario fast --throttle 6 --label fast-t6               # headless, 60 Hz
node bench/zoom-pan.mjs --scenario fast --headed --delta 127 --throttle 4 --label ours-120-t4
node bench/zoom-pan.mjs --scenario fast --eval 'window.__map._fadeDuration=150' --label fade150
node bench/zoom-pan.mjs --scenario fast --trace /tmp/tr --label traced             # use an unminified build
node bench/zoom-pan.mjs --scenario fast --headed --delta 127 --url https://trains.theojaffee.net --label ref-120
```

## Scenarios

- **Default (slow).** Per view: jump to z9, `flyTo` z14 over 2.5 s, `panBy` one screen east over 1.5 s, `easeTo` z8 over 2.5 s. Fixed durations, so runs do identical work. Nearly all misses land in the zoom-in.
- **Fast.** Real mouse-wheel input through MapLibre's scroll-zoom handler. From z10: 40 ticks, one every 25 ms (1 s), then a 300 ms pause, then the same burst out, 3 cycles. Without `--delta` the script first finds the per-tick delta that gives a 4-level burst; use `--delta 127` to repeat a known setup. Also records idle time after the last wheel tick and per-view tile requests (pmtiles, OpenFreeMap tiles and glyphs: requested, aborted, failed).

## Views and why China

Chengdu East, Shenzhen, Guangzhou, Beijing, Shanghai. China has the densest network, and every view draws from the one pilot tile source, so tile cost is the same for all. Compare totals across the five, not single views (single views are noisier).

## A/B comparisons

- Quick preset for iteration: `--view "Chengdu East,Shenzhen" --runs 2 --warmup 1 --headed --delta 127 --throttle 4`, configs alternated (ABAB). Confirm a final winner with the full five views.

- Same script, same flags, same machine state for both sides. Run them back to back.
- Two builds: `git stash` the change under `app/src` and rebuild, or build into two directories (`npx vite build --outDir <dir>`) and serve each on its own port.
- Quick tests need no rebuild: use `--eval` (below).
- Compare totals. Run-to-run noise is about 10% on totals, up to ~20-30% on one view; the summary prints the spread. Treat gains inside the spread as no gain.
- Check `refreshHalved` and `controlMs` in the result before trusting a run.

## Diagnostic toggles with `--eval`

The app exposes `window.__map` when opened with `?debug` (the script adds it for local URLs). Examples:

```
--eval 'window.__map._fadeDuration=150'
--eval 'window.__map.getStyle().layers.filter(l => l.id === "pilot:hl-fs").forEach(l => window.__map.setLayoutProperty(l.id, "visibility", "none"))'
--eval 'window.__map.setPixelRatio(1)'
```

Hiding layers by id shows what each group costs. The eval runs once, after load and before calibration.

## Tracing

Build unminified so function names are readable, serve it, and add `--trace`:

```
cd app && npx vite build --minify false --outDir dist-unmin && npx vite preview --outDir dist-unmin --port 4174
node bench/zoom-pan.mjs --scenario fast --throttle 6 --runs 1 --warmup 0 --trace /tmp/tr --url http://localhost:4174
```

One JSON per view (`<label>-<view>.json`), loadable in Chrome DevTools Performance. Tracing itself inflates misses about 2x, so use traced runs to see where time goes, never to count misses.

## External sites

`--url` on a non-localhost address skips `?debug` and sets the start view with MapLibre's named hash: `#view=z/lat/lon`. The page must expose `window.__map`; `trains.theojaffee.net` does. Idle waits time out after 20 s instead of failing (logged in the result as `_idleTimeouts`). The zoom span differs between sites: the reference clamps at maxZoom 17, so it ends higher than a 4-level burst from z10 would on ours. Say so when comparing.

## Results

Files go to `app/bench/results/` (git-ignored): `<label>-<timestamp>.json`, or `<label>-fast-<timestamp>.json` for the fast scenario. Contents:

- `env`: Chrome version, `commit`, `dirty`, `viewport`, `headed`, `throttle`, `runs`, `warmup`, `vsyncMs`, `dropMs`, `loadAvg` (start) and `loadAvgEnd`, `controlMs` (per run, before / after), `refreshHalved`, `controlLimitMs`, and for the fast scenario `win` (DPR, window and screen size, layer count).
- `raw`: every frame interval and long task per run, view and move, so any run can be re-scored later with a different budget or threshold.
- Default scenario also holds `summary` and `summaryCleanRuns`. Fast results are scored from `raw`; the summary table is printed to the console.
- `eval` and `fast` (scenario constants) are saved with the result, so a file says what was toggled.

## Pitfalls

- The 33 ms threshold sits on the 2-vsync rounding boundary; that is why dropped uses 25 ms.
- Rounded values (`r1`) in printed tables can differ from the raw sums by a hair. Re-score from the JSON.
- At 30 Hz (battery, load) every frame reads ~33 ms. The control catches it; do not compare flagged runs.
- Headless at 60 Hz hides 120 Hz overruns. Check final claims headed.
- Tracing inflates misses about 2x.
- Zoom span differs between sites (see External sites). Compare a site only against itself unless the end zoom is reported.
- The throttle does not slow workers, so it understates tile cost. Slow-network tests use `--latency`; a cold cache is the first measured run with `--warmup 0` (fresh browser profile). `bench/probe.mjs <latencyMs> <url>` prints tiles in view and rendered feature counts per zoom.
- Trackpad pinch input is not simulated; wheel ticks are.

The request table also counts the app's own glyph files (`/glyphs/`: requests / KB) and the PMTiles KB transferred (`encodedDataLength`, cache hits count 0, so it varies with the HTTP cache; do not compare it across runs).
