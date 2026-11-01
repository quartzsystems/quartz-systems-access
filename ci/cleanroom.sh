#!/usr/bin/env bash
# Clean-room check: fails if any tracked file mentions a term from the banned list.
# The list is stored ROT13-encoded in ci/cleanroom.rot13 so that the list itself
# does not put the terms into the tree.
#
# One exemption: the 5250 Telnet RFCs make the client name its device on the
# wire with terminal-type identifiers (the 24x80 display's, the 27x132
# display's, later the printers'). Those tokens, and only that exact upper-case
# shape (name, four digits, model), are blanked before the check. They are
# protocol values, never shown as product names.
set -euo pipefail
exempt='\b[I][B][M]-[0-9]{4}-[0-9A-Z]{1,3}\b'
cd "$(dirname "$0")/.."
patterns="$(mktemp)"
trap 'rm -f "$patterns"' EXIT
tr 'A-Za-z' 'N-ZA-Mn-za-m' < ci/cleanroom.rot13 > "$patterns"
mapfile -t files < <(git ls-files -co --exclude-standard | grep -v '^ci/cleanroom\.rot13$')
hits=0
for f in "${files[@]}"; do
  [ -f "$f" ] || continue
  if sed -E "s/$exempt//g" -- "$f" | grep -nHIiE --label="$f" -f "$patterns"; then hits=1; fi
done
if [ "$hits" -ne 0 ]; then
  echo "clean-room check failed: banned names found above" >&2
  exit 1
fi
echo "clean-room check passed (${#files[@]} files)"
