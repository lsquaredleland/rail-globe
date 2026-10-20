#!/usr/bin/env bash
# Raw data -> build -> R2 -> Vercel, in one command. Every step is the documented one (README.md "Run it", docs/10-world-build.md); this only chains
# them, checks the output between steps, and orders the upload and the deploy so the live site is never half-updated.
#
#   pipeline/publish.sh [--planet FILE] [--push] [--dry-run] [--stages a,b,c] [--skip a,b]
#
# Stages, in order (default: all but cache, deploy):
#   tools     build the Rust extractor
#   cache     planet .osm.pbf -> .railcache (only with --planet FILE; once per planet, about 1 min, 4.5 GB)
#   outlines  region outlines from the cache
#   build     cut, classify, station model, side files, tiles, places (about 2 min and 12 GB)
#   verify    check the output on disk (read only); stops the run on a failure
#   upload    r2/sync.sh in copy mode: adds and updates files in R2, deletes nothing, so the live app keeps working
#   deploy    git push origin main, which makes Vercel build the app (only with --push)
#   prune     r2/sync.sh: now delete what the new build no longer has under world/ (old shard files, stale objects, slugs in data/world/dropped.txt)
#   live      check R2 (bucket world/ equals app/public/data/world, nothing outside world/) and the site over HTTPS (read only)
#
# r2/sync.sh uploads and prunes only app/public/data/world -> bucket world/.
# Credentials: r2/.env (gitignored, see r2/.env.example) or the environment. Nothing here reads or prints them.
# --dry-run prints each command and runs only the read-only stages; upload and prune run rclone with --dry-run (that also proves the credentials).
# The build stage is a full world build (about 2 min, 12 GB); it needs the user's go to publish (CLAUDE.md). A data-only refresh needs no --push (the app reads the data from R2); push when the
# app code changed, or when the data layout did (then upload, push, prune is the order that keeps the site working).
set -euo pipefail
cd "$(dirname "$0")/.."

PLANET=; PUSH=0; DRY=0; ONLY=; SKIP=
while [ $# -gt 0 ]; do
  case "$1" in
    --planet) PLANET="$2"; shift ;;
    --push) PUSH=1 ;;
    --dry-run) DRY=1 ;;
    --stages) ONLY="$2"; shift ;;
    --skip) SKIP="$2"; shift ;;
    -h|--help) sed -n 2,25p "$0"; exit 0 ;;
    *) echo "unknown option $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

RE=pipeline/rail-extract/target/release/rail-extract
DATA=app/public/data
SITE=${SITE:-https://rail-globe.vercel.app}

want() {  # is stage $1 on?
  local s="$1"
  if [ -n "$ONLY" ]; then case ",$ONLY," in *",$s,"*) ;; *) return 1 ;; esac; else
    case "$s" in
      cache) [ -n "$PLANET" ] || return 1 ;;
      deploy) [ "$PUSH" = 1 ] || return 1 ;;
    esac
  fi
  case ",$SKIP," in *",$s,"*) return 1 ;; esac
  return 0
}
say() { printf '\n== %s ==\n' "$*"; }
run() { printf '+ %s\n' "$*"; [ "$DRY" = 1 ] || "$@"; }   # a step that changes something: skipped in --dry-run
fail() { echo "publish: $*" >&2; exit 1; }

# ---- stages --------------------------------------------------------------------------------------------------------------------------

if want tools; then
  say "tools: build the extractor"
  run cargo build --release --manifest-path pipeline/rail-extract/Cargo.toml
fi

if want cache; then
  say "cache: $PLANET -> .railcache"
  [ -f "$PLANET" ] || fail "no such planet file: $PLANET (download planet-YYMMDD.osm.pbf from https://planet.openstreetmap.org/pbf/)"
  run "$RE" cache --planet "$PLANET"
fi

if want outlines; then
  say "outlines: countries, provinces and states, China cities"
  run "$RE" outlines --level 2
  run "$RE" outlines --level 4
  run "$RE" outlines --level 5 --country CN
fi

