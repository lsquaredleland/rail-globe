#!/usr/bin/env bash
# Prints the slugs that may be pruned (one per line): those in data/world/dropped.txt, minus any slug the current build uses
# (world.parts.json: skipped with a warning). Prints nothing, with a warning, when kept.txt is missing or empty or overlaps dropped.txt. Used by r2/sync.sh and pipeline/publish.sh; reads files only.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DROPPED="${DROPPED:-$ROOT/data/world/dropped.txt}"
PARTS="${PARTS:-$ROOT/app/public/data/world/world.parts.json}"
KEPT="${KEPT:-$ROOT/data/world/kept.txt}"
[ -f "$DROPPED" ] || exit 0
# consistency guards: with kept.txt missing or empty, or sharing a slug with dropped.txt, the two lists do not describe one build: prune nothing
[ -s "$KEPT" ] || { echo "dropped-slugs: $KEPT is missing or empty: not pruning anything" >&2; exit 0; }
both=$(grep -E '^[a-z]{2}$' "$DROPPED" | sort -u | comm -12 - <(grep -E '^[a-z]{2}$' "$KEPT" | sort -u) || true)
[ -z "$both" ] || { echo "dropped-slugs: $KEPT and $DROPPED both list: $(echo $both): not pruning anything" >&2; exit 0; }
used=$( { [ -f "$PARTS" ] && grep -o '"slug": *"[^"]*"' "$PARTS" | sed 's/.*: *"\(.*\)"/\1/'; } || true )
while read -r d || [ -n "$d" ]; do
  case "$d" in [a-z][a-z]) ;; *) continue ;; esac
  if printf '%s\n' "$used" | grep -qx "$d"; then echo "dropped-slugs: $d is in world.parts.json: skipped" >&2; continue; fi
  echo "$d"
done < "$DROPPED"
