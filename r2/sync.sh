#!/usr/bin/env bash
# Publish pipeline output to R2: tiles and side files at the bucket root.
# Credentials come from the environment only (never a file in the repo):
#   R2_ACCOUNT_ID, R2_BUCKET, R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY
# Use an R2 API token scoped to "Object Read & Write" on this one bucket.
set -euo pipefail
# Credentials may live in r2/.env (gitignored; copy r2/.env.example). Variables already in the environment win.
HERE="$(cd "$(dirname "$0")" && pwd)"
if [ -f "$HERE/.env" ]; then
  while IFS='=' read -r k v; do
    case "$k" in R2_ACCOUNT_ID|R2_BUCKET|R2_ACCESS_KEY_ID|R2_SECRET_ACCESS_KEY) [ -z "${!k:-}" ] && export "$k=$v" ;; esac
  done < "$HERE/.env"
fi
: "${R2_ACCOUNT_ID:?}" "${R2_BUCKET:?}" "${R2_ACCESS_KEY_ID:?}" "${R2_SECRET_ACCESS_KEY:?}"
cd "$(dirname "$0")/../app/public"

export RCLONE_CONFIG=/dev/null   # no config file: the remote below comes from the environment only
export RCLONE_CONFIG_R2_TYPE=s3 RCLONE_CONFIG_R2_PROVIDER=Cloudflare
export RCLONE_CONFIG_R2_ACCESS_KEY_ID="$R2_ACCESS_KEY_ID" RCLONE_CONFIG_R2_SECRET_ACCESS_KEY="$R2_SECRET_ACCESS_KEY"
export RCLONE_CONFIG_R2_ENDPOINT="https://${R2_ACCOUNT_ID}.r2.cloudflarestorage.com" RCLONE_CONFIG_R2_ACL=private
COMMON=(--checksum --transfers 32 --checkers 32 --fast-list --s3-no-check-bucket)
# ordered --filter rules, first match wins. Never public: hidden files and folders (tool state such as .omc, .DS_Store).
HIDDEN=(--filter '- .*' --filter '- .*/**')

# Slugs in data/world/dropped.txt (minus any the build uses, see dropped-slugs.sh) are never uploaded (a stale local folder cannot reach the bucket) and, in sync mode, are deleted from it.
# The filters go first: first match wins, and an excluded path is also never deleted by sync, so the delete is done explicitly below.
DROP=(); DROPDEL=()
while read -r d; do
  [ -n "$d" ] && { DROP+=(--filter "- /world/$d/**" --filter "- /world/$d.*"); DROPDEL+=(--filter "+ /world/$d/**" --filter "+ /world/$d.*"); }
done < <("$HERE/dropped-slugs.sh")

MODE=${R2_MODE:-sync}   # R2_MODE=copy uploads without deleting: use it first when the layout changes (see r2/README.md)
# data: everything but shards.json (uploaded last, with its own header)
rclone "$MODE" data "R2:${R2_BUCKET}" "${COMMON[@]}" "${HIDDEN[@]}" ${DROP[@]+"${DROP[@]}"} --filter '- shards.json' --filter '- **/shards.json' --header-upload 'Cache-Control: public, max-age=3600' "$@"
# shards.json holds the shard counts: a cached old copy beside new shard files would look in the wrong shard, so browsers must revalidate it.
# --ignore-times re-uploads the ~200 small files even when unchanged, so the header is always right (rclone skips identical files otherwise).
rclone copy data "R2:${R2_BUCKET}" "${COMMON[@]}" ${DROP[@]+"${DROP[@]}"} --filter '+ shards.json' --filter '+ **/shards.json' --filter '- *' --ignore-times --header-upload 'Cache-Control: no-cache' "$@"
# sync mode: remove the dropped slugs from the bucket (their folders and their .boundary.geojson files); copy mode deletes nothing
if [ "$MODE" = sync ] && [ ${#DROPDEL[@]} -gt 0 ]; then
  rclone delete "R2:${R2_BUCKET}" "${COMMON[@]}" "${DROPDEL[@]}" --filter '- *' "$@"
fi