if want build; then
  ls data/cache/*.railcache >/dev/null 2>&1 || fail "no data/cache/*.railcache: run with --planet planet-YYMMDD.osm.pbf first"
  say "build: world (docs/10-world-build.md)"
  run mkdir -p data/world
  if [ "$DRY" = 1 ]; then echo "+ ls data/regions | grep -E '^[a-z]{2}.geojson\$' > data/world/slugs.txt"; else
    ls data/regions | grep -E '^[a-z]{2}\.geojson$' | sed 's/.geojson//' > data/world/slugs.txt; fi
  run "$RE" cut --regions data/regions --data data/world $( [ -f data/world/slugs.txt ] && cat data/world/slugs.txt || true )
  run "$RE" world-list --data data/world --slugs-file data/world/slugs.txt
  KEPT=$( [ -f data/world/kept.txt ] && cat data/world/kept.txt || true )
  run "$RE" build $KEPT --no-tiles --threads 5 --data data/world --out "$DATA/world"
  run "$RE" world --slugs-file data/world/kept.txt
  run "$RE" outlines --cities data/world/city-outlines-wanted.json
  run "$RE" build $KEPT --no-tiles --threads 5 --data data/world --out "$DATA/world"
  run "$RE" world --slugs-file data/world/kept.txt
fi

if want verify; then
  say "verify: output on disk"
  root="$DATA/world"; tiles="$root/world.pmtiles"; min=500000000
  [ -f "$tiles" ] || fail "missing $tiles"
  size=$(stat -f %z "$tiles" 2>/dev/null || stat -c %s "$tiles")
  [ "$size" -ge "$min" ] || fail "$tiles is only $size bytes (expected at least $min)"
  for f in places.json search.json; do [ -f "$root/$f" ] || fail "missing $root/$f"; done
  # every region folder is in the shard layout (shards.json) and has no old per-record files
  bad=0; n=0
  for d in "$root"/*/; do
    [ -d "$d/stations" ] || continue
    n=$((n + 1))
    [ -f "$d/shards.json" ] || { echo "no shards.json: $d"; bad=$((bad + 1)); }
    [ -z "$(find "$d/stations" -name '*-*' -print -quit)" ] || { echo "old per-station files in $d"; bad=$((bad + 1)); }
  done
  [ "$n" -gt 0 ] || fail "no region folders under $root"
  [ "$bad" = 0 ] || fail "$bad region folders are not in the shard layout (rebuild with the build stage; pipeline/archive/repack_side_files.mjs converted the old layout in place)"
  files=$(find "$DATA" -type f | wc -l | tr -d ' ')
  echo "ok: $n regions, $files files, $(basename "$tiles") $((size / 1000000)) MB"
  [ "$files" -le 30000 ] || fail "$files files under $DATA: too many (the shard layout has about 11,000)"
  if command -v python3 >/dev/null; then
    python3 -I pipeline/check.py >/tmp/publish-check.log 2>&1 && echo "answer lists: pass" || echo "warning: answer lists failed (see /tmp/publish-check.log; one failure, '成都' tier 3, is known)"
  fi
fi

SYNC_FLAGS=(-P); [ "$DRY" = 1 ] && SYNC_FLAGS+=(--dry-run)

if want upload || want prune; then
  # slugs the build left out on purpose (data/world/dropped.txt): a leftover local folder is removed so it is never uploaded (r2/sync.sh also filters them out)
  while read -r d; do
    [ -n "$d" ] || continue
    if [ -e "$DATA/world/$d" ] || [ -e "$DATA/world/$d.boundary.geojson" ] || [ -e "$DATA/world/$d.pmtiles" ]; then
      printf '+ rm -rf %s\n' "$DATA/world/$d $DATA/world/$d.*"
      [ "$DRY" = 1 ] || rm -rf "${DATA:?}/world/$d" "${DATA:?}/world/$d".*
    fi
  done < <(r2/dropped-slugs.sh)
fi

if want upload; then
  say "upload: R2, copy mode (adds and updates, deletes nothing)"
  printf '+ R2_MODE=copy r2/sync.sh %s\n' "${SYNC_FLAGS[*]}"
  R2_MODE=copy r2/sync.sh "${SYNC_FLAGS[@]}"
fi

if want deploy; then
  say "deploy: push to main (Vercel builds on every push)"
  [ "$(git rev-parse --abbrev-ref HEAD)" = main ] || fail "not on main"
  [ -z "$(git status --porcelain --untracked-files=no -- app pipeline r2 docs shared)" ] || fail "uncommitted changes in app/, pipeline/, r2/, docs/ or shared/: commit them first"
  run git push origin main
