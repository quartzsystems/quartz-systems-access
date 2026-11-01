#!/usr/bin/env bash
# The tn5250 crate must never depend on a UI crate.
set -euo pipefail
cd "$(dirname "$0")/.."
if cargo tree -p tn5250 -e normal,build --prefix none | grep -E '^(fltk|fltk-sys|qsa-ui) '; then
  echo "tn5250 must not depend on a UI crate" >&2
  exit 1
fi
echo "tn5250 has no UI dependencies"
