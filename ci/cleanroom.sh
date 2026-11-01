#!/usr/bin/env bash
# Clean-room check: fails if any tracked file mentions a term from the banned list.
# The list is stored ROT13-encoded in ci/cleanroom.rot13 so that the list itself
# does not put the terms into the tree.
#
# The protocol crates' wire.rs files are exempt, and nothing else. Each
# protocol requires some exact strings that carry a vendor prefix (the 5250
# Telnet RFCs' terminal types and NEW-ENVIRON variable names). Those values live in the crate's wire.rs and
# nowhere else; code elsewhere uses their constants by name. A new protocol
# crate adds its wire.rs to this list.
set -euo pipefail
allowed=(
  tn5250/src/wire.rs
)
cd "$(dirname "$0")/.."
patterns="$(mktemp)"
trap 'rm -f "$patterns"' EXIT
tr 'A-Za-z' 'N-ZA-Mn-za-m' < ci/cleanroom.rot13 > "$patterns"
exclude=(-e ci/cleanroom.rot13)
for a in "${allowed[@]}"; do exclude+=(-e "$a"); done
mapfile -t files < <(git ls-files -co --exclude-standard | grep -vxF "${exclude[@]}")
hits=0
for f in "${files[@]}"; do
  [ -f "$f" ] || continue
  if grep -nHIiE -f "$patterns" -- "$f"; then hits=1; fi
done
if [ "$hits" -ne 0 ]; then
  echo "clean-room check failed: banned names found above" >&2
  exit 1
fi
echo "clean-room check passed (${#files[@]} files; allowlisted: ${allowed[*]})"
