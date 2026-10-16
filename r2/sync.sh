#!/usr/bin/env bash
# Publish pipeline output to R2: tiles and side files at the bucket root, glyphs under glyphs/.
# Credentials come from the environment only (never a file in the repo):
#   R2_ACCOUNT_ID, R2_BUCKET, R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY
# Use an R2 API token scoped to "Object Read & Write" on this one bucket.
set -euo pipefail
: "${R2_ACCOUNT_ID:?}" "${R2_BUCKET:?}" "${R2_ACCESS_KEY_ID:?}" "${R2_SECRET_ACCESS_KEY:?}"
cd "$(dirname "$0")/../app/public"

export RCLONE_CONFIG=/dev/null   # no config file: the remote below comes from the environment only
export RCLONE_CONFIG_R2_TYPE=s3 RCLONE_CONFIG_R2_PROVIDER=Cloudflare
export RCLONE_CONFIG_R2_ACCESS_KEY_ID="$R2_ACCESS_KEY_ID" RCLONE_CONFIG_R2_SECRET_ACCESS_KEY="$R2_SECRET_ACCESS_KEY"
export RCLONE_CONFIG_R2_ENDPOINT="https://${R2_ACCOUNT_ID}.r2.cloudflarestorage.com" RCLONE_CONFIG_R2_ACL=private
COMMON=(--checksum --transfers 32 --checkers 32 --fast-list --s3-no-check-bucket --exclude '.DS_Store')

MODE=${R2_MODE:-sync}   # R2_MODE=copy uploads without deleting: use it first when the layout changes (see r2/README.md)
rclone "$MODE" data "R2:${R2_BUCKET}" "${COMMON[@]}" --header-upload 'Cache-Control: public, max-age=3600' "$@"
# glyphs never change under the same name
rclone "$MODE" glyphs "R2:${R2_BUCKET}/glyphs" "${COMMON[@]}" --header-upload 'Cache-Control: public, max-age=31536000, immutable' "$@"
# shards.json holds the shard counts: a cached old copy beside new shard files would look in the wrong shard, so browsers must revalidate it
rclone copy data "R2:${R2_BUCKET}" --include '**/shards.json' --include 'shards.json' "${COMMON[@]}" --header-upload 'Cache-Control: no-cache' "$@"
