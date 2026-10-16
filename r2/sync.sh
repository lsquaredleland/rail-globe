#!/usr/bin/env bash
# Publish pipeline output to R2: tiles and side files at the bucket root, glyphs under glyphs/.
# Credentials come from the environment only (never a file in the repo):
#   R2_ACCOUNT_ID, R2_BUCKET, R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY
# Use an R2 API token scoped to "Object Read & Write" on this one bucket.
set -euo pipefail
: "${R2_ACCOUNT_ID:?}" "${R2_BUCKET:?}" "${R2_ACCESS_KEY_ID:?}" "${R2_SECRET_ACCESS_KEY:?}"
cd "$(dirname "$0")/../app/public"

export RCLONE_CONFIG_R2_TYPE=s3 RCLONE_CONFIG_R2_PROVIDER=Cloudflare
export RCLONE_CONFIG_R2_ACCESS_KEY_ID="$R2_ACCESS_KEY_ID" RCLONE_CONFIG_R2_SECRET_ACCESS_KEY="$R2_SECRET_ACCESS_KEY"
export RCLONE_CONFIG_R2_ENDPOINT="https://${R2_ACCOUNT_ID}.r2.cloudflarestorage.com" RCLONE_CONFIG_R2_ACL=private
COMMON=(--checksum --transfers 16 --s3-no-check-bucket --exclude '.DS_Store')

rclone sync data "R2:${R2_BUCKET}" "${COMMON[@]}" --header-upload 'Cache-Control: public, max-age=3600' "$@"
# glyphs never change under the same name
rclone sync glyphs "R2:${R2_BUCKET}/glyphs" "${COMMON[@]}" --header-upload 'Cache-Control: public, max-age=31536000, immutable' "$@"
