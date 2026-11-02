#!/usr/bin/env bash
# The network, protocol and print-data crates must never depend on a UI crate.
set -euo pipefail
cd "$(dirname "$0")/.."
for crate in qsa-net qap-client tn5250 scs; do
  if cargo tree -p "$crate" -e normal,build --prefix none | grep -E '^(fltk|fltk-sys|qsa-ui) '; then
    echo "$crate must not depend on a UI crate" >&2
    exit 1
  fi
  echo "$crate has no UI dependencies"
done