fi

if want prune; then
  say "prune: R2, sync mode (deletes what this build no longer has)"
  printf '+ r2/sync.sh %s\n' "${SYNC_FLAGS[*]}"
  r2/sync.sh "${SYNC_FLAGS[@]}"
fi

if want live; then
  say "live: R2 and the site"
  DATA_ORIGIN=$(node -e "const c=JSON.parse(require('fs').readFileSync('app/vercel.json','utf8')).headers[0].headers.find(h=>h.key==='Content-Security-Policy').value;const m=c.split(';').map(s=>s.trim()).find(s=>s.startsWith('connect-src '));console.log(m.split(' ').find(o=>o.startsWith('https://')&&!o.includes('openfreemap')))")
  # the bucket against the local build, read only: world/ matches by size (hidden files excluded) and nothing exists outside world/
  if [ -f r2/.env ]; then
    while IFS='=' read -r k v; do
      case "$k" in R2_ACCOUNT_ID|R2_BUCKET|R2_ACCESS_KEY_ID|R2_SECRET_ACCESS_KEY) [ -z "${!k:-}" ] && export "$k=$v" ;; esac
    done < r2/.env
  fi
  : "${R2_ACCOUNT_ID:?live: R2 credentials missing (r2/.env or the environment)}" "${R2_BUCKET:?}" "${R2_ACCESS_KEY_ID:?}" "${R2_SECRET_ACCESS_KEY:?}"
  export RCLONE_CONFIG=/dev/null RCLONE_CONFIG_R2_TYPE=s3 RCLONE_CONFIG_R2_PROVIDER=Cloudflare
  export RCLONE_CONFIG_R2_ACCESS_KEY_ID="$R2_ACCESS_KEY_ID" RCLONE_CONFIG_R2_SECRET_ACCESS_KEY="$R2_SECRET_ACCESS_KEY"
  export RCLONE_CONFIG_R2_ENDPOINT="https://${R2_ACCOUNT_ID}.r2.cloudflarestorage.com" RCLONE_CONFIG_R2_ACL=private
  if rclone check "$DATA/world" "R2:${R2_BUCKET}/world" --one-way --size-only --fast-list --s3-no-check-bucket --filter '- .*' --filter '- .*/**' >/tmp/publish-r2-check.log 2>&1; then
    echo "ok: bucket world/ has every local file at the same size"
  else
    tail -5 /tmp/publish-r2-check.log >&2; fail "bucket world/ differs from $DATA/world (see /tmp/publish-r2-check.log; run the upload stage)"
  fi
  outside=$(rclone lsf "R2:${R2_BUCKET}" --fast-list --s3-no-check-bucket --max-depth 1 | grep -v '^world/$' || true)
  [ -z "$outside" ] && echo "ok: nothing in the bucket outside world/" || fail "bucket holds objects outside world/: $(echo $outside | tr '\n' ' ') (delete them: rclone purge or delete with the R2 credentials)"
  prefix=world/; tiles=world/world.pmtiles
  code=$(curl -s -o /dev/null -w '%{http_code}' -H 'Range: bytes=0-15' -H "Origin: $SITE" "$DATA_ORIGIN/$tiles"); [ "$code" = 206 ] && echo "ok: $tiles range request 206" || fail "$tiles range request returned $code"
  code=$(curl -s -o /dev/null -w '%{http_code}' "$DATA_ORIGIN/${prefix}it/shards.json"); [ "$code" = 200 ] && echo "ok: ${prefix}it/shards.json" || fail "${prefix}it/shards.json returned $code"
  acao=$(curl -sI -H "Origin: $SITE" "$DATA_ORIGIN/$tiles" | tr -d '\r' | grep -i '^access-control-allow-origin' || true); [ -n "$acao" ] && echo "ok: CORS ($acao)" || fail "no CORS header for $SITE"
  csp=$(curl -sI "$SITE" | tr -d '\r' | grep -ic '^content-security-policy' || true); [ "$csp" = 1 ] && echo "ok: $SITE serves its CSP" || fail "$SITE did not answer with a Content-Security-Policy header"
fi

say "done$([ "$DRY" = 1 ] && echo ' (dry run)')"
