#!/usr/bin/env bash
# Clean-room check: fails if any tracked file mentions a term from the banned list.
# The list is stored ROT13-encoded in ci/cleanroom.rot13 so that the list itself
# does not put the terms into the tree.
#
# One file is exempt: tn5250/src/wire.rs. The 5250 Telnet RFCs make the client
# send terminal types and NEW-ENVIRON variable names that carry a vendor
# prefix. Those protocol values live in that module and nowhere else; code
# elsewhere uses its constants by name.
set -euo pipefail
allowed='tn5250/src/wire.rs'
cd "$(dirname "$0")/.."
patterns="$(mktemp)"
trap 'rm -f "$patterns"' EXIT
tr 'A-Za-z' 'N-ZA-Mn-za-m' < ci/cleanroom.rot13 > "$patterns"
mapfile -t files < <(git ls-files -co --exclude-standard | grep -vxF -e ci/cleanroom.rot13 -e "$allowed")
hits=0
for f in "${files[@]}"; do
  [ -f "$f" ] || continue
  if grep -nHIiE -f "$patterns" -- "$f"; then hits=1; fi
done
if [ "$hits" -ne 0 ]; then
  echo "clean-room check failed: banned names found above" >&2
  exit 1
fi
echo "clean-room check passed (${#files[@]} files; $allowed allowlisted)"
