#!/usr/bin/env bash
# Clean-room check: fails if any tracked file mentions a term from the banned list.
# The list is stored ROT13-encoded in ci/cleanroom.rot13 so that the list itself
# does not put the terms into the tree.
set -euo pipefail
cd "$(dirname "$0")/.."
patterns="$(mktemp)"
trap 'rm -f "$patterns"' EXIT
tr 'A-Za-z' 'N-ZA-Mn-za-m' < ci/cleanroom.rot13 > "$patterns"
mapfile -t files < <(git ls-files -co --exclude-standard | grep -v '^ci/cleanroom\.rot13$')
hits=0
for f in "${files[@]}"; do
  [ -f "$f" ] || continue
  if grep -nHIiE -f "$patterns" -- "$f"; then hits=1; fi
done
if [ "$hits" -ne 0 ]; then
  echo "clean-room check failed: banned names found above" >&2
  exit 1
fi
echo "clean-room check passed (${#files[@]} files)"
